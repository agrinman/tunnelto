use std::net::SocketAddr;

use crate::Config;
use cli_table::format::Padding;
use cli_table::{format::Justify, print_stderr, Cell, Table};
use colored::Colorize;
use indicatif::{ProgressBar, ProgressStyle};

pub struct CliInterface {
    spinner: ProgressBar,
    config: Config,
    introspect: SocketAddr,
}
impl CliInterface {
    pub fn start(config: Config, introspect: SocketAddr) -> Self {
        let spinner = new_spinner("Opening remote tunnel...");
        Self {
            spinner,
            config,
            introspect,
        }
    }

    fn get_sub_domain_notice(&self, sub_domain: &str) -> Option<String> {
        if self.config.sub_domain.is_some()
            && (self.config.sub_domain.as_ref().map(|s| s.as_str()) != Some(sub_domain))
        {
            if self.config.secret_key.is_some() {
                Some(format!("{}",
                          "To use custom sub-domains feature, please upgrade your billing plan at https://dashboard.tunnelto.dev.".yellow()))
            } else {
                Some(format!("{}",
                          "To access the sub-domain feature, get your authentication key at https://dashboard.tunnelto.dev.".yellow()))
            }
        } else {
            None
        }
    }

    pub fn did_connect(&self, sub_domain: &str, full_hostname: &str) {
        self.spinner
            .finish_with_message("Success! Remote tunnel is now open.\n".green().as_ref());

        if !self.config.first_run {
            return;
        }

        let public_url = self.config.activation_url(&full_hostname).bold().green();
        let forward_url = self.config.forward_url();
        let inspect = format!("http://localhost:{}", self.introspect.port());

        let table = vec![
            vec![
                "Public tunnel URL".green().cell(),
                public_url
                    .green()
                    .cell()
                    .padding(Padding::builder().left(4).right(4).build())
                    .justify(Justify::Left),
            ],
            vec![
                "Local inspect dashboard".magenta().cell(),
                inspect
                    .magenta()
                    .cell()
                    .padding(Padding::builder().left(4).build())
                    .justify(Justify::Left),
            ],
            vec![
                "Forwarding traffic to".cell(),
                forward_url
                    .cell()
                    .padding(Padding::builder().left(4).build())
                    .justify(Justify::Left),
            ],
        ];

        let table = table.table();
        print_stderr(table).expect("failed to generate starting terminal user interface");

        if let Some(notice) = self.get_sub_domain_notice(sub_domain) {
            eprintln!("\n{}: {}\n", ">>> Notice".yellow(), notice);
        }
    }
}

fn new_spinner(message: &str) -> ProgressBar {
    let pb = ProgressBar::new_spinner();
    pb.enable_steady_tick(150);
    pb.set_style(
        ProgressStyle::default_spinner()
            // .tick_strings(&["⣾", "⣽", "⣻", "⢿", "⡿", "⣟", "⣯", "⣷"])
            .tick_strings(&["🌎", "🌍", "🌏"])
            .template("{spinner:.blue} {msg}"),
    );
    pb.set_message(message);
    pb
}
