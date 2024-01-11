use std::collections::HashMap;

use color_eyre::eyre::ensure;
use color_eyre::Result;
use log::{debug, info, warn};
use serde::Deserialize;
use serde::Serialize;
use serde_json::Number;
use serde_json::Value;

use crate::config::*;
use crate::util::*;

#[derive(Debug, Deserialize)]
struct CloudflareApiResponse {
    errors: Vec<Value>,
    messages: Vec<Value>,
    success: bool,
}

impl CloudflareApiResponse {
    pub fn ensure_success(&self, custom_message: String) -> Result<()> {
        ensure!(
            self.success,
            "{}\nMessages: {:?}\nErrors: {:?}",
            custom_message,
            self.messages,
            self.errors
        );
        Ok(())
    }
}

// https://developers.cloudflare.com/api/operations/zones-0-get
#[derive(Debug, Deserialize)]
struct ZoneDetails {
    #[serde(flatten)]
    api: CloudflareApiResponse,
    result: ZoneDetailsResult,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ZoneDetailsResult {
    pub name: String,
}

// https://developers.cloudflare.com/api/operations/dns-records-for-a-zone-list-dns-records
#[derive(Debug, Deserialize)]
pub struct DNSRecords {
    #[serde(flatten)]
    api: CloudflareApiResponse,
    result: Vec<DNSRecordsResult>,
}

#[allow(unused)] // TODO log the result
#[derive(Debug, Deserialize)]
pub struct DNSRecord {
    #[serde(flatten)]
    api: CloudflareApiResponse,
    result: DNSRecordsResult,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DNSRecordsResult {
    content: String,
    name: String,
    id: Option<String>,
    #[serde(rename = "type")]
    type_: String,
    proxied: Option<bool>,
    ttl: Option<Number>,
    // locked: bool, // TODO check this
    // proxiable: bool, // TODO check this
    // comment: Option<String>,
    // tags: Option<Vec<String>>,
}

pub struct Client {
    pub config: Config,
    authed_client: reqwest::Client,
    zone_id_cache: HashMap<String, ZoneDetailsResult>,
    ip_cache: [Option<String>; 2],
}

impl Client {
    pub fn new(config: Config) -> Result<Self> {
        let default_headers = config.cloudflare.auth.headers()?;

        let authed_client = reqwest::Client::builder()
            .default_headers(default_headers)
            .build()?;

        Ok(Client {
            config,
            authed_client,
            zone_id_cache: Default::default(),
            ip_cache: Default::default(),
        })
    }

    pub async fn get_ip(&mut self, version: IP) -> Result<String> {
        let idx = version as usize;
        Ok(match &self.ip_cache[idx] {
            Some(s) => s.clone(),
            None => {
                let ip = get_ip(version).await?;
                self.ip_cache[idx] = Some(ip.clone());
                ip
            }
        })
    }

    pub async fn get_zone_details(&mut self, zone_id: &String) -> Result<ZoneDetailsResult> {
        if let Some(zone_details) = self.zone_id_cache.get(zone_id.as_str()) {
            return Ok(zone_details.clone());
        };

        let zone_details = self
            .authed_client
            .get(format!(
                "https://api.cloudflare.com/client/v4/zones/{zone_id}"
            ))
            .send()
            .await?
            .ensure_status_code(200)?
            .json::<ZoneDetails>()
            .await?;

        zone_details
            .api
            .ensure_success(format!("Failed to get zone details (zone: {zone_id})"))?;

        self.zone_id_cache
            .insert(zone_id.to_string(), zone_details.result.clone());
        Ok(zone_details.result)
    }

    pub async fn get_dns_records(
        &self,
        zone_id: &String,
        fqdn: &String,
    ) -> Result<Vec<DNSRecordsResult>> {
        let dns_records = self
            .authed_client
            .get(format!(
                "https://api.cloudflare.com/client/v4/zones/{zone_id}/dns_records\
                ?per_page=100&name={fqdn}",
            ))
            .send()
            .await?
            .ensure_status_code(200)?
            .json::<DNSRecords>()
            .await?;

        dns_records.api.ensure_success(format!(
            "Failed to get dns records (zone: {zone_id}, name: {fqdn})"
        ))?;

        Ok(dns_records.result)
    }

