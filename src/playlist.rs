use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::{Context, Result};

use crate::config::Settings;
use crate::models::{Channel, ChannelMap, Origin, ParsedChannel, Stream};

pub fn load_local_sources(settings: &Settings) -> Result<Vec<ParsedChannel>> {
    let mut items = Vec::new();
    let local_file = settings.resolve(&settings.local_file);
    if local_file.exists() {
        let text = std::fs::read_to_string(&local_file)
            .with_context(|| format!("failed to read {}", local_file.display()))?;
        items.extend(parse_playlist(
            &text,
            local_file.to_string_lossy().as_ref(),
            Origin::Local,
            false,
        )?);
    }

    let local_dir = settings.resolve(&settings.local_dir);
    if local_dir.is_dir() {
        let mut paths: Vec<PathBuf> = std::fs::read_dir(&local_dir)?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.is_file())
            .filter(|path| {
                path.extension()
                    .and_then(|ext| ext.to_str())
                    .map(|ext| matches!(ext.to_ascii_lowercase().as_str(), "txt" | "m3u" | "m3u8"))
                    .unwrap_or(false)
            })
            .collect();
        paths.sort();

        for path in paths {
            let text = std::fs::read_to_string(&path)
                .with_context(|| format!("failed to read {}", path.display()))?;
            items.extend(parse_playlist(
                &text,
                path.to_string_lossy().as_ref(),
                Origin::Local,
                false,
            )?);
        }
    }

    Ok(items)
}

pub fn parse_playlist(
    text: &str,
    source_name: &str,
    origin: Origin,
    whitelist: bool,
) -> Result<Vec<ParsedChannel>> {
    if text.contains("#EXTM3U") || text.contains("#EXTINF") {
        Ok(parse_m3u(text, origin, whitelist))
    } else {
        Ok(parse_txt(text, source_name, origin, whitelist))
    }
}

pub fn aggregate_channels(items: Vec<ParsedChannel>, urls_limit: usize) -> Vec<Channel> {
    let mut map: ChannelMap = HashMap::new();

    for item in items {
        let key = normalize_channel_key(&item.name);
        map.entry(key)
            .and_modify(|channel| channel.merge(item.clone()))
            .or_insert_with(|| Channel::new(item));
    }

    let mut channels: Vec<Channel> = map.into_values().collect();
    for channel in &mut channels {
        channel
            .streams
            .sort_by_key(|stream| stream.origin.priority());
        channel.streams.truncate(urls_limit);
    }
    channels.sort_by(|a, b| {
        a.order
            .cmp(&b.order)
            .then_with(|| a.group.cmp(&b.group))
            .then_with(|| a.name.cmp(&b.name))
    });
    channels
}

fn parse_txt(
    text: &str,
    _source_name: &str,
    origin: Origin,
    whitelist: bool,
) -> Vec<ParsedChannel> {
    let mut group: Option<String> = None;
    let mut order = 0;
    let mut items = Vec::new();

    for raw_line in text.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        if let Some((name, marker)) = line.split_once(',') {
            if marker.trim().eq_ignore_ascii_case("#genre#") {
                group = Some(name.trim().to_string());
                continue;
            }
        }

        let parsed = parse_txt_channel_line(line, group.clone(), origin, whitelist, order);
        if let Some(item) = parsed {
            items.push(item);
            order += 1;
            continue;
        }

        // Guovin templates may list bare channel names under a group.
        if !line.contains(',') && !looks_like_stream_url(line) {
            items.push(ParsedChannel {
                name: line.to_string(),
                group: group.clone(),
                tvg_id: None,
                logo: None,
                stream: None,
                order: order_for_origin(origin, order),
            });
            order += 1;
        }
    }

    items
}

fn parse_txt_channel_line(
    line: &str,
    group: Option<String>,
    origin: Origin,
    whitelist: bool,
    order: usize,
) -> Option<ParsedChannel> {
    let (name, rest) = line.split_once(',')?;
    let name = name.trim();
    let rest = rest.trim();
    if name.is_empty() || !looks_like_stream_url(rest) {
        return None;
    }

    Some(ParsedChannel {
        name: name.to_string(),
        group,
        tvg_id: None,
        logo: None,
        stream: Some(Stream {
            url: rest.to_string(),
            origin,
            whitelist,
        }),
        order: order_for_origin(origin, order),
    })
}

