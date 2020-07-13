use super::*;

#[derive(Debug, Clone)]
pub struct ConnectedClient {
    pub id: ClientId,
    pub host: String,
    pub tx: UnboundedSender<ControlPacket>,
}

pub struct Connections {
    clients: Arc<RwLock<HashMap<ClientId, ConnectedClient>>>,
    hosts: Arc<RwLock<HashMap<String, ConnectedClient>>>
}

impl Connections {
    pub fn new() -> Self {
        Self {
            clients: Arc::new(RwLock::new(HashMap::new())),
            hosts: Arc::new(RwLock::new(HashMap::new()))
        }
    }

    pub fn remove(client: &ConnectedClient) {
        client.tx.close_channel();
        let mut connected = CONNECTIONS.clients.write().unwrap();
        let mut hosts = CONNECTIONS.hosts.write().unwrap();

        // ensure another client isn't using this host
        match hosts.get(&client.host) {
            Some(client_for_host) if client_for_host.id == client.id => {
                log::debug!("dropping sub-domain: {}", &client.host);
                hosts.remove(&client.host);
            },
            _ => {}
        };

        connected.remove(&client.id);
        log::debug!("rm client: {}", &client.id);

        // drop all the streams
        // if there are no more tunnel clients
        if connected.is_empty() {
            let mut streams = ACTIVE_STREAMS.write().unwrap();
            for (_, stream) in streams.drain() {
                stream.tx.close_channel();
            }
        }
    }

    pub fn client_for_host(host: &String) -> Option<ClientId> {
        CONNECTIONS.hosts.read().unwrap().get(host).map(|c| c.id.clone())
    }

    pub fn get(client_id: &ClientId) -> Option<ConnectedClient> {
        CONNECTIONS.clients.read().unwrap().get(&client_id).cloned()
    }

    pub fn find_by_host(host: &String) -> Option<ConnectedClient> {
        CONNECTIONS.hosts.read().unwrap().get(host).cloned()
    }

    pub fn add(client: ConnectedClient) {
        CONNECTIONS.clients.write().unwrap().insert(client.id.clone(), client.clone());
        CONNECTIONS.hosts.write().unwrap().insert(client.host.clone(), client);
    }
}
