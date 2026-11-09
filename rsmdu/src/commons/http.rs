//! Clients HTTP bloquants partagés — un `Client` réutilisé garde le pool de connexions.

use anyhow::{Context, Result};
use reqwest::blocking::Client;
use std::sync::OnceLock;
use std::time::Duration;

const USER_AGENT: &str = concat!(
    "Mozilla/5.0 (compatible; pymdurs/0.1; ",
    "+https://github.com/rupeelab17/pymdurs)"
);

static IGN: OnceLock<Client> = OnceLock::new();
static DOWNLOAD: OnceLock<Client> = OnceLock::new();
static RNB: OnceLock<Client> = OnceLock::new();
static QUERY: OnceLock<Client> = OnceLock::new();

fn get_or_build(
    cell: &'static OnceLock<Client>,
    build: impl FnOnce() -> Result<Client>,
) -> Result<&'static Client> {
    if let Some(c) = cell.get() {
        return Ok(c);
    }
    let client = build()?;
    Ok(cell.get_or_init(|| client))
}

/// IGN WFS/WMS — réponses volumineuses ; HTTP/1 only (coupures HTTP/2 sur certains CDN).
pub fn ign() -> Result<&'static Client> {
    get_or_build(&IGN, || {
        Client::builder()
            .timeout(Duration::from_secs(900))
            .connect_timeout(Duration::from_secs(120))
            .user_agent(USER_AGENT)
            .http1_only()
            .build()
            .context("Failed to build IGN HTTP client")
    })
}

/// Gros téléchargements (LAZ / COPC / ZIP).
pub fn download() -> Result<&'static Client> {
    get_or_build(&DOWNLOAD, || {
        Client::builder()
            .connect_timeout(Duration::from_secs(30))
            .timeout(Duration::from_secs(900))
            .user_agent(USER_AGENT)
            .build()
            .context("Failed to build download HTTP client")
    })
}

/// API RNB — `danger_accept_invalid_certs` (équivalent Python `verify=False`).
pub fn rnb() -> Result<&'static Client> {
    get_or_build(&RNB, || {
        Client::builder()
            .timeout(Duration::from_secs(120))
            .connect_timeout(Duration::from_secs(30))
            .user_agent(USER_AGENT)
            .danger_accept_invalid_certs(true)
            .build()
            .context("Failed to build RNB HTTP client")
    })
}

/// Requêtes courtes (WFS metadata, etc.).
pub fn query() -> Result<&'static Client> {
    get_or_build(&QUERY, || {
        Client::builder()
            .timeout(Duration::from_secs(120))
            .connect_timeout(Duration::from_secs(30))
            .user_agent(USER_AGENT)
            .build()
            .context("Failed to build query HTTP client")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clients_are_singletons() {
        let a = download().expect("download client");
        let b = download().expect("download client again");
        assert!(std::ptr::eq(a, b));
    }
}
