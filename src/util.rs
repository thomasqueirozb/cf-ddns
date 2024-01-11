use color_eyre::eyre::{ensure, Context, ContextCompat};
use color_eyre::Result;
use reqwest::Response;
use std::fmt::Display;

// Ensure Success is copied from here: https://github.com/thomasqueirozb/autovor/blob/master/src/helper.rs
pub trait EnsureSuccess {
    fn ensure_success(self) -> Result<Self>
    where
        Self: Sized;
    fn ensure_status_code(self, status_code: u16) -> Result<Self>
    where
        Self: Sized;
    fn ensure_success_or<D>(self, msg: D) -> Result<Self>
    where
        D: Display + Send + Sync + 'static,
        Self: Sized;
}

impl EnsureSuccess for Response {
    fn ensure_success(self) -> Result<Self> {
        let status = self.status();
        ensure!(
            status.is_success(),
            "{} returned HTTP status code {}",
            self.url().as_str(),
            status.as_str()
        );
        Ok(self)
    }

    fn ensure_status_code(self, status_code: u16) -> Result<Self> {
        let response_status_code = self.status().as_u16();
        ensure!(
            response_status_code == status_code,
            "{} returned HTTP status code {}, expected {}",
            self.url().as_str(),
            response_status_code,
            status_code,
        );
        Ok(self)
    }

    fn ensure_success_or<D>(self, msg: D) -> Result<Self>
    where
        D: Display + Send + Sync + 'static,
    {
        let status = self.status();
        ensure!(
            status.is_success(),
            "{}\n{} returned HTTP status code {}",
            msg,
            self.url().as_str(),
            status.as_str()
        );
        Ok(self)
    }
}

#[derive(Debug, Clone, Copy)]
#[repr(u8)]
pub enum IP {
    V4,
    V6,
}

pub async fn get_ip(version: IP) -> Result<String> {
    const CF_IPV4_URL: &str = "https://1.1.1.1/cdn-cgi/trace";
    const CF_IPV6_URL: &str = "https://[2606:4700:4700::1111]/cdn-cgi/trace";
    let (ip_str, url) = match version {
        IP::V4 => ("IPv4", CF_IPV4_URL),
        IP::V6 => ("IPv6", CF_IPV6_URL),
    };

    let response = match reqwest::get(url).await {
        Ok(r) => r,
        Err(e) => {
            return if e.is_connect() {
                Err(e).with_context(|| format!("Connection error, check {ip_str} connectivity"))
            } else {
                Err(e.into())
            }
        }
    };
    let text = response.ensure_success()?.text().await?;
    let ip = text
        .lines()
        .find_map(|line| line.strip_prefix("ip=").map(String::from))
        .with_context(|| {
            format!("Couldn't find ip= in the response from {url}\nFull response: {text}")
        })?;

    Ok(ip)
}
