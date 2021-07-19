use futures::channel::mpsc::{unbounded, UnboundedSender};
use futures::{SinkExt, StreamExt};

use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

use human_panic::setup_panic;
pub use log::{debug, error, info, warn};

use std::collections::HashMap;
use std::env;
use std::net::SocketAddr;
use std::sync::{Arc, RwLock};

mod cli_ui;
mod config;
mod error;
mod introspect;
mod local;
mod update;
pub use self::error::*;

pub use config::*;
pub use tunnelto_lib::*;

use crate::cli_ui::CliInterface;
use colored::Colorize;
use futures::future::Either;
use std::time::Duration;
use tokio::sync::Mutex;

pub type ActiveStreams = Arc<RwLock<HashMap<StreamId, UnboundedSender<StreamMessage>>>>;

lazy_static::lazy_static! {
    pub static ref ACTIVE_STREAMS:ActiveStreams = Arc::new(RwLock::new(HashMap::new()));
    pub static ref RECONNECT_TOKEN: Arc<Mutex<Option<ReconnectToken>>> = Arc::new(Mutex::new(None));
}

#[derive(Debug, Clone)]
pub enum StreamMessage {
    Data(Vec<u8>),
    Close,
}

#[tokio::main]
async fn main() {
    let mut config = match Config::get() {
        Ok(config) => config,
        Err(_) => return,
    };

    setup_panic!();

    update::check().await;

    let introspect_dash_addr = introspect::start_introspect_web_dashboard(config.clone());

    loop {
        let (restart_tx, mut restart_rx) = unbounded();
        let wormhole = run_wormhole(config.clone(), introspect_dash_addr.clone(), restart_tx);
        let result = futures::future::select(Box::pin(wormhole), restart_rx.next()).await;
        config.first_run = false;

        match result {
            Either::Left((Err(e), _)) => match e {
                Error::WebSocketError(_) | Error::NoResponseFromServer | Error::Timeout => {
                    error!("Control error: {:?}. Retrying in 5 seconds.", e);
                    tokio::time::sleep(Duration::from_secs(5)).await;
                }
                Error::AuthenticationFailed => {
                    if config.secret_key.is_none() {
                        eprintln!(
                            ">> {}",
                            "Please use an access key with the `--key` option".yellow()
                        );
                        eprintln!(
                            ">> {}{}",
                            "You can get your access key here: ".yellow(),
                            "https://dashboard.tunnelto.dev".yellow().underline()
                        );
                    } else {
                        eprintln!(
                            ">> {}{}",
                            "Please check your access key at ".yellow(),
                            "https://dashboard.tunnelto.dev".yellow().underline()
                        );
                    }
                    eprintln!("\nError: {}", format!("{}", e).red());
                    return;
                }
                _ => {
                    eprintln!("Error: {}", format!("{}", e).red());
                    return;
                }
            },
            Either::Right((Some(e), _)) => {
                warn!("restarting in 3 seconds...from error: {:?}", e);
                tokio::time::sleep(Duration::from_secs(3)).await;
            }
            _ => {}
        };

        info!("restarting wormhole");
    }
}

/// Setup the tunnel to our control server
async fn run_wormhole(
    config: Config,
    introspect_web_addr: SocketAddr,
    mut restart_tx: UnboundedSender<Option<Error>>,
) -> Result<(), Error> {
    let interface = CliInterface::start(config.clone(), introspect_web_addr);
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    let Wormhole {
        websocket,
        sub_domain,
        hostname,
    } = connect_to_wormhole(&config).await?;

    interface.did_connect(&sub_domain, &hostname);

    // split reading and writing
    let (mut ws_sink, mut ws_stream) = websocket.split();

    // tunnel channel
    let (tunnel_tx, mut tunnel_rx) = unbounded::<ControlPacket>();

    // continuously write to websocket tunnel
    let mut restart = restart_tx.clone();
    tokio::spawn(async move {
        loop {
            let packet = match tunnel_rx.next().await {
                Some(data) => data,
                None => {
                    warn!("control flow didn't send anything!");
                    let _ = restart.send(Some(Error::Timeout)).await;
                    return;
                }
            };

            if let Err(e) = ws_sink.send(Message::binary(packet.serialize())).await {
                warn!("failed to write message to tunnel websocket: {:?}", e);
                let _ = restart.send(Some(Error::WebSocketError(e))).await;
                return;
            }
        }
    });

    // continuously read from websocket tunnel

    loop {
        match ws_stream.next().await {
            Some(Ok(message)) if message.is_close() => {
                debug!("got close message");
                let _ = restart_tx.send(None).await;
                return Ok(());
            }
            Some(Ok(message)) => {
                let packet = process_control_flow_message(
                    config.clone(),
                    tunnel_tx.clone(),
                    message.into_data(),
                )
                .await
                .map_err(|e| {
                    error!("Malformed protocol control packet: {:?}", e);
                    Error::MalformedMessageFromServer
                })?;
                debug!("Processed packet: {:?}", packet.packet_type());
            }
            Some(Err(e)) => {
                warn!("websocket read error: {:?}", e);
                return Err(Error::Timeout);
            }
            None => {
                warn!("websocket sent none");
                return Err(Error::Timeout);
            }
        }
    }
}

