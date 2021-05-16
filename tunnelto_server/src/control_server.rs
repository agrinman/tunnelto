pub use super::*;
use crate::auth::reconnect_token::ReconnectTokenPayload;
use crate::client_auth::ClientHandshake;
use chrono::Utc;
use std::net::{IpAddr, SocketAddr};
use std::str::FromStr;
use std::time::Duration;
use tracing::{error, Instrument};
use warp::Rejection;

pub fn spawn<A: Into<SocketAddr>>(addr: A) {
    let health_check = warp::get().and(warp::path("health_check")).map(|| {
        tracing::debug!("Health Check #2 triggered");
        "ok"
    });
    let client_conn = warp::path("wormhole").and(client_ip()).and(warp::ws()).map(
        move |client_ip: IpAddr, ws: Ws| {
            ws.on_upgrade(move |w| {
                async move { handle_new_connection(client_ip, w).await }
                    .instrument(observability::remote_trace("handle_websocket"))
            })
        },
    );

    let routes = client_conn.or(health_check);

    // spawn our websocket control server
    tokio::spawn(warp::serve(routes).run(addr.into()));
}

fn client_ip() -> impl Filter<Extract = (IpAddr,), Error = Rejection> + Copy {
    warp::any()
        .and(warp::header::optional("Fly-Client-IP"))
        .and(warp::header::optional("X-Forwarded-For"))
        .and(warp::addr::remote())
        .map(
            |client_ip: Option<String>, fwd: Option<String>, remote: Option<SocketAddr>| {
                let client_ip = client_ip.map(|s| IpAddr::from_str(&s).ok()).flatten();
                let fwd = fwd
                    .map(|s| {
                        s.split(",")
                            .into_iter()
                            .next()
                            .map(IpAddr::from_str)
                            .map(Result::ok)
                            .flatten()
                    })
                    .flatten();
                let remote = remote.map(|r| r.ip());
                client_ip
                    .or(fwd)
                    .or(remote)
                    .unwrap_or(IpAddr::from([0, 0, 0, 0]))
            },
        )
}

#[tracing::instrument(skip(websocket))]
async fn handle_new_connection(client_ip: IpAddr, websocket: WebSocket) {
    // check if this client is blocked
    if CONFIG.blocked_ips.contains(&client_ip) {
        tracing::warn!(?client_ip, "client ip is on block list, denying connection");
        let _ = websocket.close().await;
        return;
    }

    let (websocket, handshake) = match try_client_handshake(websocket).await {
        Some(ws) => ws,
        None => return,
    };

    tracing::info!(client_ip=%client_ip, subdomain=%handshake.sub_domain, "open tunnel");

    let (tx, rx) = unbounded::<ControlPacket>();
    let mut client = ConnectedClient {
        id: handshake.id,
        host: handshake.sub_domain,
        is_anonymous: handshake.is_anonymous,
        tx,
    };
    Connections::add(client.clone());

    let (sink, stream) = websocket.split();

    let client_clone = client.clone();

    tokio::spawn(
        async move {
            tunnel_client(client_clone, sink, rx).await;
        }
        .instrument(observability::remote_trace("tunnel_client")),
    );

    let client_clone = client.clone();

    tokio::spawn(
        async move {
            process_client_messages(client_clone, stream).await;
        }
        .instrument(observability::remote_trace("process_client")),
    );

    // play ping pong
    tokio::spawn(
        async move {
            loop {
                tracing::trace!("sending ping");

                // create a new reconnect token for anonymous clients
                let reconnect_token = if client.is_anonymous {
                    ReconnectTokenPayload {
                        sub_domain: client.host.clone(),
                        client_id: client.id.clone(),
                        expires: Utc::now() + chrono::Duration::minutes(2),
                    }
                    .into_token(&CONFIG.master_sig_key)
                    .map_err(|e| error!("unable to create reconnect token: {:?}", e))
                    .ok()
                } else {
                    None
                };

                match client.tx.send(ControlPacket::Ping(reconnect_token)).await {
                    Ok(_) => {}
                    Err(e) => {
                        tracing::debug!("Failed to send ping: {:?}, removing client", e);
                        Connections::remove(&client);
                        return;
                    }
                };

                tokio::time::sleep(Duration::new(PING_INTERVAL, 0)).await;
            }
        }
        .instrument(observability::remote_trace("control_ping")),
    );
}

