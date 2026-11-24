use super::*;
use crate::{models::Origin, playlist::parse_playlist};
use axum::{
    extract::{Path, State},
    http::header,
    response::{IntoResponse, Response},
};
use futures_util::future::join_all;
use std::time::Instant;

pub(super) struct Catalog {
    rows: Vec<(String, String)>,
    pub(super) urls: HashMap<String, Url>,
}

#[derive(Default)]
pub(super) struct Cached {
    catalog: Option<Arc<Catalog>>,
    retry: Option<Instant>,
}

impl Relay {
    pub(super) async fn catalog(&self, index: usize) -> HttpResult<Arc<Catalog>> {
        let mut cache = self.catalogs[index].lock().await;
        if cache.retry.is_none_or(|next| Instant::now() >= next) {
            let result = self.fetch_catalog(index).await;
            let delay = if let Ok(catalog) = result {
                cache.catalog = Some(Arc::new(catalog));
                3600
            } else {
                eprintln!("relay catalogue unavailable for {}", self.sources[index].id);
                15
            };
            cache.retry = Some(Instant::now() + Duration::from_secs(delay));
        }
        cache.catalog.clone().ok_or(StatusCode::SERVICE_UNAVAILABLE)
    }

    async fn fetch_catalog(&self, index: usize) -> HttpResult<Catalog> {
        let source = &self.sources[index];
        let mut url = source.playlist.clone();
        for _ in 0..8 {
            let response = self
                .http
                .get(url.clone())
                .timeout(Duration::from_secs(12))
                .send()
                .await
                .map_err(|_| GATEWAY)?;
            if let Some(next) = media::redirect(&response)? {
                if !http_url(&next) || next.origin() != source.playlist.origin() {
                    return Err(GATEWAY);
                }
                url = next;
                continue;
            }
            if !response.status().is_success() {
                return Err(GATEWAY);
            }
            let data = media::bounded(response, Vec::new()).await?;
            let parsed = parse_playlist(
                std::str::from_utf8(&data)
                    .map_err(|_| GATEWAY)?
                    .trim_start_matches('\u{feff}'),
                &source.id,
                Origin::Subscribe,
                false,
                index,
                None,
                false,
            )
            .map_err(|_| GATEWAY)?;
            let mut table = Catalog {
                rows: Vec::new(),
                urls: HashMap::new(),
            };
            for entry in parsed {
                let Some(stream) = entry.stream else { continue };
                let Ok(url) = Url::parse(&stream.url) else {
                    continue;
                };
                if !source.allows(&url) {
                    continue;
                }
                let key = channel_key(&entry.name, &url);
                if table.urls.insert(key.clone(), url).is_none() {
                    table.rows.push((key, entry.name));
                }
            }
            if table.rows.is_empty() {
                return Err(GATEWAY);
            }
            return Ok(table);
        }
        Err(GATEWAY)
    }
}

pub(super) async fn playlist(
    State(state): State<Endpoint>,
    Path(list): Path<String>,
) -> HttpResult<Response> {
    let combined = matches!(list.as_str(), "m3u" | "playlist.m3u");
    let plain = list.ends_with(".txt");
    let ids = if combined {
        (0..state.relay.sources.len()).collect::<Vec<_>>()
    } else {
        let source = list
            .strip_suffix(".txt")
            .or_else(|| list.strip_suffix(".m3u"))
            .ok_or(StatusCode::NOT_FOUND)?;
        vec![*state.relay.ids.get(source).ok_or(StatusCode::NOT_FOUND)?]
    };
    let tables = join_all(ids.iter().map(|&i| state.relay.catalog(i))).await;
    let mut output = if plain {
        String::new()
    } else {
        "#EXTM3U\n".into()
    };
    let mut available = false;
    for (index, table) in ids.into_iter().zip(tables) {
        let Ok(table) = table else { continue };
        available = true;
        let source = &state.relay.sources[index];
        let extension = if combined { source.format.suffix() } else { "" };
        for (key, name) in &table.rows {
            use std::fmt::Write;
            if plain {
                write!(output, "{name},").unwrap();
            } else {
                writeln!(output, "#EXTINF:-1 group-title=\"{}\",{name}", source.group).unwrap();
            }
            writeln!(output, "{}/live/{}/{key}{extension}", state.base, source.id).unwrap();
        }
    }
    if !available {
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    }
    let mime = if plain {
        "text/plain; charset=utf-8"
    } else {
        "application/vnd.apple.mpegurl"
    };
    Ok(([(header::CONTENT_TYPE, mime)], output).into_response())
}