struct Wormhole {
    websocket: WebSocketStream<MaybeTlsStream<TcpStream>>,
    sub_domain: String,
    hostname: String,
}

async fn connect_to_wormhole(config: &Config) -> Result<Wormhole, Error> {
    let (mut websocket, _) = tokio_tungstenite::connect_async(&config.control_url).await?;

    // send our Client Hello message
    let client_hello = match config.secret_key.clone() {
        Some(secret_key) => ClientHello::generate(
            config.sub_domain.clone(),
            ClientType::Auth { key: secret_key },
        ),
        None => {
            // if we have a reconnect token, use it.
            if let Some(reconnect) = RECONNECT_TOKEN.lock().await.clone() {
                ClientHello::reconnect(reconnect)
            } else {
                ClientHello::generate(config.sub_domain.clone(), ClientType::Anonymous)
            }
        }
    };

    info!("connecting to wormhole...");

    let hello = serde_json::to_vec(&client_hello).unwrap();
    websocket
        .send(Message::binary(hello))
        .await
        .expect("Failed to send client hello to wormhole server.");

    // wait for Server hello
    let server_hello_data = websocket
        .next()
        .await
        .ok_or(Error::NoResponseFromServer)??
        .into_data();
    let server_hello = serde_json::from_slice::<ServerHello>(&server_hello_data).map_err(|e| {
        error!("Couldn't parse server_hello from {:?}", e);
        Error::ServerReplyInvalid
    })?;

    let (sub_domain, hostname) = match server_hello {
        ServerHello::Success {
            sub_domain,
            client_id,
            hostname,
        } => {
            info!("Server accepted our connection. I am client_{}", client_id);
            (sub_domain, hostname)
        }
        ServerHello::AuthFailed => {
            return Err(Error::AuthenticationFailed);
        }
        ServerHello::InvalidSubDomain => {
            return Err(Error::InvalidSubDomain);
        }
        ServerHello::SubDomainInUse => {
            return Err(Error::SubDomainInUse);
        }
        ServerHello::Error(error) => return Err(Error::ServerError(error)),
    };

    Ok(Wormhole {
        websocket,
        sub_domain,
        hostname,
    })
}

async fn process_control_flow_message(
    config: Config,
    mut tunnel_tx: UnboundedSender<ControlPacket>,
    payload: Vec<u8>,
) -> Result<ControlPacket, Box<dyn std::error::Error>> {
    let control_packet = ControlPacket::deserialize(&payload)?;

    match &control_packet {
        ControlPacket::Init(stream_id) => {
            info!("stream[{:?}] -> init", stream_id.to_string());
        }
        ControlPacket::Ping(reconnect_token) => {
            log::info!("got ping. reconnect_token={}", reconnect_token.is_some());

            if let Some(reconnect) = reconnect_token {
                let _ = RECONNECT_TOKEN.lock().await.replace(reconnect.clone());
            }
            let _ = tunnel_tx.send(ControlPacket::Ping(None)).await;
        }
        ControlPacket::Refused(_) => return Err("unexpected control packet".into()),
        ControlPacket::End(stream_id) => {
            // find the stream
            let stream_id = stream_id.clone();

            info!("got end stream [{:?}]", &stream_id);

            tokio::spawn(async move {
                let stream = ACTIVE_STREAMS.read().unwrap().get(&stream_id).cloned();
                if let Some(mut tx) = stream {
                    tokio::time::sleep(Duration::from_secs(5)).await;
                    let _ = tx.send(StreamMessage::Close).await.map_err(|e| {
                        error!("failed to send stream close: {:?}", e);
                    });
                    ACTIVE_STREAMS.write().unwrap().remove(&stream_id);
                }
            });
        }
        ControlPacket::Data(stream_id, data) => {
            info!(
                "stream[{:?}] -> new data: {:?}",
                stream_id.to_string(),
                data.len()
            );

            if !ACTIVE_STREAMS.read().unwrap().contains_key(&stream_id) {
                if local::setup_new_stream(config.clone(), tunnel_tx.clone(), stream_id.clone())
                    .await
                    .is_none()
                {
                    error!("failed to open local tunnel")
                }
            }

            // find the right stream
            let active_stream = ACTIVE_STREAMS.read().unwrap().get(&stream_id).cloned();

            // forward data to it
            if let Some(mut tx) = active_stream {
                tx.send(StreamMessage::Data(data.clone())).await?;
                info!("forwarded to local tcp ({})", stream_id.to_string());
            } else {
                error!("got data but no stream to send it to.");
                let _ = tunnel_tx
                    .send(ControlPacket::Refused(stream_id.clone()))
                    .await?;
            }
        }
    };

    Ok(control_packet.clone())
}
