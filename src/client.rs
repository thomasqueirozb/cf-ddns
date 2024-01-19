use std::collections::HashMap;

use cloudflare::endpoints::dns;
use cloudflare::endpoints::zone;
use cloudflare::framework::async_api::Client as CClient;
use cloudflare::framework::Environment;
use color_eyre::eyre::Context;
use color_eyre::Result;
use log::{debug, info, warn};

use crate::config::*;
use crate::util::*;

pub struct Client {
    pub config: Config,
    authed_client: CClient,
    zone_id_cache: HashMap<String, String>,
    ip_cache: [Option<String>; 2],
}

impl Client {
    pub fn new(config: Config) -> Result<Self> {
        let authed_client = CClient::new(
            config.cloudflare.auth.clone(),
            Default::default(),
            Environment::Production,
        )?;

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

    pub async fn get_zone_details(&mut self, zone_id: &str) -> Result<String> {
        if let Some(zone_details) = self.zone_id_cache.get(zone_id) {
            return Ok(zone_details.clone());
        };

        let zone_details = self
            .authed_client
            .request(&zone::ZoneDetails {
                identifier: zone_id,
            })
            .await
            .with_context(|| format!("Failed to get zone details (zone: {zone_id})"))?;

        self.zone_id_cache
            .insert(zone_id.to_string(), zone_details.result.name.clone());
        Ok(zone_details.result.name)
    }

    pub async fn get_dns_records(&self, zone_id: &str, fqdn: &str) -> Result<Vec<dns::DnsRecord>> {
        let records = self
            .authed_client
            .request(&dns::ListDnsRecords {
                zone_identifier: zone_id,
                params: dns::ListDnsRecordsParams {
                    per_page: Some(100),
                    name: Some(fqdn.to_string()),
                    ..Default::default()
                },
            })
            .await
            .with_context(|| {
                format!("Failed to get dns records (zone: {zone_id}, name: {fqdn})")
            })?;
        Ok(records.result)
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
        let base_domain_name = self.get_zone_details(&zone_id).await?;
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

            if let Some((record, record_ip)) =
                dns_records.iter().find_map(|record| match ip_version {
                    IP::V4 => {
                        if let dns::DnsContent::A { content } = record.content {
                            Some((record, content.to_string()))
                        } else {
                            None
                        }
                    }
                    IP::V6 => {
                        if let dns::DnsContent::AAAA { content } = record.content {
                            Some((record, content.to_string()))
                        } else {
                            None
                        }
                    }
                })
            {
                let ip = self.get_ip(ip_version).await?;

                let content = match ip_version {
                    IP::V4 => dns::DnsContent::A {
                        content: ip.parse().unwrap(),
                    },
                    IP::V6 => dns::DnsContent::AAAA {
                        content: ip.parse().unwrap(),
                    },
                };
                let id = &record.id;

                if record.proxied == proxied && record_ip == ip && record.ttl == ttl {
                    info!("{fqdn}: record {id} doesn't need to be modified");
                } else {
                    info!(
                        "{fqdn}: updating {type_} record with id {id}. Old ip: {}",
                        record_ip,
                    );
                    debug!("{fqdn}: old record: {record:?}");
                    let record = self
                        .authed_client
                        .request(&dns::UpdateDnsRecord {
                            identifier: id,
                            zone_identifier: &zone_id,
                            params: dns::UpdateDnsRecordParams {
                                ttl: Some(ttl),
                                proxied: Some(proxied),
                                name: &fqdn,
                                content,
                            },
                        })
                        .await
                        .with_context(|| format!("Failed to update {type_} record for {fqdn}"))?;

                    info!("{fqdn}: succesfully updated {type_} record with id {id}. New ip: {ip}");
                    debug!("{fqdn}: new record: {:?}", record.result);
                }
            } else {
                info!("{fqdn}: {type_} record not found, creating it");

                let ip = self.get_ip(ip_version).await?;
                let content = match ip_version {
                    IP::V4 => dns::DnsContent::A {
                        content: ip.parse().unwrap(),
                    },
                    IP::V6 => dns::DnsContent::AAAA {
                        content: ip.parse().unwrap(),
                    },
                };

                let record = self
                    .authed_client
                    .request(&dns::CreateDnsRecord {
                        zone_identifier: &zone_id,
                        params: dns::CreateDnsRecordParams {
                            content,
                            name: &fqdn,
                            proxied: Some(proxied),
                            ttl: Some(ttl),
                            priority: None,
                        },
                    })
                    .await
                    .with_context(|| format!("Failed to create {type_} record for {fqdn}"))?;

                info!(
                    "{fqdn}: successfully created {type_} record. id: {}, ip: {:?}",
                    record.result.id, record.result.content
                );
            }
        }

        Ok(())
    }
}
