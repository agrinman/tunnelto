use rand::prelude::*;
use serde::{Serialize, Deserialize};
use serde::export::Formatter;
use chrono::Utc;

#[derive(Debug, Clone)]
pub struct SecretKey(pub String);
impl SecretKey {
    #[allow(unused)]
    pub fn generate() -> Self {
        let mut key = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut key);
        Self(base64::encode_config(&key, base64::URL_SAFE_NO_PAD))
    }

    #[allow(unused)]
    pub fn anonymous_key() -> Self {
        let mut key = [0u8; 32];
        Self(base64::encode_config(&key, base64::URL_SAFE_NO_PAD))
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

const CLIENT_HELLO_TTL_SECONDS:i64 = 300;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ClientHello {
    pub id: ClientId,
    pub sub_domain: Option<String>,
    pub is_anonymous: bool,
    // epoch
    unix_seconds: i64,
    //hex encoded
    signature: String,
}

impl ClientHello {
    pub fn generate(id: ClientId, secret_key: &Option<SecretKey>, sub_domain: Option<String>) -> (Self, ClientId) {
        let unix_seconds = Utc::now().timestamp();
        let input = format!("{}", unix_seconds);
        let signature = match secret_key {
            Some(key) => hmac_sha256::HMAC::mac(input.as_bytes(), key.0.as_bytes()),
            None => hmac_sha256::HMAC::mac(input.as_bytes(), SecretKey::anonymous_key().0.as_bytes()),
        };

        (ClientHello {
            id: id.clone(), sub_domain, unix_seconds, signature: hex::encode(signature), is_anonymous: secret_key.is_none()
        }, id)
    }

    #[allow(unused)]
    pub fn verify(secret_key: &SecretKey, data: &[u8], allow_unknown: bool) -> Result<Self, Box<dyn std::error::Error>> {
        let client_hello:ClientHello = serde_json::from_slice(&data)?;

        // check the time
        if (Utc::now().timestamp() - client_hello.unix_seconds).abs() > CLIENT_HELLO_TTL_SECONDS {
            return Err("Expired client hello".into())
        }

        // check that anonymous is allowed
        if !allow_unknown && client_hello.is_anonymous {
            return Err("Anonymous clients are not allowed".into())
        }

        let input = format!("{}", client_hello.unix_seconds);

        let expected = if client_hello.is_anonymous {
            hmac_sha256::HMAC::mac(input.as_bytes(), SecretKey::anonymous_key().0.as_bytes())
        } else {
            hmac_sha256::HMAC::mac(input.as_bytes(), secret_key.0.as_bytes())
        };

        if hex::encode(expected) != client_hello.signature {
            return Err("Bad signature in client hello".into())
        }

        Ok(client_hello)
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Hash)]
#[serde(transparent)]
pub struct ClientId(String);

impl std::fmt::Display for ClientId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
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

pub const PING_INTERVAL:u64 = 4;

const EMPTY_STREAM:StreamId = StreamId([0xF, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]);

impl ControlPacket {
    pub fn serialize(self) -> Vec<u8> {
        match self {
            ControlPacket::Init(sid) => [vec![0x01], sid.0.to_vec()].concat(),
            ControlPacket::Data(sid, data) => [vec![0x02], sid.0.to_vec(), data].concat(),
            ControlPacket::Refused(sid) => [vec![0x03], sid.0.to_vec()].concat(),
            ControlPacket::End(sid) =>  [vec![0x04], sid.0.to_vec()].concat(),
            ControlPacket::Ping => [vec![0x05], EMPTY_STREAM.0.to_vec()].concat()
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