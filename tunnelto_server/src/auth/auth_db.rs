use rusoto_dynamodb::{DynamoDbClient, DynamoDb, AttributeValue, GetItemInput, GetItemError};
use rusoto_core::{HttpClient, Client, Region};

use std::collections::HashMap;
use uuid::Uuid;
use thiserror::Error;
use sha2::Digest;
use rusoto_credential::EnvironmentProvider;
use std::str::FromStr;

pub struct AuthDbService {
    client: DynamoDbClient,
}

impl AuthDbService {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let provider = EnvironmentProvider::default();
        let http_client = HttpClient::new()?;
        let client = Client::new_with(provider, http_client);

        Ok(Self { client: DynamoDbClient::new_with_client(client, Region::UsEast1) })
    }
}

mod domain_db {
    pub const TABLE_NAME:&'static str = "tunnelto_domains";
    pub const PRIMARY_KEY:&'static str = "subdomain";
    pub const ACCOUNT_ID:&'static str = "account_id";
}

mod key_db {
    pub const TABLE_NAME:&'static str = "tunnelto_auth";
    pub const PRIMARY_KEY:&'static str = "auth_key_hash";
    pub const ACCOUNT_ID:&'static str = "account_id";
}

fn key_id(auth_key: &str) -> String {
    let hash = sha2::Sha256::digest(auth_key.as_bytes()).to_vec();
    base64::encode_config(&hash, base64::URL_SAFE_NO_PAD)
}

#[derive(Error, Debug)]
pub enum Error {
    #[error("failed to get domain item")]
    AuthDbGetItem(#[from] rusoto_core::RusotoError<GetItemError>),

    #[error("The authentication key is invalid")]
    AccountNotFound,

    #[error("The authentication key is invalid")]
    InvalidAccountId(#[from] uuid::Error),

    #[error("The subdomain is not authorized")]
    SubdomainNotAuthorized,
}

pub enum AuthResult {
    ReservedByYou,
    ReservedByOther,
    Available,
}
impl AuthDbService {
    pub async fn auth_sub_domain(&self, auth_key: &str, subdomain: &str) -> Result<AuthResult, Error> {
        let authenticated_account_id = self.get_account_id_for_auth_key(auth_key).await?;
        match self.get_account_id_for_subdomain(subdomain).await? {
            Some(account_id) => {
                if authenticated_account_id == account_id {
                    return Ok(AuthResult::ReservedByYou)
                }

                Ok(AuthResult::ReservedByOther)
            },
            None => Ok(AuthResult::Available)
        }
    }

    async fn get_account_id_for_auth_key(&self, auth_key: &str) -> Result<Uuid, Error> {
        let auth_key_hash = key_id(auth_key);

        let mut input = GetItemInput { table_name: key_db::TABLE_NAME.to_string(), ..Default::default() };
        input.key = {
            let mut item = HashMap::new();
            item.insert(key_db::PRIMARY_KEY.to_string(), AttributeValue {
                s: Some(auth_key_hash),
                ..Default::default()
            });
            item
        };

        let result = self.client.get_item(input).await?;
        let account_str = result.item
            .unwrap_or(HashMap::new())
            .get(key_db::ACCOUNT_ID)
            .cloned()
            .unwrap_or(AttributeValue::default())
            .s
            .ok_or(Error::AccountNotFound)?;

        let uuid = Uuid::from_str(&account_str)?;
        Ok(uuid)
    }

    async fn get_account_id_for_subdomain(&self, subdomain: &str) -> Result<Option<Uuid>, Error> {
        let mut input = GetItemInput { table_name: domain_db::TABLE_NAME.to_string(), ..Default::default() };
        input.key = {
            let mut item = HashMap::new();
            item.insert(domain_db::PRIMARY_KEY.to_string(), AttributeValue {
                s: Some(subdomain.to_string()),
                ..Default::default()
            });
            item
        };

        let result = self.client.get_item(input).await?;
        let account_str = result.item
            .unwrap_or(HashMap::new())
            .get(domain_db::ACCOUNT_ID)
            .cloned()
            .unwrap_or(AttributeValue::default())
            .s;

        if let Some(account_str) = account_str {
            let uuid = Uuid::from_str(&account_str)?;
            Ok(Some(uuid))
        } else {
            Ok(None)
        }
    }
}