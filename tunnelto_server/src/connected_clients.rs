use super::*;
use dashmap::DashMap;
use std::fmt::Formatter;

#[derive(Clone)]
pub struct ConnectedClient {
    pub id: ClientId,
    pub host: String,
    pub is_anonymous: bool,
    pub tx: UnboundedSender<ControlPacket>,
}

impl std::fmt::Debug for ConnectedClient {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConnectedClient")
            .field("id", &self.id)
            .field("sub", &self.host)
            .field("anon", &self.is_anonymous)
            .finish()
    }
}

pub struct Connections {
    clients: Arc<DashMap<ClientId, ConnectedClient>>,
    hosts: Arc<DashMap<String, ConnectedClient>>,
}

impl Connections {
    pub fn new() -> Self {
        Self {
            clients: Arc::new(DashMap::new()),
            hosts: Arc::new(DashMap::new()),
        }
    }

    pub fn update_host(client: &ConnectedClient) {
        CONNECTIONS
            .hosts
            .insert(client.host.clone(), client.clone());
    }

    pub fn remove(client: &ConnectedClient) {
        client.tx.close_channel();

        // ensure another client isn't using this host
        if CONNECTIONS
            .hosts
            .get(&client.host)
            .map_or(false, |c| c.id == client.id)
        {
            tracing::debug!("dropping sub-domain: {}", &client.host);
            CONNECTIONS.hosts.remove(&client.host);
        };

        CONNECTIONS.clients.remove(&client.id);
        tracing::debug!("rm client: {}", &client.id);

        // // drop all the streams
        // // if there are no more tunnel clients
        // if CONNECTIONS.clients.is_empty() {
        //     let mut streams = ACTIVE_STREAMS.;
        //     for (_, stream) in streams.drain() {
        //         stream.tx.close_channel();
        //     }
        // }
    }

    pub fn client_for_host(host: &String) -> Option<ClientId> {
        CONNECTIONS.hosts.get(host).map(|c| c.id.clone())
    }

    pub fn get(client_id: &ClientId) -> Option<ConnectedClient> {
        CONNECTIONS
            .clients
            .get(&client_id)
            .map(|c| c.value().clone())
    }

    pub fn find_by_host(host: &String) -> Option<ConnectedClient> {
        CONNECTIONS.hosts.get(host).map(|c| c.value().clone())
    }

    pub fn add(client: ConnectedClient) {
        CONNECTIONS
            .clients
            .insert(client.id.clone(), client.clone());
        CONNECTIONS.hosts.insert(client.host.clone(), client);
    }
}
