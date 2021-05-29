use async_trait::async_trait;
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::convert::TryInto;
use std::fmt::Formatter;

#[cfg(feature = "dynamodb")]
pub mod dynamo_auth_db;
#[cfg(feature = "sqlite")]
pub mod sqlite_auth_db;
pub mod client_auth;
pub mod reconnect_token;

#[derive(Clone)]
pub struct SigKey([u8; 32]);

impl std::fmt::Debug for SigKey {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str("<hidden sig key>")
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(transparent)]
pub struct Signature(String);

impl SigKey {
    pub fn generate() -> Self {
        SigKey(rand::thread_rng().gen::<[u8; 32]>())
    }

    pub fn from_hex(hex: &str) -> Result<Self, ()> {
        let bytes = hex::decode(hex)
            .map_err(|_| ())?
            .try_into()
            .map_err(|_| ())?;
        Ok(SigKey(bytes))
    }

    pub fn sign(&self, data: &[u8]) -> Signature {
        let sig = hmac_sha256::HMAC::mac(data, &self.0).to_vec();
        Signature(hex::encode(sig))
    }

    pub fn verify(&self, data: &[u8], signature: &Signature) -> bool {
        let signature = match hex::decode(signature.0.as_str()) {
            Ok(s) => s,
            Err(_) => return false,
        };
        let expected = hmac_sha256::HMAC::mac(data, &self.0).to_vec();
        signature == expected
    }
}

/// Define the required behavior of an Authentication Service
#[async_trait]
pub trait AuthService {
    type Error;
    type AuthKey;

    /// Authenticate a subdomain with an AuthKey
    async fn auth_sub_domain(
        &self,
        auth_key: &Self::AuthKey,
        subdomain: &str,
    ) -> Result<AuthResult, Self::Error>;
}

/// A result for authenticating a subdomain
pub enum AuthResult {
    ReservedByYou,
    ReservedByOther,
    ReservedByYouButDelinquent,
    PaymentRequired,
    Available,
}

#[derive(Debug, Clone, Copy)]
pub struct NoAuth;

#[async_trait]
impl AuthService for NoAuth {
    type Error = ();
    type AuthKey = String;

    /// Authenticate a subdomain with an AuthKey
    async fn auth_sub_domain(
        &self,
        _auth_key: &Self::AuthKey,
        _subdomain: &str,
    ) -> Result<AuthResult, Self::Error> {
        Ok(AuthResult::Available)
    }
}
