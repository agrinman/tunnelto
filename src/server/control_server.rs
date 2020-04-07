pub use super::*;
use std::net::SocketAddr;

pub fn spawn<A: Into<SocketAddr>>(addr: A) {
    let health_check = warp::get().and(warp::path("health_check")).map(|| "ok");
    let client_conn = warp::path("wormhole").and(warp::ws()).map(move |ws: Ws| {
        ws.on_upgrade(handle_new_connection)
    });

    // spawn our websocket control server
    tokio::spawn(warp::serve(client_conn.or(health_check)).run(addr.into()));
}

async fn handle_new_connection(websocket: WebSocket) {
    let (websocket, client_id, sub_domain) = match try_client_handshake(websocket).await {
        Some(ws) => ws,
        None => return,
    };

    let (tx, rx) = unbounded::<ControlPacket>();
    let client = ConnectedClient { id: client_id, host: sub_domain, tx };
    Connections::add(client.clone());

    let  (sink, stream) = websocket.split();

    tokio::spawn(async move {
        tunnel_client(client, sink, rx).await;
    });

    tokio::spawn(async move {
        process_client_messages(stream).await;
    });
}

async fn try_client_handshake(mut websocket: WebSocket) -> Option<(WebSocket, ClientId, String)> {
    // Wait for control hello
    let client_hello_data = match websocket.next().await {
        Some(Ok(msg)) => msg,
        _ => {
            error!("no client init message");
            return None
        },
    };

    let client_hello = ClientHello::verify(&SECRET_KEY, client_hello_data.as_bytes()).map_err(|e| format!("{:?}", e));
    let (client_hello, sub_domain) = match  client_hello {
        Ok(ch) => {
            // check that the subdomain is available and valid
            let sub_domain = if let Some(sub_domain) = &ch.sub_domain {
                // ignore uppercase
                let sub_domain = sub_domain.to_lowercase();

                if sub_domain.chars().filter(|c| !c.is_alphanumeric()).count() > 0 {
                    error!("invalid client hello: only alphanumeric chars allowed!");
                    let data = serde_json::to_vec(&ServerHello::InvalidSubDomain).unwrap_or_default();
                    let _ = websocket.send(Message::binary(data)).await;
                    return None
                }

                let existing_client = Connections::client_for_host(&sub_domain);
                if existing_client.is_some() && Some(&ch.id) != existing_client.as_ref() {
                    error!("invalid client hello: requested sub domain in use already!");
                    let data = serde_json::to_vec(&ServerHello::SubDomainInUse).unwrap_or_default();
                    let _ = websocket.send(Message::binary(data)).await;
                    return None
                }

                sub_domain
            } else {
                ServerHello::random_domain()
            };

            (ch, sub_domain)
        },
        Err(e) => {
            error!("invalid client hello: {}", e);
            let data = serde_json::to_vec(&ServerHello::AuthFailed).unwrap_or_default();
            let _ = websocket.send(Message::binary(data)).await;
            return None
        }
    };

    // Send server hello success
    let data = serde_json::to_vec(&ServerHello::Success { sub_domain: sub_domain.clone() }).unwrap_or_default();
    let send_result = websocket.send(Message::binary(data)).await;
    if let Err(e) = send_result {
        error!("aborting...failed to write server hello: {:?}", e);
        return None
    }

    info!("new client connected: {:?}", &client_hello.id);
    Some((websocket, client_hello.id, sub_domain))
}

/// Send the client a "stream init" message
pub async fn send_client_stream_init(mut stream: ActiveStream) {
    match stream.client.tx.send(ControlPacket::Init(stream.id.clone())).await {
        Ok(_) => {
            info!("sent control to client: {}", &stream.client.id);
        },
        Err(_) => {
            info!("removing disconnected client: {}", &stream.client.id);
            Connections::remove(&stream.client);
        }
    }

}

/// Process client control messages
async fn process_client_messages(mut client_conn: SplitStream<WebSocket>) {
    loop {
        let result = client_conn.next().await;

        let message = match result {
            Some(Ok(msg)) if !msg.as_bytes().is_empty() => msg,
            _ => {
                eprintln!("ending recv on websocket stream");
                return
            },
        };

        let (stream_id, data) = match ControlPacket::deserialize(message.as_bytes()) {
            Ok(ControlPacket::Data(stream_id, data)) => (stream_id, data),
            Ok(ControlPacket::Init(_)) => {
                eprintln!("invalid protocol control::init message");
                continue
            }
            Err(e) => {
                eprintln!("invalid data packet: {:?}", e);
                continue
            }
        };

        let stream = ACTIVE_STREAMS.read().unwrap().get(&stream_id).cloned();

        if let Some(mut stream) = stream {
            let str_contents  = std::str::from_utf8(&data).unwrap_or("<non-utf8 response>");
            eprintln!("sending to stream[id={}]: {:?}", &stream_id.to_string(), str_contents);

            stream.tx.send(StreamMessage::Data(data)).await.expect("failed to send data to server from websocket");
        }
    }
}

async fn tunnel_client(client: ConnectedClient, mut sink: SplitSink<WebSocket, Message>, mut queue: UnboundedReceiver<ControlPacket>) {
    loop {
        match queue.next().await {
            Some(packet) => {
                let result = sink.send(Message::binary(packet.serialize())).await;
                if result.is_err() {
                    eprintln!("client disconnected: aborting.");
                    Connections::remove(&client);
                    return
                }
            },
            None => {
                eprintln!("ending client tunnel");
                return
            },
        };

    }
}