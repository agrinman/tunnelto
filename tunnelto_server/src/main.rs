use futures::{SinkExt, StreamExt};
use warp::ws::{Message, WebSocket, Ws};
use warp::Filter;

use dashmap::DashMap;
use std::sync::Arc;
pub use tunnelto_lib::*;

use tokio::net::TcpListener;

use futures::channel::mpsc::{unbounded, UnboundedReceiver, UnboundedSender};
use futures::stream::{SplitSink, SplitStream};
use lazy_static::lazy_static;
use log::{error, info};

mod connected_clients;
use self::connected_clients::*;
mod active_stream;
use self::active_stream::*;

mod auth;
pub use self::auth::auth_db;
pub use self::auth::client_auth;

pub use self::auth_db::AuthDbService;

mod control_server;
mod remote;

mod config;
pub use self::config::Config;
mod network;

lazy_static! {
    pub static ref CONNECTIONS: Connections = Connections::new();
    pub static ref ACTIVE_STREAMS: ActiveStreams = Arc::new(DashMap::new());
    pub static ref AUTH_DB_SERVICE: AuthDbService =
        AuthDbService::new().expect("failed to init auth-service");
    pub static ref CONFIG: Config = Config::from_env();
}

#[tokio::main]
async fn main() {
    pretty_env_logger::init();

    control_server::spawn(([0, 0, 0, 0], CONFIG.control_port));
    info!("started tunnelto server on 0.0.0.0:{}", CONFIG.control_port);

    network::spawn(([0, 0, 0, 0, 0, 0, 0, 0], CONFIG.internal_network_port));
    info!(
        "start network service on [::]:{}",
        CONFIG.internal_network_port
    );

    let listen_addr = format!("[::]:{}", CONFIG.remote_port);
    info!("listening on: {}", &listen_addr);

    // create our accept any server
    let listener = TcpListener::bind(listen_addr)
        .await
        .expect("failed to bind");

    loop {
        let socket = match listener.accept().await {
            Ok((socket, _)) => socket,
            _ => {
                error!("failed to accept socket");
                continue;
            }
        };

        tokio::spawn(async move {
            remote::accept_connection(socket).await;
        });
    }
}