    pub async fn commit_record(
        &mut self,
        subdomain: &str,
        config: &SubdomainsConfig,
    ) -> Result<()> {
        debug!("[commit_record] subdomain: {subdomain}");
        let zone_id = config
            .zone_id
            .as_ref()
            .or(self.config.subdomains_config.zone_id.as_ref())
            .expect("zone_id is None even after checks")
            .to_string();
        let zone_details = self.get_zone_details(&zone_id).await?;
        let base_domain_name = zone_details.name;
        debug!("Base domain name: {base_domain_name}");

        let name = subdomain.to_lowercase();
        let name = name.trim();
        let fqdn = if !matches!(name, "" | "@") {
            format!("{name}.{base_domain_name}")
        } else {
            base_domain_name
        };
        debug!("fqdn: {fqdn}");

        let a = config.a.or(self.config.subdomains_config.a).unwrap_or(true);
        let aaaa = config
            .aaaa
            .or(self.config.subdomains_config.aaaa)
            .unwrap_or(false);

        if (a, aaaa) == (false, false) {
            warn!("A = false and AAAA = false for subdomain {name}");
            return Ok(());
        }

        let dns_records = self.get_dns_records(&zone_id, &fqdn).await?;

        let proxied = config
            .proxied
            .or(self.config.subdomains_config.proxied)
            .unwrap_or(true);

        let ttl = config
            .ttl
            .or(self.config.subdomains_config.ttl)
            .unwrap_or(1);

        for (use_, type_, ip_version) in [(a, "A", IP::V4), (aaaa, "AAAA", IP::V6)] {
            if !use_ {
                continue;
            }

            if let Some(record) = dns_records.iter().find(|record| record.type_ == type_) {
                let ip = self.get_ip(ip_version).await?;
                let id = record.id.as_ref().unwrap();
                let rttl = record.ttl.as_ref().and_then(Number::as_u64).unwrap_or(1);

                if record.proxied == Some(proxied) && record.content == ip && rttl == (ttl as u64) {
                    info!("{fqdn}: record {id} doesn't need to be modified");
                } else {
                    info!(
                        "{fqdn}: patching {type_} record with id {id}. Old ip: {}",
                        record.content
                    );
                    debug!("{fqdn}: old record: {record:?}");
                    let record = self
                        .authed_client
                        .patch(format!(
                            "https://api.cloudflare.com/client/v4/zones/{zone_id}/dns_records/{id}",
                        ))
                        .json(&DNSRecordsResult {
                            content: ip.clone(),
                            name: fqdn.clone(),
                            id: None,
                            proxied: Some(proxied),
                            type_: type_.to_string(),
                            ttl: Some(ttl.into()),
                        })
                        .send()
                        .await?
                        .ensure_status_code(200)?
                        .json::<DNSRecord>()
                        .await?;

                    record
                        .api
                        .ensure_success(format!("Failed to patch {type_} record for {fqdn}"))?;

                    info!("{fqdn}: succesfully patched {type_} record with id {id}. New ip: {ip}");
                    debug!("{fqdn}: new record: {:?}", record.result);
                }
            } else {
                info!("{fqdn}: {type_} record not found, creating it");
                let record = self
                    .authed_client
                    .post(format!(
                        "https://api.cloudflare.com/client/v4/zones/{zone_id}/dns_records"
                    ))
                    .json(&DNSRecordsResult {
                        content: self.get_ip(ip_version).await?,
                        name: fqdn.clone(),
                        id: None,
                        type_: type_.to_string(),
                        proxied: Some(proxied),
                        ttl: Some(ttl.into()),
                    })
                    .send()
                    .await?
                    .ensure_status_code(200)?
                    .json::<DNSRecord>()
                    .await?;

                record
                    .api
                    .ensure_success(format!("Failed to create {type_} record for {fqdn}"))?;

                info!(
                    "{fqdn}: successfully created {type_} record. id: {}, ip: {}",
                    record.result.id.unwrap(),
                    record.result.content
                );
            }
        }

        Ok(())
    }
}
