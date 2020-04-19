use structopt::StructOpt;
use super::*;

const HOST_ENV:&'static str = "WORMHOLE_HOST";
const PORT_ENV:&'static str = "WORMHOLE_PORT";
const TLS_OFF_ENV:&'static str = "TLS_OFF";

const DEFAULT_HOST:&'static str = "tunnelto.dev";
const DEFAULT_CONTROL_HOST:&'static str = "wormhole.tunnelto.dev";
const DEFAULT_CONTROL_PORT:&'static str = "10001";

const WORMHOLE_DIR:&'static str = ".wormhole";
const SECRET_KEY_FILE:&'static str = "key.token";

/// Command line arguments
#[derive(Debug, StructOpt)]
#[structopt(name = "wormhole", about = "Expose your local web server to the internet with a public url.")]
struct Opts {
    /// A level of verbosity, and can be used multiple times
    #[structopt(short = "v", long = "verbose")]
    verbose: bool,

    #[structopt(subcommand)]
    command: SubCommand,
}

#[derive(Debug, StructOpt)]
enum SubCommand {
    /// Store the API Authentication key
    Auth {
        /// Sets an API authentication key on disk for future use
        #[structopt(short = "k", long = "key")]
        key: String
    },

    /// Start the wormhole
    Start {
        /// Sets an API authentication key to use for this wormhole
        #[structopt(short = "k", long = "key")]
        key: Option<String>,

        /// Specify a sub-domain for this wormhole
        #[structopt(short = "s", long = "subdomain")]
        sub_domain: Option<String>,

        /// Sets the port to forward incoming tunnel traffic to on localhost
        #[structopt(short = "p", long = "port")]
        port: String,
    }
}

/// Config
#[derive(Debug, Clone)]
pub struct Config {
    pub client_id: ClientId,
    pub control_url: String,
    pub host: String,
    pub local_port: String,
    pub sub_domain: String,
    pub secret_key: SecretKey,
    pub tls_off: bool
}

impl Config {
    /// Parse the URL to use to connect to the wormhole control server
    pub fn get() -> Result<Config, ()> {
        // parse the opts
        let opts: Opts = Opts::from_args();

        if opts.verbose {
            std::env::set_var("RUST_LOG", "wormhole=debug");
        } else {
            std::env::set_var("RUST_LOG", "wormhole=error");
        }

        pretty_env_logger::init();

        let (secret_key, sub_domain, local_port) = match opts.command {
            SubCommand::Auth { key } => {
                let wormhole_dir = match dirs::home_dir().map(|h| h.join(WORMHOLE_DIR)) {
                    Some(path) => path,
                    None => {
                        panic!("Could not find home directory to store token.")
                    }
                };
                std::fs::create_dir_all(&wormhole_dir).expect("Fail to create wormhole file in home directory");
                std::fs::write(wormhole_dir.join(SECRET_KEY_FILE), key).expect("Failed to save authentication key file.");

                eprintln!("Authentication key stored successfully!");
                std::process::exit(0);
            },
            SubCommand::Start { key, sub_domain, port } => {
                (match key {
                    Some(key) => key,
                    None => {
                        let key_file_path = match dirs::home_dir().map(|h| h.join(WORMHOLE_DIR).join(SECRET_KEY_FILE)) {
                            Some(path) => path,
                            None => {
                                panic!("Missing authentication key file. Could not find home directory.")
                            }
                        };

                        std::fs::read_to_string(key_file_path).expect("Missing authentication token. Try running the `auth` command.")
                    }
                }, sub_domain, port)
            }
        };

        // get the host url
        let tls_off = env::var(TLS_OFF_ENV).is_ok();
        let host = env::var(HOST_ENV)
            .unwrap_or(format!("{}", DEFAULT_HOST));

        let control_host = env::var(HOST_ENV)
            .unwrap_or(format!("{}", DEFAULT_CONTROL_HOST));

        let port = env::var(PORT_ENV)
            .unwrap_or(format!("{}", DEFAULT_CONTROL_PORT));

        let scheme = if tls_off { "ws" } else { "wss" };
        let control_url = format!("{}://{}:{}/wormhole", scheme, control_host, port);

        info!("Control Server URL: {}", &control_url);

        Ok(Config {
            client_id: ClientId::generate(),
            control_url,
            host,
            local_port,
            sub_domain: sub_domain.unwrap_or(ServerHello::random_domain()),
            secret_key: SecretKey(secret_key),
            tls_off,
        })
    }

    pub fn activation_url(&self, server_chosen_sub_domain: &str) -> String {
        format!("{}://{}.{}",
                  if self.tls_off { "http" } else { "https" },
                  &server_chosen_sub_domain,
                  &self.host)
    }
}
