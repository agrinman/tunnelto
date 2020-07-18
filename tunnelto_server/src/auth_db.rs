use rusoto_dynamodb::{DynamoDbClient, DynamoDb, AttributeValue, GetItemInput, GetItemError};
use rusoto_core::{HttpClient, Client, Region};

use std::collections::HashMap;
use uuid::Uuid;
use thiserror::Error;
use sha2::Digest;
use rusoto_credential::EnvironmentProvider;
use std::str::FromStr;
use log::{info, error};
use rusqlite::{params, Connection};
use std::io::ErrorKind;

enum Engine {
    DYNAMODB,
    SQLITE,
}

#[derive(Debug)]
struct DbAuth {
    auth_key_hash: String,
    account_id: String,
}

#[derive(Debug)]
struct DbDomains {
    subdomain: String,
    account_id: String,
}

pub struct AuthDbService {
    engine: Engine,
    client: Option<DynamoDbClient>,
}

impl AuthDbService {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let provider = EnvironmentProvider::default();
        let http_client = HttpClient::new()?;
        let client = Client::new_with(provider, http_client);
        let engine = std::env::var("STORE").unwrap_or("dynamodb".to_string());
        info!("Engine = {}", engine);
        if engine == "sqlite" {
            let conn = Connection::open("./tunnelto.db")?;
            conn.execute(
                "CREATE TABLE IF NOT EXISTS tunnelto_domains  (
                        subdomain       TEXT NOT NULL,
                        account_id      TEXT NOT NULL
                        )",
                params![],
            )?;
            conn.execute(
                "CREATE TABLE IF NOT EXISTS tunnelto_auth  (
                        auth_key_hash   TEXT NOT NULL,
                        account_id      TEXT NOT NULL
                        )",
                params![],
            )?;
            conn.close().expect("Error handling the sqlite database");
        }

        match engine.as_ref() {
            "dynamodb" => Ok( Self { engine: Engine::DYNAMODB, client: Some(DynamoDbClient::new_with_client(client, Region::UsEast1)) } ),
            "sqlite" => Ok( Self { engine: Engine::SQLITE, client: None } ),
            _ => Err(Box::new(std::io::Error::new(ErrorKind::Other, "Unknown storage engine specified in environment")))
        }
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

        let account_str = match (&self.engine, &self.client) {
            (Engine::DYNAMODB, Some(client)) => {
                let mut input = GetItemInput { table_name: key_db::TABLE_NAME.to_string(), ..Default::default() };
                input.key = {
                    let mut item = HashMap::new();
                    item.insert(key_db::PRIMARY_KEY.to_string(), AttributeValue {
                        s: Some(auth_key_hash),
                        ..Default::default()
                    });
                    item
                };

                let result = client.get_item(input).await?;
                result.item
                    .unwrap_or(HashMap::new())
                    .get(key_db::ACCOUNT_ID)
                    .cloned()
                    .unwrap_or(AttributeValue::default())
                    .s
                    .ok_or(Error::AccountNotFound)?
            },
            (Engine::SQLITE, _) => {
                let conn = Connection::open("./tunnelto.db").expect("Unable to open database for authentication purpose");
                let row: Result<String, _> = conn.query_row(
                    "SELECT account_id FROM tunnelto_auth WHERE auth_key_hash=?",
                    params![auth_key_hash,],
                    |row| row.get(0)
                );
                match row {
                    Ok(value) => value,
                    Err(_) => "default".to_string()
                }
            },
            (Engine::DYNAMODB, None) => {
                "default".to_string()
            }
        };


        let uuid = Uuid::from_str(&account_str)?;
        Ok(uuid)
    }

    async fn get_account_id_for_subdomain(&self, subdomain: &str) -> Result<Option<Uuid>, Error> {
        let account_str = match (&self.engine, &self.client) {
            (Engine::DYNAMODB, Some(_)) => {
                let mut input = GetItemInput { table_name: domain_db::TABLE_NAME.to_string(), ..Default::default() };
                input.key = {
                    let mut item = HashMap::new();
                    item.insert(domain_db::PRIMARY_KEY.to_string(), AttributeValue {
                        s: Some(subdomain.to_string()),
                        ..Default::default()
                    });
                    item
                };

                let result = self.client.as_ref().unwrap().get_item(input).await?;
                result.item
                    .unwrap_or(HashMap::new())
                    .get(domain_db::ACCOUNT_ID)
                    .cloned()
                    .unwrap_or(AttributeValue::default())
                    .s
            },
            (Engine::SQLITE, _) => {
                let conn = Connection::open("./tunnelto.db").expect("Unable to open database for ownership purpose");
                let row: Result<String, _> = conn.query_row(
                    "SELECT account_id FROM tunnelto_domains WHERE subdomain=?",
                    params![subdomain,],
                    |row| row.get(0)
                );
                match row {
                    Ok(value) => Some(value),
                    Err(_) => None
                }
            },
            (Engine::DYNAMODB, None) => {
                None
            }
        };

        if let Some(account_str) = account_str {
            let uuid = Uuid::from_str(&account_str)?;
            Ok(Some(uuid))
        } else {
            Ok(None)
        }
    }
}
