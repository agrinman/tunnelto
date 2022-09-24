use std::net::{SocketAddr, ToSocketAddrs};

use super::*;
use structopt::StructOpt;

const HOST_ENV: &'static str = "CTRL_HOST";
const PORT_ENV: &'static str = "CTRL_PORT";
const TLS_OFF_ENV: &'static str = "CTRL_TLS_OFF";

const DEFAULT_HOST: &'static str = "tunnelto.dev";
const DEFAULT_CONTROL_HOST: &'static str = "wormhole.tunnelto.dev";
const DEFAULT_CONTROL_PORT: &'static str = "10001";

const SETTINGS_DIR: &'static str = ".tunnelto";
const SECRET_KEY_FILE: &'static str = "key.token";

/// Command line arguments
#[derive(Debug, StructOpt)]
#[structopt(
    name = "tunnelto",
    author = "support@tunnelto.dev",
    about = "Expose your local web server to the internet with a public url."
)]
struct Opts {
    /// A level of verbosity, and can be used multiple times
    #[structopt(short = "v", long = "verbose")]
    verbose: bool,

    #[structopt(subcommand)]
    command: Option<SubCommand>,

    /// Sets an API authentication key to use for this tunnel
    #[structopt(short = "k", long = "key")]
    key: Option<String>,

    /// Specify a sub-domain for this tunnel
    #[structopt(short = "s", long = "subdomain")]
    sub_domain: Option<String>,

    /// Sets the HOST (i.e. localhost) to forward incoming tunnel traffic to
    #[structopt(long = "host", default_value = "localhost")]
    local_host: String,

    /// Sets the protocol for local forwarding (i.e. https://localhost) to forward incoming tunnel traffic to
    #[structopt(long = "use-tls", short = "t")]
    use_tls: bool,

    /// Sets the port to forward incoming tunnel traffic to on the target host
    #[structopt(short = "p", long = "port", default_value = "8000")]
    port: u16,

    /// Sets the address of the local introspection dashboard
    #[structopt(long = "dashboard-port")]
    dashboard_port: Option<u16>,
}

#[derive(Debug, StructOpt)]
enum SubCommand {
    /// Store the API Authentication key
    SetAuth {
        /// Sets an API authentication key on disk for future use
        #[structopt(short = "k", long = "key")]
        key: String,
    },
}

/// Config
#[derive(Debug, Clone)]
pub struct Config {
    pub client_id: ClientId,
    pub control_url: String,
    pub use_tls: bool,
    pub host: String,
    pub local_host: String,
    pub local_port: u16,
    pub local_addr: SocketAddr,
    pub sub_domain: Option<String>,
    pub secret_key: Option<SecretKey>,
    pub control_tls_off: bool,
    pub first_run: bool,
    pub dashboard_port: u16,
    pub verbose: bool,
}

impl Config {
    /// Parse the URL to use to connect to the wormhole control server
    pub fn get() -> Result<Config, ()> {
        // parse the opts
        let opts: Opts = Opts::from_args();

        if opts.verbose {
            std::env::set_var("RUST_LOG", "tunnelto=debug");
        }

        pretty_env_logger::init();

        let (secret_key, sub_domain) = match opts.command {
            Some(SubCommand::SetAuth { key }) => {
                let key = opts.key.unwrap_or(key);
                let settings_dir = match dirs::home_dir().map(|h| h.join(SETTINGS_DIR)) {
                    Some(path) => path,
                    None => {
                        panic!("Could not find home directory to store token.")
                    }
                };
                std::fs::create_dir_all(&settings_dir)
                    .expect("Fail to create file in home directory");
                std::fs::write(settings_dir.join(SECRET_KEY_FILE), key)
                    .expect("Failed to save authentication key file.");

                eprintln!("Authentication key stored successfully!");
                std::process::exit(0);
            }
            None => {
                let key = opts.key;
                let sub_domain = opts.sub_domain;
                (
                    match key {
                        Some(key) => Some(key),
                        None => dirs::home_dir()
                            .map(|h| h.join(SETTINGS_DIR).join(SECRET_KEY_FILE))
                            .map(|path| {
                                if path.exists() {
                                    std::fs::read_to_string(path)
                                        .map_err(|e| {
                                            error!("Error reading authentication token: {:?}", e)
                                        })
                                        .ok()
                                } else {
                                    None
                                }
                            })
                            .unwrap_or(None),
                    },
                    sub_domain,
                )
            }
        };

        let local_addr = match (opts.local_host.as_str(), opts.port)
            .to_socket_addrs()
            .unwrap_or(vec![].into_iter())
            .next()
        {
            Some(addr) => addr,
            None => {
                error!(
                    "An invalid local address was specified: {}:{}",
                    opts.local_host.as_str(),
                    opts.port
                );
                return Err(());
            }
        };

        // get the host url
        let tls_off = env::var(TLS_OFF_ENV).is_ok();
        let host = env::var(HOST_ENV).unwrap_or(format!("{}", DEFAULT_HOST));

        let control_host = env::var(HOST_ENV).unwrap_or(format!("{}", DEFAULT_CONTROL_HOST));

        let port = env::var(PORT_ENV).unwrap_or(format!("{}", DEFAULT_CONTROL_PORT));

        let scheme = if tls_off { "ws" } else { "wss" };
        let control_url = format!("{}://{}:{}/wormhole", scheme, control_host, port);

        info!("Control Server URL: {}", &control_url);

        Ok(Config {
            client_id: ClientId::generate(),
            local_host: opts.local_host,
            use_tls: opts.use_tls,
            control_url,
            host,
            local_port: opts.port,
            local_addr,
            sub_domain,
            dashboard_port: opts.dashboard_port.unwrap_or(0),
            verbose: opts.verbose,
            secret_key: secret_key.map(|s| SecretKey(s)),
            control_tls_off: tls_off,
            first_run: true,
        })
    }

    pub fn activation_url(&self, full_hostname: &str) -> String {
        format!(
            "{}://{}",
            if self.control_tls_off {
                "http"
            } else {
                "https"
            },
            full_hostname
        )
    }

    pub fn forward_url(&self) -> String {
        let scheme = if self.use_tls { "https" } else { "http" };
        format!("{}://{}:{}", &scheme, &self.local_host, &self.local_port)
    }
    pub fn ws_forward_url(&self) -> String {
        let scheme = if self.use_tls { "wss" } else { "ws" };
        format!("{}://{}:{}", scheme, &self.local_host, &self.local_port)
    }
}
