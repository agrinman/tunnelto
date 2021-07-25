use futures::future::select_ok;
use futures::FutureExt;
use std::net::{IpAddr, SocketAddr};
use thiserror::Error;
mod server;
pub use self::server::spawn;
mod proxy;
pub use self::proxy::proxy_stream;
use crate::network::server::{HostQuery, HostQueryResponse};
use crate::ClientId;
use reqwest::StatusCode;
use trust_dns_resolver::TokioAsyncResolver;

#[derive(Error, Debug)]
pub enum Error {
    #[error("IOError: {0}")]
    IoError(#[from] std::io::Error),

    #[error("RequestError: {0}")]
    Request(#[from] reqwest::Error),

    #[error("ResolverError: {0}")]
    Resolver(#[from] trust_dns_resolver::error::ResolveError),

    #[error("Does not serve host")]
    DoesNotServeHost,
}

/// An instance of our server
#[derive(Debug, Clone)]
pub struct Instance {
    pub ip: IpAddr,
}

impl Instance {
    /// get all instances where our app runs
    async fn get_instances() -> Result<Vec<Instance>, Error> {
        let query = if let Some(dns) = crate::CONFIG.gossip_dns_host.clone() {
            dns
        } else {
            tracing::warn!("warning! gossip mode disabled!");
            return Ok(vec![]);
        };

        tracing::debug!("querying app instances");

        let resolver = TokioAsyncResolver::tokio_from_system_conf()?;

        let ips = resolver.lookup_ip(query).await?;

        let instances = ips.iter().map(|ip| Instance { ip }).collect();
        tracing::debug!("Found app instances: {:?}", &instances);
        Ok(instances)
    }

    /// query the instance and see if it runs our host
    async fn serves_host(self, host: &str) -> Result<(Instance, ClientId), Error> {
        let addr = SocketAddr::new(self.ip.clone(), crate::CONFIG.internal_network_port);
        let url = format!("http://{}", addr.to_string());
        let client = reqwest::Client::new();
        let response = client
            .get(url)
            .timeout(std::time::Duration::from_secs(2))
            .query(&HostQuery {
                host: host.to_string(),
            })
            .send()
            .await
            .map_err(|e| {
                tracing::error!(error=?e, "failed to send a host query");
                e
            })?;
        let status = response.status();
        let result: HostQueryResponse = response.json().await?;

        let found_client = result
            .client_id
            .as_ref()
            .map(|c| c.to_string())
            .unwrap_or_default();
        tracing::debug!(status=%status, found=%found_client, "got net svc response");

        match (status, result.client_id) {
            (StatusCode::OK, Some(client_id)) => Ok((self, client_id)),
            _ => Err(Error::DoesNotServeHost),
        }
    }
}

/// get the ip address we need to connect to that runs our host
#[tracing::instrument]
pub async fn instance_for_host(host: &str) -> Result<(Instance, ClientId), Error> {
    let instances = Instance::get_instances()
        .await?
        .into_iter()
        .map(|i| i.serves_host(host).boxed());

    if instances.len() == 0 {
        return Err(Error::DoesNotServeHost);
    }

    let instance = select_ok(instances).await?.0;
    tracing::info!(instance_ip=%instance.0.ip, client_id=%instance.1.to_string(), subdomain=%host, "found instance for host");
    Ok(instance)
}