fn parse_m3u(text: &str, origin: Origin, whitelist: bool) -> Vec<ParsedChannel> {
    let mut items = Vec::new();
    let mut pending: Option<M3uInfo> = None;
    let mut order = 0;

    for raw_line in text.lines() {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }

        if line.starts_with("#EXTINF") {
            pending = Some(parse_extinf(line));
            continue;
        }
        if line.starts_with('#') {
            continue;
        }
        if looks_like_stream_url(line) {
            let info = pending.take().unwrap_or_default();
            let Some(name) = info.name.filter(|name| !name.trim().is_empty()) else {
                continue;
            };
            items.push(ParsedChannel {
                name,
                group: info.group,
                tvg_id: info.tvg_id,
                logo: info.logo,
                stream: Some(Stream {
                    url: line.to_string(),
                    origin,
                    whitelist,
                }),
                order: order_for_origin(origin, order),
            });
            order += 1;
        }
    }

    items
}

#[derive(Debug, Default)]
struct M3uInfo {
    name: Option<String>,
    group: Option<String>,
    tvg_id: Option<String>,
    logo: Option<String>,
}

fn parse_extinf(line: &str) -> M3uInfo {
    let mut info = M3uInfo {
        name: line
            .rsplit_once(',')
            .map(|(_, name)| name.trim().to_string())
            .filter(|name| !name.is_empty()),
        ..M3uInfo::default()
    };

    info.group = attr_value(line, "group-title");
    info.tvg_id = attr_value(line, "tvg-id").or_else(|| attr_value(line, "tvg-name"));
    info.logo = attr_value(line, "tvg-logo");
    if info.name.is_none() {
        info.name = attr_value(line, "tvg-name");
    }

    info
}

fn attr_value(line: &str, name: &str) -> Option<String> {
    let needle = format!("{name}=\"");
    let start = line.find(&needle)? + needle.len();
    let end = line[start..].find('"')? + start;
    Some(line[start..end].trim().to_string()).filter(|value| !value.is_empty())
}

fn looks_like_stream_url(value: &str) -> bool {
    let value = value.trim();
    value.starts_with("http://")
        || value.starts_with("https://")
        || value.starts_with("rtmp://")
        || value.starts_with("rtsp://")
        || value.starts_with("udp://")
        || value.starts_with("rtp://")
        || value.starts_with("file://")
}

fn normalize_channel_key(name: &str) -> String {
    name.chars()
        .filter(|ch| !ch.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect()
}

fn order_for_origin(origin: Origin, order: usize) -> usize {
    let offset = match origin {
        Origin::Template => 0,
        Origin::Local => 1_000_000,
        Origin::SubscribeWhitelist => 2_000_000,
        Origin::Subscribe => 3_000_000,
    };
    offset + order
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_txt_groups_and_urls() {
        let items = parse_txt(
            "央视,#genre#\nCCTV-1,http://a/cctv1.m3u8\nCCTV-2\n",
            "test",
            Origin::Local,
            false,
        );

        assert_eq!(items.len(), 2);
        assert_eq!(items[0].group.as_deref(), Some("央视"));
        assert_eq!(items[0].stream.as_ref().unwrap().url, "http://a/cctv1.m3u8");
        assert!(items[1].stream.is_none());
    }

    #[test]
    fn parses_m3u_extinf() {
        let items = parse_m3u(
            r#"#EXTM3U
#EXTINF:-1 tvg-id="cctv1" tvg-logo="http://logo" group-title="央视",CCTV-1
http://a/cctv1.m3u8
"#,
            Origin::Subscribe,
            false,
        );

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].name, "CCTV-1");
        assert_eq!(items[0].tvg_id.as_deref(), Some("cctv1"));
        assert_eq!(items[0].logo.as_deref(), Some("http://logo"));
    }

    #[test]
    fn aggregates_and_limits_urls() {
        let items = vec![
            ParsedChannel {
                name: "CCTV-1".into(),
                group: Some("央视".into()),
                tvg_id: None,
                logo: None,
                stream: None,
                order: 0,
            },
            ParsedChannel {
                name: "CCTV-1".into(),
                group: None,
                tvg_id: None,
                logo: None,
                stream: Some(Stream {
                    url: "http://a".into(),
                    origin: Origin::Subscribe,
                    whitelist: false,
                }),
                order: 10,
            },
        ];

        let channels = aggregate_channels(items, 1);
        assert_eq!(channels.len(), 1);
        assert_eq!(channels[0].group.as_deref(), Some("央视"));
        assert_eq!(channels[0].streams.len(), 1);
    }
}
