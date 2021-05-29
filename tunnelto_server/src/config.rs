use crate::auth::SigKey;
use std::net::IpAddr;
use std::str::FromStr;
use uuid::Uuid;

/// Global service configuration
pub struct Config {
    /// What hosts do we allow tunnels on:
    /// i.e:    baz.com => *.baz.com
    ///         foo.bar => *.foo.bar
    pub allowed_hosts: Vec<String>,

    /// What sub-domains do we always block:
    /// i.e:    dashboard.tunnelto.dev
    pub blocked_sub_domains: Vec<String>,

    /// port for remote streams (end users)
    pub remote_port: u16,

    /// port for the control server
    pub control_port: u16,

    /// internal port for instance-to-instance gossip coms
    pub internal_network_port: u16,

    /// our signature key
    pub master_sig_key: SigKey,

    /// Instance DNS discovery domain for gossip protocol
    pub gossip_dns_host: Option<String>,

    /// Observability API key
    pub honeycomb_api_key: Option<String>,

    /// The identifier for this instance of the server
    pub instance_id: String,

    /// Blocked IP addresses
    pub blocked_ips: Vec<IpAddr>,

    /// Connection string for "old timey" database engines
    pub db_connection_string: String,
    /// The host on which we create tunnels on
    pub tunnel_host: String,
}

impl Config {
    pub fn from_env() -> Config {
        let allowed_hosts = std::env::var("ALLOWED_HOSTS")
            .map(|s| s.split(",").map(String::from).collect())
            .unwrap_or(vec![]);

        let blocked_sub_domains = std::env::var("BLOCKED_SUB_DOMAINS")
            .map(|s| s.split(",").map(String::from).collect())
            .unwrap_or(vec![]);

        let master_sig_key = if let Ok(key) = std::env::var("MASTER_SIG_KEY") {
            SigKey::from_hex(&key).expect("invalid master key: not hex or length incorrect")
        } else {
            tracing::warn!("WARNING! generating ephemeral signature key!");
            SigKey::generate()
        };

        let gossip_dns_host = std::env::var("FLY_APP_NAME")
            .map(|app_name| format!("global.{}.internal", app_name))
            .ok();

        let honeycomb_api_key = std::env::var("HONEYCOMB_API_KEY").ok();
        let instance_id = std::env::var("FLY_ALLOC_ID").unwrap_or(Uuid::new_v4().to_string());
        let blocked_ips = std::env::var("BLOCKED_IPS")
            .map(|s| {
                s.split(",")
                    .map(IpAddr::from_str)
                    .filter_map(Result::ok)
                    .collect()
            })
            .unwrap_or(vec![]);

        let db_connection_string = match std::env::var("DB_CONNECTION_STRING") {
            Ok(connection_string) => connection_string,
            _ => "./tunnelto.db".to_string(),
        };
        let tunnel_host = std::env::var("TUNNEL_HOST").unwrap_or("tunnelto.dev".to_string());

        Config {
            allowed_hosts,
            blocked_sub_domains,
            control_port: get_port("CTRL_PORT", 5000),
            remote_port: get_port("PORT", 8080),
            internal_network_port: get_port("NET_PORT", 6000),
            master_sig_key,
            gossip_dns_host,
            honeycomb_api_key,
            instance_id,
            blocked_ips,
            db_connection_string,
            tunnel_host,
        }
    }
}

fn get_port(var: &'static str, default: u16) -> u16 {
    if let Ok(port) = std::env::var(var) {
        port.parse().unwrap_or_else(|_| {
            panic!("invalid port ENV {}={}", var, port);
        })
    } else {
        default
    }
}
