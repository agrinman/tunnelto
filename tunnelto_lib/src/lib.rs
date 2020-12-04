use rand::prelude::*;
use serde::{Serialize, Deserialize};
use sha2::Digest;

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(transparent)]
pub struct SecretKey(pub String);
impl SecretKey {
    pub fn generate() -> Self {
        let mut rng = rand::thread_rng();
        Self(std::iter::repeat(())
            .map(|_| rng.sample(rand::distributions::Alphanumeric))
            .take(22)
            .collect::<String>())
    }

    pub fn client_id(&self) -> ClientId {
        ClientId(base64::encode(&sha2::Sha256::digest(self.0.as_bytes()).to_vec()))
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all="snake_case")]
pub enum ServerHello {
    Success {
        sub_domain: String,
    },
    SubDomainInUse,
    InvalidSubDomain,
    AuthFailed,
}

impl ServerHello {
    #[allow(unused)]
    pub fn random_domain() -> String {
        let mut rng = rand::thread_rng();
        std::iter::repeat(())
            .map(|_| rng.sample(rand::distributions::Alphanumeric))
            .take(8)
            .collect::<String>()
            .to_lowercase()
    }

    #[allow(unused)]
    pub fn prefixed_random_domain(prefix: &str) -> String {
        format!("{}-{}", prefix, Self::random_domain())
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ClientHello {
    pub id: ClientId,
    pub sub_domain: Option<String>,
    pub client_type: ClientType
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ClientHelloV1 {
    pub id: ClientId,
    pub sub_domain: Option<String>,
    pub is_anonymous: bool,
    unix_seconds: i64,
    signature: String,
}

impl ClientHello {
    pub fn generate(sub_domain: Option<String>, typ: ClientType) -> Self {
        ClientHello { id: ClientId::generate(), client_type: typ, sub_domain}
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum ClientType {
    Auth { key: SecretKey },
    Anonymous,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Hash)]
#[serde(transparent)]
pub struct ClientId(String);

impl std::fmt::Display for ClientId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}
impl ClientId {
    pub fn generate() -> Self {
        let mut id = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut id);
        ClientId(base64::encode_config(&id, base64::URL_SAFE_NO_PAD))
    }
}


#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StreamId([u8; 8]);

impl StreamId {
    #[allow(unused)]
    pub fn generate() -> StreamId {
        let mut id = [0u8; 8];
        rand::thread_rng().fill_bytes(&mut id);
        StreamId(id)
    }

    pub fn to_string(&self) -> String {
        format!("stream_{}", base64::encode_config(&self.0, base64::URL_SAFE_NO_PAD))
    }
}

#[derive(Debug, Clone)]
pub enum ControlPacket {
    Init(StreamId),
    Data(StreamId, Vec<u8>),
    Refused(StreamId),
    End(StreamId),
    Ping,
}

pub const PING_INTERVAL:u64 = 5;

const EMPTY_STREAM:StreamId = StreamId([0xF, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]);

impl ControlPacket {
    pub fn serialize(self) -> Vec<u8> {
        match self {
            ControlPacket::Init(sid) => [vec![0x01], sid.0.to_vec()].concat(),
            ControlPacket::Data(sid, data) => [vec![0x02], sid.0.to_vec(), data].concat(),
            ControlPacket::Refused(sid) => [vec![0x03], sid.0.to_vec()].concat(),
            ControlPacket::End(sid) =>  [vec![0x04], sid.0.to_vec()].concat(),
            ControlPacket::Ping => [vec![0x05], EMPTY_STREAM.0.to_vec()].concat(),
        }
    }

    pub fn packet_type(&self) -> &str {
        match &self {
            ControlPacket::Ping => "PING",
            ControlPacket::Init(_) => "INIT STREAM",
            ControlPacket::Data(_, _) => "STREAM DATA",
            ControlPacket::Refused(_) => "REFUSED",
            ControlPacket::End(_) => "END STREAM",
        }
    }

    pub fn deserialize(data: &[u8]) -> Result<Self, Box<dyn std::error::Error>> {
        if data.len() < 9 {
            return Err("invalid DataPacket, missing stream id".into())
        }

        let mut stream_id = [0u8; 8];
        stream_id.clone_from_slice(&data[1..9]);
        let stream_id = StreamId(stream_id);

        let packet = match data[0] {
            0x01 => ControlPacket::Init(stream_id),
            0x02 => ControlPacket::Data(stream_id, data[9..].to_vec()),
            0x03 => ControlPacket::Refused(stream_id),
            0x04 => ControlPacket::End(stream_id),
            0x05 => ControlPacket::Ping,
            _ => return Err("invalid control byte in DataPacket".into())
        };

        Ok(packet)
    }
}