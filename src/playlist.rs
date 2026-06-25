use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use anyhow::{Context, Result};
use url::Url;

use crate::config::Settings;
use crate::models::{Channel, ChannelMap, IpvType, Origin, ParsedChannel, Stream};
use crate::rules::{AliasMatcher, FilterRules};

pub fn load_local_sources(settings: &Settings) -> Result<Vec<ParsedChannel>> {
    let mut items = Vec::new();
    let local_file = settings.resolve(&settings.local_file);
    if local_file.exists() {
        let text = std::fs::read_to_string(&local_file)
            .with_context(|| format!("failed to read {}", local_file.display()))?;
        let source_label = source_label_from_path(&local_file);
        items.extend(parse_playlist(
            &text,
            local_file.to_string_lossy().as_ref(),
            Origin::Local,
            false,
            0,
            source_label.as_deref(),
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

        for (index, path) in paths.into_iter().enumerate() {
            let text = std::fs::read_to_string(&path)
                .with_context(|| format!("failed to read {}", path.display()))?;
            items.extend(parse_playlist(
                &text,
                path.to_string_lossy().as_ref(),
                Origin::Local,
                false,
                index + 1,
                source_label_from_path(&path).as_deref(),
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
    source_order: usize,
    iptv_source: Option<&str>,
) -> Result<Vec<ParsedChannel>> {
    if text.contains("#EXTM3U") || text.contains("#EXTINF") {
        Ok(parse_m3u(
            text,
            origin,
            whitelist,
            source_order,
            iptv_source,
        ))
    } else {
        Ok(parse_txt(
            text,
            source_name,
            origin,
            whitelist,
            source_order,
            iptv_source,
        ))
    }
}

pub fn aggregate_channels(
    items: Vec<ParsedChannel>,
    settings: &Settings,
    aliases: &AliasMatcher,
    rules: &FilterRules,
) -> Vec<Channel> {
    let template_keys: HashSet<String> = items
        .iter()
        .filter(|item| item.order < 1_000_000)
        .map(|item| normalize_channel_key(&aliases.primary_name(&item.name)))
        .collect();

    let mut map: ChannelMap = HashMap::new();
    for mut item in items {
        let primary_name = aliases.primary_name(&item.name);
        let key = normalize_channel_key(&primary_name);
        let is_template_match = template_keys.contains(&key);
        let is_template = item.order < 1_000_000;

        if !is_template && !is_template_match && !settings.open_unmatch_category {
            continue;
        }

        if let Some(stream) = &mut item.stream {
            stream.ipv_type = infer_ipv_type(&stream.url);
            stream.whitelist = stream.whitelist || rules.is_whitelisted(&stream.url, &primary_name);
            if !stream.whitelist && rules.is_blacklisted(&stream.url) {
                continue;
            }
            if !matches_ipv_type(settings, stream.ipv_type) {
                continue;
            }
        }

        item.name = primary_name;
        if !is_template_match && !is_template && settings.open_unmatch_category {
            item.group = Some("未匹配频道".to_string());
        }

        map.entry(key)
            .and_modify(|channel| channel.merge(item.clone()))
            .or_insert_with(|| Channel::new(item));
    }

    inject_whitelist_urls(&mut map, rules);

    let mut channels: Vec<Channel> = map.into_values().collect();
    for channel in &mut channels {
        sort_channel_streams(channel, settings);
    }

    if !settings.open_empty_category {
        channels.retain(|channel| !channel.streams.is_empty());
    }

    channels.sort_by(|a, b| {
        a.order
            .cmp(&b.order)
            .then_with(|| a.group.cmp(&b.group))
            .then_with(|| a.name.cmp(&b.name))
    });
    channels
}

pub fn sort_channel_streams(channel: &mut Channel, settings: &Settings) {
    channel.streams.sort_by(|a, b| {
        stream_sort_key(a, settings)
            .cmp(&stream_sort_key(b, settings))
            .then_with(|| a.source_order.cmp(&b.source_order))
            .then_with(|| a.url.cmp(&b.url))
    });
}

pub fn limit_channel_streams(channels: &mut [Channel], settings: &Settings) {
    for channel in channels {
        channel.streams.truncate(settings.urls_limit);
    }
}

fn inject_whitelist_urls(map: &mut ChannelMap, rules: &FilterRules) {
    for channel in map.values_mut() {
        let mut urls: HashSet<String> = channel
            .streams
            .iter()
            .map(|stream| stream.url.clone())
            .collect();

        for url in rules.whitelist_urls_for(&channel.name) {
            if urls.insert(url.clone()) {
                channel.streams.push(Stream {
                    ipv_type: infer_ipv_type(&url),
                    url,
                    origin: Origin::SubscribeWhitelist,
                    whitelist: true,
                    source_order: 0,
                    iptv_source: None,
                });
            }
        }
    }
}

fn parse_txt(
    text: &str,
    _source_name: &str,
    origin: Origin,
    whitelist: bool,
    source_order: usize,
    iptv_source: Option<&str>,
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

        let parsed = parse_txt_channel_line(
            line,
            group.clone(),
            origin,
            whitelist,
            order,
            source_order,
            iptv_source,
        );
        if let Some(item) = parsed {
            items.push(item);
            order += 1;
            continue;
        }

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
    source_order: usize,
    iptv_source: Option<&str>,
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
            source_order,
            iptv_source: iptv_source.map(ToString::to_string),
            ipv_type: infer_ipv_type(rest),
        }),
        order: order_for_origin(origin, order),
    })
}

fn parse_m3u(
    text: &str,
    origin: Origin,
    whitelist: bool,
    source_order: usize,
    iptv_source: Option<&str>,
) -> Vec<ParsedChannel> {
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
                    source_order,
                    iptv_source: iptv_source.map(ToString::to_string),
                    ipv_type: infer_ipv_type(line),
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

fn matches_ipv_type(settings: &Settings, ipv_type: IpvType) -> bool {
    matches!(settings.ipv_type.as_str(), "all" | "")
        || settings.ipv_type == ipv_type.as_str()
        || (ipv_type == IpvType::Unknown && settings.ipv_type == "ipv4")
}

fn stream_sort_key(stream: &Stream, settings: &Settings) -> (usize, usize, usize, usize) {
    (
        iptv_source_rank(stream, settings),
        usize::from(!stream.whitelist),
        origin_rank(stream.origin, settings),
        ipv_rank(stream.ipv_type, settings),
    )
}

fn iptv_source_rank(stream: &Stream, settings: &Settings) -> usize {
    let Some(source) = &stream.iptv_source else {
        return settings.iptv_source_prefer.len();
    };
    let source = source.to_ascii_lowercase();
    settings
        .iptv_source_prefer
        .iter()
        .position(|prefer| prefer == &source)
        .unwrap_or(settings.iptv_source_prefer.len())
}

fn origin_rank(origin: Origin, settings: &Settings) -> usize {
    let name = match origin {
        Origin::Template | Origin::Local => "local",
        Origin::Subscribe | Origin::SubscribeWhitelist => "subscribe",
    };
    settings
        .origin_type_prefer
        .iter()
        .position(|prefer| prefer == name)
        .unwrap_or_else(|| origin.priority() + settings.origin_type_prefer.len())
}

fn ipv_rank(ipv_type: IpvType, settings: &Settings) -> usize {
    settings
        .ipv_type_prefer
        .iter()
        .position(|prefer| prefer == ipv_type.as_str())
        .unwrap_or(settings.ipv_type_prefer.len())
}

fn infer_ipv_type(url: &str) -> IpvType {
    let host = Url::parse(url)
        .ok()
        .and_then(|url| url.host_str().map(ToString::to_string))
        .or_else(|| {
            url.split_once("://").map(|(_, rest)| {
                rest.split('/')
                    .next()
                    .unwrap_or(rest)
                    .trim_matches(['[', ']'])
                    .to_string()
            })
        });
    let Some(host) = host else {
        return IpvType::Unknown;
    };
    let host = host.trim_matches(['[', ']']);
    if host.parse::<std::net::Ipv6Addr>().is_ok() {
        IpvType::Ipv6
    } else {
        IpvType::Ipv4
    }
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

fn source_label_from_path(path: &std::path::Path) -> Option<String> {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .map(str::trim)
        .filter(|stem| !stem.is_empty())
        .map(ToString::to_string)
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
            0,
            None,
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
            0,
            Some("sub-a"),
        );

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].name, "CCTV-1");
        assert_eq!(items[0].tvg_id.as_deref(), Some("cctv1"));
        assert_eq!(items[0].logo.as_deref(), Some("http://logo"));
        assert_eq!(
            items[0].stream.as_ref().unwrap().iptv_source.as_deref(),
            Some("sub-a")
        );
    }
}