#[tracing::instrument(skip(websocket))]
async fn try_client_handshake(websocket: WebSocket) -> Option<(WebSocket, ClientHandshake)> {
    // Authenticate client handshake
    let (mut websocket, client_handshake) = client_auth::auth_client_handshake(websocket).await?;

    // Send server hello success
    let data = serde_json::to_vec(&ServerHello::Success {
        sub_domain: client_handshake.sub_domain.clone(),
        hostname: format!("{}.{}", &client_handshake.sub_domain, CONFIG.tunnel_host),
        client_id: client_handshake.id.clone(),
    })
    .unwrap_or_default();

    let send_result = websocket.send(Message::binary(data)).await;
    if let Err(error) = send_result {
        error!(?error, "aborting...failed to write server hello");
        return None;
    }

    tracing::debug!(
        "new client connected: {:?}{}",
        &client_handshake.id,
        if client_handshake.is_anonymous {
            " (anonymous)"
        } else {
            ""
        }
    );
    Some((websocket, client_handshake))
}

/// Send the client a "stream init" message
pub async fn send_client_stream_init(mut stream: ActiveStream) {
    match stream
        .client
        .tx
        .send(ControlPacket::Init(stream.id.clone()))
        .await
    {
        Ok(_) => {
            tracing::debug!("sent control to client: {}", &stream.client.id);
        }
        Err(_) => {
            tracing::debug!("removing disconnected client: {}", &stream.client.id);
            Connections::remove(&stream.client);
        }
    }
}

/// Process client control messages
#[tracing::instrument(skip(client_conn))]
async fn process_client_messages(client: ConnectedClient, mut client_conn: SplitStream<WebSocket>) {
    loop {
        let result = client_conn.next().await;

        let message = match result {
            // handle protocol message
            Some(Ok(msg)) if (msg.is_binary() || msg.is_text()) && !msg.as_bytes().is_empty() => {
                msg.into_bytes()
            }
            // handle close with reason
            Some(Ok(msg)) if msg.is_close() && !msg.as_bytes().is_empty() => {
                tracing::debug!(close_reason=?msg, "got close");
                Connections::remove(&client);
                return;
            }
            _ => {
                tracing::debug!(?client.id, "goodbye client");
                Connections::remove(&client);
                return;
            }
        };

        let packet = match ControlPacket::deserialize(&message) {
            Ok(packet) => packet,
            Err(error) => {
                error!(?error, "invalid data packet");
                continue;
            }
        };

        let (stream_id, message) = match packet {
            ControlPacket::Data(stream_id, data) => {
                tracing::debug!(?stream_id, num_bytes=?data.len(),"forwarding to stream");
                (stream_id, StreamMessage::Data(data))
            }
            ControlPacket::Refused(stream_id) => {
                tracing::debug!("tunnel says: refused");
                (stream_id, StreamMessage::TunnelRefused)
            }
            ControlPacket::Init(_) | ControlPacket::End(_) => {
                error!("invalid protocol control::init message");
                continue;
            }
            ControlPacket::Ping(_) => {
                tracing::trace!("pong");
                Connections::add(client.clone());
                continue;
            }
        };

        let stream = ACTIVE_STREAMS.get(&stream_id).map(|s| s.value().clone());

        if let Some(mut stream) = stream {
            let _ = stream.tx.send(message).await.map_err(|error| {
                tracing::trace!(?error, "Failed to send to stream tx");
            });
        }
    }
}

#[tracing::instrument(skip(sink, queue))]
async fn tunnel_client(
    client: ConnectedClient,
    mut sink: SplitSink<WebSocket, Message>,
    mut queue: UnboundedReceiver<ControlPacket>,
) {
    loop {
        match queue.next().await {
            Some(packet) => {
                let result = sink.send(Message::binary(packet.serialize())).await;
                if let Err(error) = result {
                    tracing::trace!(?error, "client disconnected: aborting.");
                    Connections::remove(&client);
                    return;
                }
            }
            None => {
                tracing::debug!("ending client tunnel");
                return;
            }
        };
    }
}
