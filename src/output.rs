use std::fs;
use std::io::Write;
use std::path::Path;

use anyhow::{Context, Result};

use crate::models::Channel;

pub fn write_txt(path: &Path, channels: &[Channel]) -> Result<()> {
    ensure_parent(path)?;
    let mut file =
        fs::File::create(path).with_context(|| format!("failed to create {}", path.display()))?;

    let mut last_group: Option<&str> = None;
    for channel in channels
        .iter()
        .filter(|channel| !channel.streams.is_empty())
    {
        let group = channel.group.as_deref().unwrap_or("未分组");
        if last_group != Some(group) {
            writeln!(file, "{group},#genre#")?;
            last_group = Some(group);
        }
        for stream in &channel.streams {
            writeln!(file, "{},{}", channel.name, stream.url)?;
        }
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
