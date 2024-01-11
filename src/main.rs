use clap::Parser;
use color_eyre::Result;

mod client;
mod config;
mod util;

use crate::client::*;
use crate::config::*;

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;

    let args = Args::parse();

    let config = Config::new(args)?;
    let mut client = Client::new(config)?;

    // Cloning here is dumb but necessary to appease the borrow checker smh
    for (subdomain, config) in client.config.subdomains.clone().iter() {
        client.commit_record(subdomain, config).await?;
    }

    Ok(())
}
