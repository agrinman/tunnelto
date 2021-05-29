use std::str::FromStr;

use colored::Colorize;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Update {
    pub html_url: String,
    pub name: String,
}

const UPDATE_URL: &str = "https://api.github.com/repos/agrinman/tunnelto/releases/latest";
const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

pub async fn check() {
    match check_inner().await {
        Ok(Some(new)) => {
            eprintln!(
                "{} {} => {} ({})\n",
                "New version available:".yellow().italic(),
                CURRENT_VERSION.bright_blue(),
                new.name.as_str().green(),
                new.html_url
            );
        }
        Ok(None) => log::debug!("Using latest version."),
        Err(error) => log::error!("Failed to check version: {:?}", error),
    }
}

/// checks for a new release on github
async fn check_inner() -> Result<Option<Update>, Box<dyn std::error::Error>> {
    let update: Update = reqwest::Client::new()
        .get(UPDATE_URL)
        .header("User-Agent", "tunnelto-client")
        .header("Accept", "application/vnd.github.v3+json")
        .send()
        .await?
        .json()
        .await?;

    let cur = semver::Version::from_str(CURRENT_VERSION)?;
    let remote = semver::Version::from_str(&update.name)?;

    if remote > cur {
        Ok(Some(update))
    } else {
        Ok(None)
    }
}
