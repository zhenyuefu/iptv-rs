//! Optional media listeners alongside the aggregation service.
mod catalog;
mod media;
mod profiles;

use anyhow::{Context, Result, ensure};
use axum::{Router, http::StatusCode, routing::get};
use ipnet::IpNet;
use lru::LruCache;
use reqwest::{Client, Url};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    net::SocketAddr,
    num::NonZeroUsize,
    path::Path,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{runtime::Runtime, sync::Mutex as AsyncMutex};

type HttpResult<T> = Result<T, StatusCode>;
const GATEWAY: StatusCode = StatusCode::BAD_GATEWAY;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    #[serde(default)]
    profiles: Vec<profiles::Profile>,
    listeners: Vec<Listener>,
    sources: Vec<Source>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Listener {
    bind: SocketAddr,
    public_base: Url,
}

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum Format {
    Hls,
    Ts,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Source {
    id: String,
    group: String,
    playlist: Url,
    format: Format,
    #[serde(default)]
    origins: Vec<Url>,
    #[serde(default)]
    http_networks: Vec<IpNet>,
}

fn http_url(url: &Url) -> bool {
    matches!(url.scheme(), "http" | "https")
        && url.has_host()
        && url.username().is_empty()
        && url.password().is_none()
}

fn bare_origin(url: &Url) -> bool {
    http_url(url) && url.path() == "/" && url.query().is_none() && url.fragment().is_none()
}

impl Source {
    fn allows(&self, url: &Url) -> bool {
        if !http_url(url) {
            return false;
        }
        if self
            .origins
            .iter()
            .any(|base| base.origin() == url.origin())
        {
            return true;
        }
        let address: std::net::IpAddr = match url.host() {
            Some(url::Host::Ipv4(ip)) => ip.into(),
            Some(url::Host::Ipv6(ip)) => ip.into(),
            _ => return false,
        };
        url.scheme() == "http"
            && url.port_or_known_default() == Some(80)
            && self.http_networks.iter().any(|net| net.contains(&address))
    }
}

impl Format {
    fn suffix(&self) -> &'static str {
        match self {
            Self::Ts => ".ts",
            Self::Hls => ".m3u8",
        }
    }
}

fn channel_key(name: &str, url: &Url) -> String {
    identifier(&format!("{name}|{}", url.path()))
}

fn identifier(value: &str) -> String {
    let hash = Sha256::digest(value.as_bytes());
    let mut key = String::with_capacity(24);
    for byte in &hash[..12] {
        use std::fmt::Write;
        write!(key, "{byte:02x}").unwrap();
    }
    key
}

struct Relay {
    http: Client,
    sources: Vec<Source>,
    ids: HashMap<String, usize>,
    catalogs: Vec<AsyncMutex<catalog::Cached>>,
    resources: Mutex<LruCache<String, (usize, Url)>>,
}

#[derive(Clone)]
struct Endpoint {
    relay: Arc<Relay>,
    base: String,
}

impl Config {
    fn validate(&self) -> Result<()> {
        ensure!(
            !self.listeners.is_empty() && !self.sources.is_empty(),
            "configure relay listeners and sources"
        );
        for listener in &self.listeners {
            ensure!(
                bare_origin(&listener.public_base),
                "public_base must be an HTTP(S) origin"
            );
        }
        let mut ids = HashSet::new();
        for s in &self.sources {
            ensure!(
                !s.id.is_empty()
                    && s.id
                        .bytes()
                        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
                    && s.id != "playlist"
                    && ids.insert(&s.id),
                "invalid or duplicate relay source ID"
            );
            ensure!(
                !s.group.is_empty()
                    && !s.group.contains(['"', '\r', '\n'])
                    && http_url(&s.playlist),
                "invalid group or playlist for {}",
                s.id
            );
            ensure!(
                s.origins.iter().all(bare_origin)
                    && !(s.origins.is_empty() && s.http_networks.is_empty()),
                "configure allowed media destinations for {}",
                s.id
            );
        }
        Ok(())
    }
}

pub struct RunningRelay {
    pub runtime: Runtime,
    relay: Arc<Relay>,
    profiles: Vec<profiles::Profile>,
}

pub fn open(path: &Path, listen: bool) -> Result<RunningRelay> {
    let cfg: Config =
        serde_json::from_slice(&std::fs::read(path)?).context("invalid relay config")?;
    cfg.validate()?;
    profiles::validate(&cfg)?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    let relay = Arc::new(Relay {
        http: Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .connect_timeout(Duration::from_secs(6))
            .read_timeout(Duration::from_secs(30))
            .build()?,
        ids: cfg
            .sources
            .iter()
            .enumerate()
            .map(|(i, s)| (s.id.clone(), i))
            .collect(),
        catalogs: cfg
            .sources
            .iter()
            .map(|_| AsyncMutex::new(catalog::Cached::default()))
            .collect(),
        sources: cfg.sources,
        resources: Mutex::new(LruCache::new(NonZeroUsize::new(32768).unwrap())),
    });
    runtime.block_on(async {
        let mut bound = Vec::new();
        for listener in cfg.listeners.into_iter().filter(|_| listen) {
            let socket = tokio::net::TcpListener::bind(listener.bind)
                .await
                .with_context(|| format!("binding relay {}", listener.bind))?;
            bound.push((socket, listener));
        }
        for (socket, listener) in bound {
            let state = Endpoint {
                relay: relay.clone(),
                base: listener.public_base.origin().ascii_serialization(),
            };
            let routes = Router::new()
                .route("/{list}", get(catalog::playlist))
                .route("/live/{source}/{key}", get(media::channel))
                .route("/media/{key}", get(media::resource))
                .with_state(state);
            tokio::spawn(async move {
                if let Err(e) = axum::serve(socket, routes).await {
                    eprintln!("relay listener {} failed: {e}", listener.bind);
                }
            });
        }
        Ok::<_, anyhow::Error>(())
    })?;
    Ok(RunningRelay {
        runtime,
        relay,
        profiles: cfg.profiles,
    })
}
