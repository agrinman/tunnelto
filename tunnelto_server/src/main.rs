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

mod auth_db;
mod client_auth;
pub use self::auth_db::AuthDbService;

mod control_server;
mod remote;

mod network;

lazy_static! {
    pub static ref CONNECTIONS: Connections = Connections::new();
    pub static ref ACTIVE_STREAMS: ActiveStreams = Arc::new(DashMap::new());
    pub static ref ALLOWED_HOSTS: Vec<String> = allowed_host_suffixes();
    pub static ref BLOCKED_SUB_DOMAINS: Vec<String> = blocked_sub_domains_suffixes();
    pub static ref AUTH_DB_SERVICE: AuthDbService =
        AuthDbService::new().expect("failed to init auth-service");
    pub static ref REMOTE_PORT: u16 = remote_port();
    pub static ref CTRL_PORT: u16 = ctrl_port();
    pub static ref NET_PORT: u16 = network_port();
}

/// What hosts do we allow tunnels on:
/// i.e:    baz.com => *.baz.com
///         foo.bar => *.foo.bar
pub fn allowed_host_suffixes() -> Vec<String> {
    std::env::var("ALLOWED_HOSTS")
        .map(|s| s.split(",").map(String::from).collect())
        .unwrap_or(vec![])
}

/// What sub-domains do we always block:
/// i.e:    dashboard.tunnelto.dev
pub fn blocked_sub_domains_suffixes() -> Vec<String> {
    std::env::var("BLOCKED_SUB_DOMAINS")
        .map(|s| s.split(",").map(String::from).collect())
        .unwrap_or(vec![])
}

pub fn ctrl_port() -> u16 {
    let ctrl_port = std::env::var("CTRL_PORT").unwrap_or("".to_string());
    if ctrl_port.is_empty() {
        5000
    } else {
        ctrl_port.parse().unwrap_or_else(|_| {
            panic!("Invalid CTRL_PORT: {}", ctrl_port);
        })
    }
}

pub fn remote_port() -> u16 {
    let port = std::env::var("PORT").unwrap_or("".to_string());
    if port.is_empty() {
        8080
    } else {
        port.parse().unwrap_or_else(|_| {
            panic!("Invalid PORT: {}", port);
        })
    }
}

pub fn network_port() -> u16 {
    let port = std::env::var("NET_PORT").unwrap_or("".to_string());
    if port.is_empty() {
        6000
    } else {
        port.parse().unwrap_or_else(|_| {
            panic!("Invalid NET_PORT: {}", port);
        })
    }
}

#[tokio::main]
async fn main() {
    pretty_env_logger::init();

    control_server::spawn(([0, 0, 0, 0], *CTRL_PORT));
    info!("started tunnelto server on 0.0.0.0:{}", *CTRL_PORT);

    network::spawn(([0, 0, 0, 0, 0, 0, 0, 0], *NET_PORT));
    info!("start network service on [::]:{}", *NET_PORT);

    let listen_addr = format!("[::]:{}", *REMOTE_PORT);
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
