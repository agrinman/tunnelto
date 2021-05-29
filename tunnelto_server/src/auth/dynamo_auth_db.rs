use rusoto_core::{Client, HttpClient, Region};
use rusoto_dynamodb::{AttributeValue, DynamoDb, DynamoDbClient, GetItemError, GetItemInput};

use super::AuthResult;
use crate::auth::AuthService;
use async_trait::async_trait;
use rusoto_credential::EnvironmentProvider;
use sha2::Digest;
use std::collections::HashMap;
use std::str::FromStr;
use thiserror::Error;
use uuid::Uuid;

pub struct AuthDbService {
    client: DynamoDbClient,
}

impl AuthDbService {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let provider = EnvironmentProvider::default();
        let http_client = HttpClient::new()?;
        let client = Client::new_with(provider, http_client);

        Ok(Self {
            client: DynamoDbClient::new_with_client(client, Region::UsEast1),
        })
    }
}

mod domain_db {
    pub const TABLE_NAME: &'static str = "tunnelto_domains";
    pub const PRIMARY_KEY: &'static str = "subdomain";
    pub const ACCOUNT_ID: &'static str = "account_id";
}

mod key_db {
    pub const TABLE_NAME: &'static str = "tunnelto_auth";
    pub const PRIMARY_KEY: &'static str = "auth_key_hash";
    pub const ACCOUNT_ID: &'static str = "account_id";
}

mod record_db {
    pub const TABLE_NAME: &'static str = "tunnelto_record";
    pub const PRIMARY_KEY: &'static str = "account_id";
    pub const SUBSCRIPTION_ID: &'static str = "subscription_id";
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

#[async_trait]
impl AuthService for AuthDbService {
    type Error = Error;
    type AuthKey = String;

    async fn auth_sub_domain(
        &self,
        auth_key: &String,
        subdomain: &str,
    ) -> Result<AuthResult, Error> {
        let authenticated_account_id = self.get_account_id_for_auth_key(auth_key).await?;
        let is_pro_account = self
            .is_account_in_good_standing(authenticated_account_id)
            .await?;

        tracing::info!(account=%authenticated_account_id.to_string(), requested_subdomain=%subdomain, is_pro=%is_pro_account, "authenticated client");

        if let Some(account_id) = self.get_account_id_for_subdomain(subdomain).await? {
            // check you reserved it
            if authenticated_account_id != account_id {
                tracing::info!(account=%authenticated_account_id.to_string(), "reserved by other");
                return Ok(AuthResult::ReservedByOther);
            }

            // next ensure that the account is in good standing
            if !is_pro_account {
                tracing::warn!(account=%authenticated_account_id.to_string(), "delinquent");
                return Ok(AuthResult::ReservedByYouButDelinquent);
            }

            return Ok(AuthResult::ReservedByYou);
        }

        if is_pro_account {
            Ok(AuthResult::Available)
        } else {
            Ok(AuthResult::PaymentRequired)
        }
    }
}

impl AuthDbService {
    async fn get_account_id_for_auth_key(&self, auth_key: &str) -> Result<Uuid, Error> {
        let auth_key_hash = key_id(auth_key);

        let mut input = GetItemInput {
            table_name: key_db::TABLE_NAME.to_string(),
            ..Default::default()
        };
        input.key = {
            let mut item = HashMap::new();
            item.insert(
                key_db::PRIMARY_KEY.to_string(),
                AttributeValue {
                    s: Some(auth_key_hash),
                    ..Default::default()
                },
            );
            item
        };

        let result = self.client.get_item(input).await?;
        let account_str = result
            .item
            .unwrap_or(HashMap::new())
            .get(key_db::ACCOUNT_ID)
            .cloned()
            .unwrap_or(AttributeValue::default())
            .s
            .ok_or(Error::AccountNotFound)?;

        let uuid = Uuid::from_str(&account_str)?;

        Ok(uuid)
    }

    async fn is_account_in_good_standing(&self, account_id: Uuid) -> Result<bool, Error> {
        let mut input = GetItemInput {
            table_name: record_db::TABLE_NAME.to_string(),
            ..Default::default()
        };
        input.key = {
            let mut item = HashMap::new();
            item.insert(
                record_db::PRIMARY_KEY.to_string(),
                AttributeValue {
                    s: Some(account_id.to_string()),
                    ..Default::default()
                },
            );
            item
        };

        let result = self.client.get_item(input).await?;
        let result = result.item.unwrap_or(HashMap::new());

        let subscription_id = result
            .get(record_db::SUBSCRIPTION_ID)
            .cloned()
            .unwrap_or(AttributeValue::default())
            .s;

        Ok(subscription_id.is_some())
    }

    async fn get_account_id_for_subdomain(&self, subdomain: &str) -> Result<Option<Uuid>, Error> {
        let mut input = GetItemInput {
            table_name: domain_db::TABLE_NAME.to_string(),
            ..Default::default()
        };
        input.key = {
            let mut item = HashMap::new();
            item.insert(
                domain_db::PRIMARY_KEY.to_string(),
                AttributeValue {
                    s: Some(subdomain.to_string()),
                    ..Default::default()
                },
            );
            item
        };

        let result = self.client.get_item(input).await?;
        let account_str = result
            .item
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
