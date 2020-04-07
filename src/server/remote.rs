use super::*;

pub async fn accept_connection(socket: TcpStream) {
    // peek the host of the http request
    // if health check, then handle it and return
    let (mut socket, host) = match peek_http_request_host(socket).await {
        Some(s) => s,
        None => return,
    };

    // parse the host string and find our client
    if ALLOWED_HOSTS.contains(&host) {
        error!("redirect to homepage");
        let _ = socket.write_all(HTTP_REDIRECT_RESPONSE).await;
        return

    }
    let host = match validate_host_prefix(&host) {
        Some(sub_domain) => sub_domain,
        None => {
            error!("invalid host specified");
            let _ = socket.write_all(HTTP_INVALID_HOST_RESPONSE).await;
            return
        }
    };

    // find the client listening for this host
    let client = match Connections::find_by_host(&host) {
        Some(client) => client.clone(),
        None => {
            error!("No tunnel found for host: {}.<>", host);
            let _ = socket.write_all(HTTP_NOT_FOUND_RESPONSE).await;
            return
        }
    };

    // allocate a new stream for this request
    let (active_stream, queue_rx) = ActiveStream::new(client.clone());
    let stream_id = active_stream.id.clone();

    info!("new stream connected: {}", active_stream.id.to_string());
    let (stream, sink) = tokio::io::split(socket);

    // add our stream
    ACTIVE_STREAMS.write().unwrap().insert(stream_id.clone(), active_stream.clone());

    // read from socket, write to client
    tokio::spawn(async move {
        process_tcp_stream(active_stream, stream).await;
    });

    // read from client, write to socket
    tokio::spawn( async move {
        tunnel_to_stream(stream_id, sink, queue_rx).await;
    });
}

fn validate_host_prefix(host: &str) -> Option<String> {
    let host = match url::Host::parse(&host) {
        Ok(url::Host::Domain(domain)) => {
            domain
        },
        Ok(url::Host::Ipv4(_)) | Ok(url::Host::Ipv6(_)) => {
            error!("unsupported IpAddress as host");
            return None
        }
        Err(e) => {
            error!("invalid host header: {:?}", e);
            return None
        }
    };

    let domain_segments = host.split(".").collect::<Vec<&str>>();
    let prefix = &domain_segments[0];
    let remaining = &domain_segments[1..].join(".");

    if ALLOWED_HOSTS.contains(remaining) {
        Some(prefix.to_string())
    } else {
        None
    }
}

/// Response Constants
const HTTP_REDIRECT_RESPONSE:&'static [u8] = b"HTTP/1.1 301 Moved Permanently\r\nLocation: https://alexgr.in/\r\nContent-Length: 17\r\n\r\nhttps://alexgr.in";
const HTTP_INVALID_HOST_RESPONSE:&'static [u8] = b"HTTP/1.1 400\r\nContent-Length: 23\r\n\r\nError: Invalid Hostname";
const HTTP_NOT_FOUND_RESPONSE:&'static [u8] = b"HTTP/1.1 400\r\nContent-Length: 23\r\n\r\nError: Tunnel Not Found";
const HTTP_OK_RESPONSE:&'static [u8] = b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok";
const HEALTH_CHECK_PATH:&'static [u8] = b"/0xDEADBEEF_HEALTH_CHECK";

/// Filter incoming remote streams
async fn peek_http_request_host(mut socket: TcpStream) -> Option<(TcpStream, String)> {
    /// Note we return out if the host header is not found
    /// within the first 4kb of the request.
    const MAX_HEADER_PEAK:usize = 1024;
    let mut buf = vec![0; MAX_HEADER_PEAK]; //4kb

    loop {
        let n = match socket.peek(&mut buf).await {
            Ok(n) => n,
            Err(e) => {
                error!("failed to read from tcp socket to determine host: {:?}", e);
                return None
            },
        };

        let mut headers = [httparse::EMPTY_HEADER; 30]; // 30 seems like a generous # of headers
        let mut req = httparse::Request::new(&mut headers);

        if let Err(e) = req.parse(&buf[..n]) {
            error!("failed to parse incoming http bytes: {:?}", e);
            return None
        }

        // Handle the health check route
        if req.path.map(|s| s.as_bytes()) == Some(HEALTH_CHECK_PATH) {
            info!("Health Check Triggered");

            let _ = socket.write_all(HTTP_OK_RESPONSE).await.map_err(|e| {
                error!("failed to write health_check: {:?}", e);
            });

            return None
        }

        // look for a host header
        if let Some(Ok(host)) = req.headers.iter()
            .filter(|h| h.name.to_lowercase() == "host".to_string())
            .map(|h| std::str::from_utf8(h.value))
            .next()
        {
            return Some((socket, host.to_string()))
        }

        if n == MAX_HEADER_PEAK {
            error!("header too long, could not determine host!");
            return None
        }
    }
}

/// Process Messages from the control path in & out of the remote stream
async fn process_tcp_stream(mut tunnel_stream: ActiveStream, mut tcp_stream: ReadHalf<TcpStream>) {
    // send initial control stream init to client
    control_server::send_client_stream_init(tunnel_stream.clone()).await;

    // now read from stream and forward to clients
    let mut buf = [0; 2048];

    loop {
        // client is no longer connected
        if Connections::get(&tunnel_stream.client.id).is_none() {
            info!("client disconnected, closing stream");
            let _ = tunnel_stream.tx.send(StreamMessage::NoClientTunnel).await;
            tunnel_stream.tx.close_channel();
            return
        }

        // read from stream
        let n = match tcp_stream.read(&mut buf).await {
            Ok(n) => n,
            Err(e) => {
                eprintln!("failed to read from tcp socket: {:?}", e);
                return
            }
        };

        if n == 0 {
            info!("stream ended");
            return;
        }

        info!("read {} bytes", n);

        let data = &buf[..n];
        let packet = ControlPacket::Data(tunnel_stream.id.clone(), data.to_vec());

        match tunnel_stream.client.tx.send(packet.clone()).await {
            Ok(_) => info!("sent data packet to client: {}", &tunnel_stream.client.id),
            Err(_) => {
                error!("failed to forward tcp packets to disconnected client. dropping client.");
                Connections::remove(&tunnel_stream.client);
            }
        }
    }
}

async fn tunnel_to_stream(stream_id: StreamId, mut sink: WriteHalf<TcpStream>, mut queue: UnboundedReceiver<StreamMessage>) {
    loop {
        let result = queue.next().await;

        let data = match result {
            Some(StreamMessage::Data(data)) => data,
            _ => {
                info!("done tunneling to sink");
                let _ = sink.write_all(HTTP_NOT_FOUND_RESPONSE).await;
                let _ = sink.shutdown().await.map_err(|_e| {
                    error!("error shutting down tcp stream");
                });

                ACTIVE_STREAMS.write().unwrap().remove(&stream_id);

                return;
            },
        };

        let result = sink.write_all(&data).await;

        if result.is_err() {
            info!("stream closed, disconnecting");
            return
        }
    }
}
