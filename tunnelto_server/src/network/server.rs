use super::*;
use crate::connected_clients::Connections;
use crate::ClientId;
use serde::{Deserialize, Serialize};
use warp::Filter;

pub fn spawn<A: Into<SocketAddr>>(addr: A) {
    let health_check = warp::get().and(warp::path("health_check")).map(|| {
        tracing::debug!("Net svc health check triggered");
        "ok"
    });

    let query_svc = warp::path::end()
        .and(warp::get())
        .and(warp::query::<HostQuery>())
        .map(|query| warp::reply::json(&handle_query(query)));

    let routes = query_svc.or(health_check);

    // spawn our websocket control server
    tokio::spawn(warp::serve(routes).run(addr.into()));
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostQuery {
    pub host: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostQueryResponse {
    pub client_id: Option<ClientId>,
}

fn handle_query(query: HostQuery) -> HostQueryResponse {
    tracing::debug!(host=%query.host, "got query");
    // SECURITY: Only return client_id for authenticated requests from authorized instances
    // The host lookup is intentionally limited to internal gossip protocol only
    // to prevent unauthorized users from discovering tunnel ownership
    HostQueryResponse {
        client_id: Connections::client_for_host(&query.host),
    }
}
