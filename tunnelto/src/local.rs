use super::*;
use futures::{StreamExt, SinkExt};
use futures::channel::mpsc::{unbounded, UnboundedSender, UnboundedReceiver};

use tokio::net::TcpStream;
use tokio::io::{ReadHalf, WriteHalf};
use tokio::io::{split, AsyncReadExt, AsyncWriteExt};

use crate::introspect;

/// Establish a new local stream and start processing messages to it
pub async fn setup_new_stream(local_port: u16, mut tunnel_tx: UnboundedSender<ControlPacket>, stream_id: StreamId) {
    info!("setting up local stream: {}", &stream_id.to_string());

    let local_tcp = match TcpStream::connect(format!("localhost:{}", local_port)).await {
        Ok(s) => s,
        Err(e) => {
            warn!("failed to connect to local service: {:?}", e);
            introspect::connect_failed();
            let _ = tunnel_tx.send(ControlPacket::Refused(stream_id)).await;
            return
        }
    };
    let (stream, sink) = split(local_tcp);

    // Read local tcp bytes, send them tunnel
    let stream_id_clone = stream_id.clone();
    tokio::spawn(async move {
        process_local_tcp(stream, tunnel_tx, stream_id_clone).await;
    });

    // Forward remote packets to local tcp
    let (tx, rx) = unbounded();
    ACTIVE_STREAMS.write().unwrap().insert(stream_id.clone(), tx.clone());

    tokio::spawn(async move {
        forward_to_local_tcp(stream_id, sink, rx).await;
    });
}

pub async fn process_local_tcp(mut stream: ReadHalf<TcpStream>, mut tunnel: UnboundedSender<ControlPacket>, stream_id: StreamId) {
    let mut buf = [0; 4*1024];

    loop {
        let n = stream.read(&mut buf).await.expect("failed to read data from socket");

        if n == 0 {
            info!("done reading from client stream");
            ACTIVE_STREAMS.write().unwrap().remove(&stream_id);
            return
        }

        let data = buf[..n].to_vec();
        debug!("read from local service: {:?}", std::str::from_utf8(&data).unwrap_or("<non utf8>"));

        let packet = ControlPacket::Data(stream_id.clone(), data.clone());
        tunnel.send(packet).await.expect("failed to tunnel packet from local tcp to tunnel");

        let stream_id_clone =  stream_id.clone();
        introspect::log_outgoing(stream_id_clone, data);
    }
}

async fn forward_to_local_tcp(stream_id: StreamId, mut sink: WriteHalf<TcpStream>, mut queue: UnboundedReceiver<StreamMessage>) {
    loop {
        let data = match queue.next().await {
            Some(StreamMessage::Data(data)) => data,
            None | Some(StreamMessage::Close) => {
                warn!("closing stream");
                let _ = sink.shutdown().await.map_err(|e| {
                    error!("failed to shutdown: {:?}", e);
                });
                return
            }
        };

        sink.write_all(&data).await.expect("failed to write packet data to local tcp socket");
        debug!("wrote to local service: {:?}", data.len());

        let stream_id_clone =  stream_id.clone();
        introspect::log_incoming(stream_id_clone, data);
    }
}