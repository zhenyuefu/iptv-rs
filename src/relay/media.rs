use super::*;
use axum::{
    body::{Body, Bytes},
    extract::{Path, State},
    http::{HeaderMap, header},
    response::{IntoResponse, Response},
};
use futures_util::{StreamExt, stream};
use regex::Regex;
use std::sync::LazyLock;

const MAX_PLAYLIST: usize = 4 * 1024 * 1024;
static URI: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?:^|[:,])(?:[A-Z0-9-]*-)?URI="([^"]+)""#).unwrap());

pub(super) async fn bounded(
    mut response: reqwest::Response,
    mut prefix: Vec<u8>,
) -> HttpResult<Vec<u8>> {
    if prefix.len() > MAX_PLAYLIST
        || response
            .content_length()
            .is_some_and(|n| n > MAX_PLAYLIST as u64)
    {
        return Err(GATEWAY);
    }
    while let Some(chunk) = response.chunk().await.map_err(|_| GATEWAY)? {
        if prefix.len() + chunk.len() > MAX_PLAYLIST {
            return Err(GATEWAY);
        }
        prefix.extend_from_slice(&chunk);
    }
    Ok(prefix)
}

pub(super) fn redirect(response: &reqwest::Response) -> HttpResult<Option<Url>> {
    if !matches!(response.status().as_u16(), 301 | 302 | 303 | 307 | 308) {
        return Ok(None);
    }
    let target = response
        .headers()
        .get(header::LOCATION)
        .and_then(|v| v.to_str().ok())
        .ok_or(GATEWAY)?;
    Ok(Some(response.url().join(target).map_err(|_| GATEWAY)?))
}

impl Relay {
    fn resource_path(&self, index: usize, base: &Url, value: &str) -> HttpResult<String> {
        let url = base.join(value).map_err(|_| GATEWAY)?;
        let source = &self.sources[index];
        if !source.allows(&url) {
            return Err(GATEWAY);
        }
        let key = identifier(&format!("{}|{}", source.id, url));
        let ext = url
            .path()
            .rsplit_once('.')
            .map(|(_, s)| s.to_ascii_lowercase())
            .unwrap_or_default();
        let suffix = match ext.as_str() {
            "ts" | "m3u8" | "m3u" | "m4s" | "mp4" | "aac" | "key" | "vtt" => format!(".{ext}"),
            _ => String::new(),
        };
        let path = format!("/media/{key}{suffix}");
        self.resources.lock().unwrap().put(key, (index, url));
        Ok(path)
    }

    fn rewrite(&self, index: usize, base: &Url, data: &[u8]) -> HttpResult<String> {
        let text = std::str::from_utf8(data).map_err(|_| GATEWAY)?;
        let mut out = String::with_capacity(text.len());
        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with('#') {
                let mut start = 0;
                for caps in URI.captures_iter(line) {
                    let value = caps.get(1).unwrap();
                    out.push_str(&line[start..value.start()]);
                    out.push_str(&self.resource_path(index, base, value.as_str())?);
                    start = value.end();
                }
                out.push_str(&line[start..]);
            } else if !trimmed.is_empty() {
                out.push_str(&self.resource_path(index, base, trimmed)?);
            }
            out.push('\n');
        }
        Ok(out)
    }

    async fn stream(&self, index: usize, mut url: Url, headers: HeaderMap) -> HttpResult<Response> {
        for _ in 0..8 {
            if !self.sources[index].allows(&url) {
                return Err(GATEWAY);
            }
            let path = url.path().to_ascii_lowercase();
            let manifest = path.ends_with(".m3u8") || path.ends_with(".m3u");
            let mut request = self
                .http
                .get(url.clone())
                .header(header::ACCEPT_ENCODING, "identity");
            if !manifest && let Some(range) = headers.get(header::RANGE) {
                request = request.header(header::RANGE, range);
            }
            let mut upstream = request.send().await.map_err(|_| GATEWAY)?;
            if let Some(next) = redirect(&upstream)? {
                url = next;
                continue;
            }
            let status = upstream.status();
            if !status.is_success() {
                let mut response = status.into_response();
                if let Some(range) = upstream.headers().get(header::CONTENT_RANGE) {
                    response
                        .headers_mut()
                        .insert(header::CONTENT_RANGE, range.clone());
                }
                return Ok(response);
            }

            // Some IPTV endpoints use opaque paths and an incorrect MIME type.
            // Inspect just the prefix, then pass media chunks through with backpressure.
            let mut chunks = Vec::new();
            let mut signature = Vec::with_capacity(7);
            while signature.len() < 7 {
                let Some(chunk) = upstream.chunk().await.map_err(|_| GATEWAY)? else {
                    break;
                };
                signature.extend_from_slice(&chunk[..(7 - signature.len()).min(chunk.len())]);
                chunks.push(chunk);
            }
            if signature == b"#EXTM3U" {
                let data = bounded(upstream, chunks.concat()).await?;
                let text = self.rewrite(index, &url, &data)?;
                return Ok((
                    [(header::CONTENT_TYPE, "application/vnd.apple.mpegurl")],
                    text,
                )
                    .into_response());
            }
            let mut response = Response::builder().status(status);
            for name in [
                header::CONTENT_TYPE,
                header::CONTENT_RANGE,
                header::ACCEPT_RANGES,
                header::CONTENT_LENGTH,
                header::CONTENT_ENCODING,
            ] {
                if let Some(value) = upstream.headers().get(&name) {
                    response = response.header(name, value);
                }
            }
            let prefix = stream::iter(chunks.into_iter().map(Ok::<Bytes, reqwest::Error>));
            let body = Body::from_stream(prefix.chain(upstream.bytes_stream()));
            return response.body(body).map_err(|_| GATEWAY);
        }
        Err(GATEWAY)
    }
}

fn bare_key(value: &str) -> &str {
    value.split('.').next().unwrap_or(value)
}

pub(super) async fn channel(
    State(state): State<Endpoint>,
    Path((source, key)): Path<(String, String)>,
    mut headers: HeaderMap,
) -> HttpResult<Response> {
    let index = *state.relay.ids.get(&source).ok_or(StatusCode::NOT_FOUND)?;
    let table = state.relay.catalog(index).await?;
    let url = table
        .urls
        .get(bare_key(&key))
        .ok_or(StatusCode::NOT_FOUND)?
        .clone();
    // The upstream can serve a manifest through an extensionless redirect.
    if matches!(state.relay.sources[index].format, Format::Hls) {
        headers.remove(header::RANGE);
    }
    state.relay.stream(index, url, headers).await
}

pub(super) async fn resource(
    State(state): State<Endpoint>,
    Path(key): Path<String>,
    headers: HeaderMap,
) -> HttpResult<Response> {
    let (index, url) = state
        .relay
        .resources
        .lock()
        .unwrap()
        .get(bare_key(&key))
        .cloned()
        .ok_or(StatusCode::NOT_FOUND)?;
    state.relay.stream(index, url, headers).await
}
