use futures::{StreamExt, SinkExt};
use warp::{Filter};
use warp::ws::{Ws, Message, WebSocket};

pub use tunnelto::*;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use tokio::net::{TcpListener};

use futures::stream::{SplitSink, SplitStream};
use futures::channel::mpsc::{unbounded, UnboundedSender, UnboundedReceiver};
use lazy_static::lazy_static;
use log::{info, error};

mod connected_clients;
use self::connected_clients::*;
mod active_stream;
use self::active_stream::*;

mod client_auth;
mod auth_db;
pub use self::auth_db::AuthDbService;

mod remote;
mod control_server;

lazy_static! {
    pub static ref CONNECTIONS:Connections = Connections::new();
    pub static ref ACTIVE_STREAMS:ActiveStreams = Arc::new(RwLock::new(HashMap::new()));
    pub static ref ALLOWED_HOSTS:Vec<String> = allowed_host_suffixes();
    pub static ref BLOCKED_SUB_DOMAINS:Vec<String> = blocked_sub_domains_suffixes();
    pub static ref AUTH_DB_SERVICE:AuthDbService = AuthDbService::new().expect("failed to init auth-service");
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

#[tokio::main]
async fn main() {
    pretty_env_logger::init();

    info!("starting wormhole server");
    control_server::spawn(([0,0,0,0], 5000));

    let listen_addr = format!("0.0.0.0:{}", std::env::var("PORT").unwrap_or("8080".to_string()));
    info!("listening on: {}", &listen_addr);

    // create our accept any server
    let mut listener = TcpListener::bind(listen_addr).await.expect("failed to bind");

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