use rusqlite::{params, NO_PARAMS, Connection};

use super::AuthResult;
use crate::auth::AuthService;
use async_trait::async_trait;
use sha2::Digest;
use std::str::FromStr;
use thiserror::Error;
use uuid::Uuid;
use std::sync::Mutex;

mod sqlite_conf {
    pub const DB_PATH:&'static str = "./tunnelto.db";
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

pub struct AuthDbService {
    connection: Mutex<Connection>,
}

impl AuthDbService {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let conn = Connection::open(sqlite_conf::DB_PATH.to_string())?;
        conn.execute(
            &format!("CREATE TABLE IF NOT EXISTS {}  (
                    {} TEXT NOT NULL,
                    {}  TEXT NOT NULL
                    )",
                    domain_db::TABLE_NAME,
                    domain_db::PRIMARY_KEY,
                    domain_db::ACCOUNT_ID
            ),
            NO_PARAMS,
        )?;
        conn.execute(
            &format!("CREATE TABLE IF NOT EXISTS {}  (
                    {} TEXT NOT NULL,
                    {}  TEXT NOT NULL
                    )",
                    key_db::TABLE_NAME,
                    key_db::PRIMARY_KEY,
                    key_db::ACCOUNT_ID
            ),
            NO_PARAMS,
        )?;
        conn.execute(
            &format!("CREATE TABLE IF NOT EXISTS {}  (
                    {} TEXT NOT NULL,
                    {}  TEXT NOT NULL
                    )",
                    record_db::TABLE_NAME,
                    record_db::PRIMARY_KEY,
                    record_db::SUBSCRIPTION_ID
            ),
            NO_PARAMS,
        )?;
        Ok( Self{connection: Mutex::new(conn)} )
    }
}

impl Drop for AuthDbService {
    fn drop(&mut self) {
        let c = &*self.connection.lock().unwrap();
        drop(c);
    }
}

fn key_id(auth_key: &str) -> String {
    let hash = sha2::Sha256::digest(auth_key.as_bytes()).to_vec();
    base64::encode_config(&hash, base64::URL_SAFE_NO_PAD)
}

#[derive(Error, Debug)]
pub enum Error {
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

        let conn:&Connection = &*self.connection.lock().unwrap();
        let row: Result<String, _> = conn.query_row(
            &format!("SELECT {} FROM {} WHERE {}=?",
                    key_db::ACCOUNT_ID,
                    key_db::TABLE_NAME,
                    key_db::PRIMARY_KEY
            ),
            params![auth_key_hash,],
            |row| row.get(0)
        );
        Ok(Uuid::from_str(&row.map_err(|_| Error::AccountNotFound)?)?)
    }

    async fn is_account_in_good_standing(&self, account_id: Uuid) -> Result<bool, Error> {
        let conn:&Connection = &*self.connection.lock().unwrap();
        let row: Result<String, _> = conn.query_row(
            &format!("SELECT {} FROM {} WHERE {}=?",
                    record_db::SUBSCRIPTION_ID,
                    record_db::TABLE_NAME,
                    record_db::PRIMARY_KEY
            ),
            params![account_id.to_string(),],
            |row| row.get(0)
        );
        Ok(row.map_or_else(|_| false, |_| true))
    }

    async fn get_account_id_for_subdomain(&self, subdomain: &str) -> Result<Option<Uuid>, Error> {
        let conn:&Connection = &*self.connection.lock().unwrap();
        let row: Result<String, _> = conn.query_row(
            &format!("SELECT {} FROM {} WHERE {}=?",
                    domain_db::ACCOUNT_ID,
                    domain_db::TABLE_NAME,
                    domain_db::PRIMARY_KEY
            ),
            params![subdomain,],
            |row| row.get(0)
        );
        let account_str = row.map_or_else(|_| None, |v| Some(v));

        if let Some(account_str) = account_str {
            Ok(Some(Uuid::from_str(&account_str)?))
        } else {
            Ok(None)
        }
    }
}