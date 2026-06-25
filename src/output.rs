use std::fs;
use std::io::Write;
use std::path::Path;

use anyhow::{Context, Result};
use chrono::Utc;

use crate::config::Settings;
use crate::models::{Channel, Origin, Stream};

pub fn write_txt(path: &Path, channels: &[Channel], settings: &Settings) -> Result<()> {
    ensure_parent(path)?;
    let mut file =
        fs::File::create(path).with_context(|| format!("failed to create {}", path.display()))?;

    if settings.open_update_time && settings.update_time_position.eq_ignore_ascii_case("top") {
        writeln!(file, "更新时间,#genre#")?;
        writeln!(file, "更新时间,{}", update_time())?;
    }

    let mut last_group: Option<&str> = None;
    for channel in channels {
        if channel.streams.is_empty() && !settings.open_empty_category {
            continue;
        }

        let group = if channel.streams.is_empty() {
            "无结果频道"
        } else {
            channel.group.as_deref().unwrap_or("未分组")
        };
        if last_group != Some(group) {
            writeln!(file, "{group},#genre#")?;
            last_group = Some(group);
        }
        if channel.streams.is_empty() {
            writeln!(file, "{},", channel.name)?;
            continue;
        }
        for stream in &channel.streams {
            writeln!(file, "{},{}", channel.name, decorated_url(stream, settings))?;
        }
    }

    if settings.open_update_time && settings.update_time_position.eq_ignore_ascii_case("bottom") {
        writeln!(file, "更新时间,#genre#")?;
        writeln!(file, "更新时间,{}", update_time())?;
    }

    Ok(())
}

pub fn write_m3u(path: &Path, channels: &[Channel]) -> Result<()> {
    ensure_parent(path)?;
    let mut file =
        fs::File::create(path).with_context(|| format!("failed to create {}", path.display()))?;
    writeln!(file, "#EXTM3U")?;

    for channel in channels
        .iter()
        .filter(|channel| !channel.streams.is_empty())
    {
        let group = channel.group.as_deref().unwrap_or("未分组");
        let tvg_id = channel.tvg_id.as_deref().unwrap_or(&channel.name);
        let logo = channel.logo.as_deref().unwrap_or_default();

        for stream in &channel.streams {
            writeln!(
                file,
                "#EXTINF:-1 tvg-id=\"{}\" tvg-name=\"{}\" tvg-logo=\"{}\" group-title=\"{}\",{}",
                escape_attr(tvg_id),
                escape_attr(&channel.name),
                escape_attr(logo),
                escape_attr(group),
                channel.name
            )?;
            writeln!(file, "{}", stream.url)?;
        }
    }

    Ok(())
}

fn ensure_parent(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create output directory {}", parent.display()))?;
    }
    Ok(())
}

fn escape_attr(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn decorated_url(stream: &Stream, settings: &Settings) -> String {
    if !settings.open_url_info {
        return stream.url.clone();
    }

    format!(
        "{}${}/{}",
        stream.url,
        origin_label(stream.origin, stream.whitelist),
        stream.ipv_type.as_str()
    )
}

fn origin_label(origin: Origin, whitelist: bool) -> &'static str {
    if whitelist {
        return "whitelist";
    }
    match origin {
        Origin::Template | Origin::Local => "local",
        Origin::Subscribe | Origin::SubscribeWhitelist => "subscribe",
    }
}

fn update_time() -> String {
    Utc::now().format("%Y-%m-%d %H:%M:%S UTC").to_string()
}
