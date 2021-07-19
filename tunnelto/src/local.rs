use super::*;
use futures::channel::mpsc::{unbounded, UnboundedReceiver, UnboundedSender};
use futures::{SinkExt, StreamExt};

use tokio::io::{split, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::io::{ReadHalf, WriteHalf};
use tokio::net::TcpStream;
use tokio_rustls::rustls::ClientConfig;
use tokio_rustls::webpki::DNSNameRef;
use tokio_rustls::TlsConnector;

use crate::introspect::{self, introspect_stream, IntrospectChannels};

pub trait AnyTcpStream: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> AnyTcpStream for T {}

/// Establish a new local stream and start processing messages to it
pub async fn setup_new_stream(
    config: Config,
    mut tunnel_tx: UnboundedSender<ControlPacket>,
    stream_id: StreamId,
) -> Option<UnboundedSender<StreamMessage>> {
    info!("setting up local stream: {}", &stream_id.to_string());

    let local_tcp = match TcpStream::connect(config.local_addr).await {
        Ok(s) => s,
        Err(e) => {
            error!("failed to connect to local service: {}", e);
            introspect::connect_failed();
            let _ = tunnel_tx.send(ControlPacket::Refused(stream_id)).await;
            return None;
        }
    };

    let local_tcp: Box<dyn AnyTcpStream> = if config.use_tls {
        let dnsname = config.local_host;
        let mut config = ClientConfig::new();
        config
            .root_store
            .add_server_trust_anchors(&webpki_roots::TLS_SERVER_ROOTS);
        let config = TlsConnector::from(Arc::new(config));
        let dnsname =
            DNSNameRef::try_from_ascii_str(dnsname.as_str()).ok()?;

        let stream = match config.connect(dnsname, local_tcp).await {
            Ok(s) => s,
            Err(e) => {
                error!("failed to connect to TLS service: {}", e);
                introspect::connect_failed();
                let _ = tunnel_tx.send(ControlPacket::Refused(stream_id)).await;
                return None;
            }
        };

        Box::new(stream)
    } else {
        Box::new(local_tcp)
    };

    let IntrospectChannels {
        request: introspect_request,
        response: introspect_response,
    } = introspect_stream();

    let (stream, sink) = split(local_tcp);

    // Read local tcp bytes, send them tunnel
    let stream_id_clone = stream_id.clone();
    tokio::spawn(async move {
        process_local_tcp(stream, tunnel_tx, stream_id_clone, introspect_response).await;
    });

    // Forward remote packets to local tcp
    let (tx, rx) = unbounded();
    ACTIVE_STREAMS
        .write()
        .unwrap()
        .insert(stream_id.clone(), tx.clone());

    tokio::spawn(async move {
        forward_to_local_tcp(sink, rx, introspect_request).await;
    });

    Some(tx)
}

pub async fn process_local_tcp<T>(
    mut stream: ReadHalf<T>,
    mut tunnel: UnboundedSender<ControlPacket>,
    stream_id: StreamId,
    mut introspect: UnboundedSender<Vec<u8>>,
) where
    T: AnyTcpStream,
{
    let mut buf = [0; 4 * 1024];

    loop {
        let n = stream
            .read(&mut buf)
            .await
            .expect("failed to read data from socket");

        if n == 0 {
            info!("done reading from client stream");
            ACTIVE_STREAMS.write().unwrap().remove(&stream_id);
            return;
        }

        let data = buf[..n].to_vec();
        debug!(
            "read from local service: {:?}",
            std::str::from_utf8(&data).unwrap_or("<non utf8>")
        );

        let packet = ControlPacket::Data(stream_id.clone(), data.clone());
        tunnel
            .send(packet)
            .await
            .expect("failed to tunnel packet from local tcp to tunnel");

        let _ = introspect.send(data).await;
    }
}

async fn forward_to_local_tcp<T>(
    mut sink: WriteHalf<T>,
    mut queue: UnboundedReceiver<StreamMessage>,
    mut introspect: UnboundedSender<Vec<u8>>,
) where
    T: AnyTcpStream,
{
    loop {
        let data = match queue.next().await {
            Some(StreamMessage::Data(data)) => data,
            None | Some(StreamMessage::Close) => {
                warn!("closing stream");
                let _ = sink.shutdown().await.map_err(|e| {
                    error!("failed to shutdown: {:?}", e);
                });
                return;
            }
        };

        sink.write_all(&data)
            .await
            .expect("failed to write packet data to local tcp socket");
        debug!("wrote to local service: {:?}", data.len());

        let _ = introspect.send(data).await;
    }
}
