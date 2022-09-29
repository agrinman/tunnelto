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

mod connected_clients;
use self::connected_clients::*;
mod active_stream;
use self::active_stream::*;

mod auth;
#[cfg(feature = "dynamodb")]
pub use self::auth::dynamo_auth_db;
#[cfg(feature = "sqlite")]
pub use self::auth::sqlite_auth_db;
pub use self::auth::client_auth;

#[cfg(feature = "dynamodb")]
pub use self::dynamo_auth_db::AuthDbService;
#[cfg(feature = "sqlite")]
pub use self::sqlite_auth_db::AuthDbService;

mod control_server;
mod remote;

mod config;
pub use self::config::Config;
mod network;

mod observability;

use tracing::level_filters::LevelFilter;
use tracing_honeycomb::libhoney;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::registry;

use tracing::{error, info, Instrument};

lazy_static! {
    pub static ref CONNECTIONS: Connections = Connections::new();
    pub static ref ACTIVE_STREAMS: ActiveStreams = Arc::new(DashMap::new());
    pub static ref CONFIG: Config = Config::from_env();
}
#[cfg(any(feature = "dynamodb", feature="sqlite"))]
lazy_static! {
    pub static ref AUTH_DB_SERVICE: AuthDbService =
        AuthDbService::new().expect("failed to init auth-service");
}
#[cfg(not(any(feature = "dynamodb", feature="sqlite")))]
lazy_static! {
    pub static ref AUTH_DB_SERVICE: crate::auth::NoAuth = crate::auth::NoAuth;
}

#[tokio::main]
async fn main() {
    // setup observability
    if let Some(api_key) = CONFIG.honeycomb_api_key.clone() {
        info!("configuring observability layer");

        let honeycomb_config = libhoney::Config {
            options: libhoney::client::Options {
                api_key,
                dataset: "t2-service".to_string(),
                ..libhoney::client::Options::default()
            },
            transmission_options: libhoney::transmission::Options {
                max_batch_size: 50,
                max_concurrent_batches: 10,
                batch_timeout: std::time::Duration::from_millis(1000),
                pending_work_capacity: 5_000,
                user_agent_addition: None,
            },
        };

        let telemetry_layer =
            tracing_honeycomb::new_honeycomb_telemetry_layer("t2-service", honeycomb_config);

        let subscriber = registry::Registry::default()
            .with(LevelFilter::INFO)
            .with(tracing_subscriber::fmt::Layer::default())
            .with(telemetry_layer);

        tracing::subscriber::set_global_default(subscriber).expect("setting global default failed");
    } else {
        let subscriber = registry::Registry::default()
            .with(LevelFilter::INFO)
            .with(tracing_subscriber::fmt::Layer::default());
        tracing::subscriber::set_global_default(subscriber).expect("setting global default failed");
    };

    tracing::info!("starting server!");

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

        tokio::spawn(
            async move {
                remote::accept_connection(socket).await;
            }
            .instrument(observability::remote_trace("remote_connect")),
        );
    }
}
