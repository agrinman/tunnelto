//    pub static ref ALLOWED_HOSTS: Vec<String> = allowed_host_suffixes();
//     pub static ref BLOCKED_SUB_DOMAINS: Vec<String> = blocked_sub_domains_suffixes();
//     pub static ref AUTH_DB_SERVICE: AuthDbService =
//         AuthDbService::new().expect("failed to init auth-service");
//     pub static ref REMOTE_PORT: u16 = remote_port();
//     pub static ref CTRL_PORT: u16 = ctrl_port();
//     pub static ref NET_PORT: u16 = network_port();

use crate::auth::SigKey;

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
            log::warn!("WARNING! generating ephemeral signature key!");
            SigKey::generate()
        };

        let gossip_dns_host = if let Some(app_name) = std::env::var("FLY_APP_NAME").ok() {
            Some(format!("global.{}.internal", app_name))
        } else {
            None
        };

        Config {
            allowed_hosts,
            blocked_sub_domains,
            control_port: get_port("CTRL_PORT", 5000),
            remote_port: get_port("PORT", 8080),
            internal_network_port: get_port("NET_PORT", 6000),
            master_sig_key,
            gossip_dns_host,
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
