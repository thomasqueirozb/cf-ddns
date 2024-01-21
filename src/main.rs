use std::process::ExitCode;

use clap::Parser;
use color_eyre::Result;
use log::error;

mod client;
mod config;
mod util;

use crate::client::*;
use crate::config::*;

#[tokio::main]
async fn main() -> Result<ExitCode> {
    color_eyre::install()?;

    let args = Args::parse();

    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let config = Config::new(args)?;
    let mut client = Client::new(config)?;

    let mut failed = false;
    for (subdomain, config) in &client.config.subdomains.clone() {
        if let Err(e) = client.commit_record(subdomain, config).await {
            error!("Failed to commit record for subdomain {subdomain:?}: {e:?}");
            failed = true;
        }
    }

    Ok((failed as u8).into())
}
