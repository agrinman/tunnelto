use super::*;
use futures::{StreamExt, SinkExt};
use futures::channel::mpsc::{unbounded, UnboundedSender, UnboundedReceiver};

use tokio::net::TcpStream;
use tokio::io::{ReadHalf, WriteHalf};
use tokio::io::{split, AsyncReadExt, AsyncWriteExt};


/// Establish a new local stream and start processing messages to it
pub async fn setup_new_stream(local_port: &str, tunnel_tx: UnboundedSender<ControlPacket>, stream_id: StreamId) {
    info!("setting up local stream: {}", &stream_id.to_string());

    let local_tcp = TcpStream::connect(format!("localhost:{}", &local_port)).await.expect("failed to connect to local service");
    let (stream, sink) = split(local_tcp);

    // Read local tcp bytes, send them tunnel
    let stream_id_clone = stream_id.clone();
    tokio::spawn(async move {
        process_local_tcp(stream, tunnel_tx, stream_id_clone).await;
    });

    // Forward remote packets to local tcp
    let (tx, rx) = unbounded::<Vec<u8>>();
    ACTIVE_STREAMS.write().unwrap().insert(stream_id, tx.clone());

    tokio::spawn(async move {
        forward_to_local_tcp(sink, rx).await;
    });
}

pub async fn process_local_tcp(mut stream: ReadHalf<TcpStream>, mut tunnel: UnboundedSender<ControlPacket>, stream_id: StreamId) {
    let mut buf = [0; 4096];

    loop {
        let n = stream.read(&mut buf).await.expect("failed to read data from socket");

        if n == 0 {
            info!("done reading from client stream");
            return
        }

        let data = buf[..n].to_vec();
        debug!("read from local service: {:?}", std::str::from_utf8(&data).unwrap_or("<non utf8>"));

        let packet = ControlPacket::Data(stream_id.clone(), data);
        tunnel.send(packet).await.expect("failed to tunnel packet from local tcp to tunnel");
    }
}

async fn forward_to_local_tcp(mut sink: WriteHalf<TcpStream>, mut queue: UnboundedReceiver<Vec<u8>>) {
    loop {
        let data = match queue.next().await {
            Some(data) => data,
            None => {
                warn!("local forward queue is empty");
                return
            }
        };

        sink.write_all(&data).await.expect("failed to write packet data to local tcp socket");
        debug!("wrote to local service: {:?}", std::str::from_utf8(&data).unwrap_or("<non utf8>"));
    }
}