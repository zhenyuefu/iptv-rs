use std::io::Read;
use std::time::Duration;

use anyhow::{Context, Result};
use flate2::read::GzDecoder;
use reqwest::blocking::Client;
use reqwest::header::USER_AGENT;
use url::Url;

use crate::config::Settings;
use crate::source_list::SourceEntry;

pub struct HttpFetcher {
    client: Client,
    default_user_agent: String,
}

impl HttpFetcher {
    pub fn new(settings: &Settings) -> Result<Self> {
        let mut builder = Client::builder().timeout(Duration::from_secs(settings.request_timeout));
        if let Some(proxy) = &settings.http_proxy {
            builder = builder.proxy(reqwest::Proxy::all(proxy)?);
        }

        Ok(Self {
            client: builder.build().context("failed to build HTTP client")?,
            default_user_agent: settings.default_user_agent.clone(),
        })
    }

    pub fn fetch_text(&self, source: &SourceEntry) -> Result<String> {
        let bytes = self.fetch_bytes(source)?;
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }

    pub fn fetch_bytes(&self, source: &SourceEntry) -> Result<Vec<u8>> {
        let bytes = if is_remote(&source.url) {
            let ua = source
                .user_agent
                .as_deref()
                .unwrap_or(&self.default_user_agent);
            self.client
                .get(&source.url)
                .header(USER_AGENT, ua)
                .send()
                .with_context(|| format!("failed to request {}", source.url))?
                .error_for_status()
                .with_context(|| format!("server returned an error for {}", source.url))?
                .bytes()
                .with_context(|| format!("failed to read response body from {}", source.url))?
                .to_vec()
        } else if let Some(path) = source.url.strip_prefix("file://") {
            std::fs::read(path).with_context(|| format!("failed to read {}", path))?
        } else {
            std::fs::read(&source.url)
                .with_context(|| format!("failed to read local source {}", source.url))?
        };

        maybe_decompress_gzip(&bytes)
    }
}

fn is_remote(value: &str) -> bool {
    Url::parse(value)
        .map(|url| matches!(url.scheme(), "http" | "https"))
        .unwrap_or(false)
}

fn maybe_decompress_gzip(bytes: &[u8]) -> Result<Vec<u8>> {
    if bytes.starts_with(&[0x1f, 0x8b]) {
        let mut decoder = GzDecoder::new(bytes);
        let mut decoded = Vec::new();
        decoder
            .read_to_end(&mut decoded)
            .context("failed to decompress gzip payload")?;
        Ok(decoded)
    } else {
        Ok(bytes.to_vec())
    }
}
