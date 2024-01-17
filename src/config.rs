use cloudflare::framework::auth::Credentials;
use color_eyre::eyre::bail;
use std::{collections::HashMap, env, fs::File, io, path::PathBuf};

use clap::Parser;
use color_eyre::{eyre::WrapErr, Result};
use serde::Deserialize;

/// Cloudflare DDNS updater
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Args {
    /// Time To Live in seconds. Minimum 60, maximum 86400. 1 means auto
    #[arg(short, long)]
    pub ttl: Option<u32>,

    /// Config file path. Default path is ~/.config/cf-ddns/config.toml
    /// (XDG_CONFIG_HOME is used instead of ~/.config/ if set)
    #[arg(short, long = "config")]
    pub config_path: Option<PathBuf>,

    /// Cloudflare API Token
    #[arg(long, env = "CF_API_TOKEN")]
    pub api_token: Option<String>,

    /// Cloudflare API Key (must be used with Account Email)
    #[arg(long, env = "CF_API_KEY")]
    pub api_key: Option<String>,

    /// Cloudflare Account Email (must be used with API Key)
    #[arg(long, env = "CF_ACCOUNT_EMAIL")]
    pub account_email: Option<String>,

    /// Zone Id
    #[arg(long, env = "CF_ZONE_ID")]
    pub zone_id: Option<String>,

    /// Proxied
    #[arg(long)]
    pub proxied: Option<bool>,

    /// A record (IPv4)
    #[arg(long)]
    pub a: Option<bool>,

    /// AAAA record (IPv6)
    #[arg(long)]
    pub aaaa: Option<bool>,

    /// Specify one subdomain prefix to be used instead of the ones in the config file.
    /// Useful for debugging or running without a config file altogether
    #[arg(long)]
    pub subdomain: Option<String>,
}

#[derive(Deserialize, Clone, Debug, Default)]
pub struct SubdomainsConfig {
    pub zone_id: Option<String>,
    pub ttl: Option<u32>,
    pub proxied: Option<bool>,
    pub a: Option<bool>,
    pub aaaa: Option<bool>,
}

#[derive(Deserialize, Debug, Default)]
pub struct TomlConfig {
    #[serde(rename = "subdomains")]
    pub subdomains_config: SubdomainsConfig,
    #[serde(rename = "subdomain")]
    pub subdomains: HashMap<String, SubdomainsConfig>,
    pub cloudflare: Option<TomlCloudflare>,
}

#[derive(Deserialize, Debug, Default)]
pub struct TomlCloudflare {
    pub api_token: Option<String>,
    pub api_key: Option<String>,
    pub account_email: Option<String>,
}

#[derive(Debug)]
pub struct Cloudflare {
    pub auth: Credentials,
}

pub fn get_toml_config_or_default(args: &Args) -> Result<TomlConfig> {
    let config_file = match &args.config_path {
        Some(config_path) => File::open(config_path),
        None => {
            let config_home = env::var("XDG_CONFIG_HOME").unwrap_or("~/.config/".to_string());
            File::open(
                PathBuf::from(config_home)
                    .join("cf-ddns")
                    .join("config.toml"),
            )
        }
    };

    match config_file {
        Ok(config_file) => {
            let config_data = io::read_to_string(config_file)?;
            Ok(toml::from_str(&config_data)?)
        }
        Err(err) => {
            if args.config_path.is_some() {
                return Err(err).wrap_err("-c supplied but couldn't open file");
            }
            Ok(TomlConfig::default())
        }
    }
}

pub trait NewCredentials {
    fn new(
        args_api_token: Option<String>,
        args_api_key: Option<String>,
        args_account_email: Option<String>,
        toml_cloudflare: Option<TomlCloudflare>,
    ) -> Result<Self>
    where
        Self: Sized;
}
impl NewCredentials for Credentials {
    fn new(
        args_api_token: Option<String>,
        args_api_key: Option<String>,
        args_account_email: Option<String>,
        toml_cloudflare: Option<TomlCloudflare>,
    ) -> Result<Credentials> {
        let TomlCloudflare {
            api_token: toml_api_token,
            api_key: toml_api_key,
            account_email: toml_account_email,
        } = toml_cloudflare.unwrap_or_default();

        if let Some(token) = args_api_token.or(toml_api_token) {
            return Ok(Credentials::UserAuthToken { token });
        }

        let api_key = args_api_key.or(toml_api_key);
        let Some(api_key) = api_key else {
            bail!("Neither api token nor api key were specified");
        };

        let account_email = args_account_email.or(toml_account_email);
        let Some(account_email) = account_email else {
            bail!("Account email not specified when api key was");
        };

        Ok(Credentials::UserAuthKey {
            email: account_email,
            key: api_key,
        })
    }
}

#[derive(Debug)]
pub struct Config {
    pub cloudflare: Cloudflare,
    pub subdomains_config: SubdomainsConfig,
    pub subdomains: HashMap<String, SubdomainsConfig>,
}

impl Config {
    pub fn new(args: Args) -> Result<Config> {
        let toml = get_toml_config_or_default(&args)?;

        let auth = Credentials::new(
            args.api_token,
            args.api_key,
            args.account_email,
            toml.cloudflare,
        )?;

        let subdomains_config = toml.subdomains_config;
        let zone_id = args.zone_id.or(subdomains_config.zone_id);

        if zone_id.is_none() {
            // Check if all the subdomains have zone_id specified
            let unspecified_zone_ids: Vec<&String> = toml
                .subdomains
                .iter()
                .filter(|(_, config)| config.zone_id.is_none())
                .map(|(name, _config)| name)
                .collect();

            if !unspecified_zone_ids.is_empty() {
                bail!(
                    "zone_id not specified in toml or in arguments.
                    Subdomains missing zone_ids: {unspecified_zone_ids:?}"
                );
            }
        }

        let subdomains = if let Some(name) = args.subdomain {
            [(name, SubdomainsConfig::default())].into()
        } else {
            toml.subdomains
        };

        Ok(Self {
            cloudflare: Cloudflare { auth },
            subdomains_config: SubdomainsConfig {
                zone_id,
                ttl: args.ttl.or(subdomains_config.ttl),
                proxied: args.proxied.or(subdomains_config.proxied),
                a: args.a.or(subdomains_config.a),
                aaaa: args.aaaa.or(subdomains_config.aaaa),
            },
            subdomains,
        })
    }
}
