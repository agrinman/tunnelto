#[derive(Debug, Clone)]
pub struct ActiveStream {
    pub id: StreamId,
    pub client: ConnectedClient,
    pub tx: UnboundedSender<StreamMessage>,
}

impl ActiveStream {
    pub fn new(client: ConnectedClient) -> (Self, UnboundedReceiver<StreamMessage>) {
        let (tx, rx) = unbounded();
        (
            ActiveStream {
                id: StreamId::generate(),
                client,
                tx,
            },
            rx,
        )
    }
}

pub type ActiveStreams = Arc<DashMap<StreamId, ActiveStream>>;

use super::*;
#[derive(Debug, Clone)]
pub enum StreamMessage {
    Data(Vec<u8>),
    TunnelRefused,
    NoClientTunnel,
}
