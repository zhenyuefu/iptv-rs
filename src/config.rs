use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

#[derive(Debug, Clone)]
pub struct Settings {
    pub root: PathBuf,
    pub open_update: bool,
    pub open_service: bool,
    pub open_local: bool,
    pub open_subscribe: bool,
    pub open_auto_disable_source: bool,
    pub open_history: bool,
    pub open_unmatch_category: bool,
    pub open_empty_category: bool,
    pub open_update_time: bool,
    pub open_url_info: bool,
    pub open_epg: bool,
    pub open_m3u_result: bool,
    pub update_startup: bool,
    pub update_interval: u64,
    pub update_time_position: String,
    pub nginx_http_port: u16,
    pub public_scheme: String,
    pub public_domain: String,
    pub public_port: u16,
    pub source_file: PathBuf,
    pub local_source_list_file: PathBuf,
    pub subscribe_file: PathBuf,
    pub epg_file: PathBuf,
    pub alias_file: PathBuf,
    pub blacklist_file: PathBuf,
    pub whitelist_file: PathBuf,
    pub final_file: PathBuf,
    pub epg_output_file: PathBuf,
    pub urls_limit: usize,
    pub request_timeout: u64,
    pub ipv_type: String,
    pub ipv_type_prefer: Vec<String>,
    pub origin_type_prefer: Vec<String>,
    pub iptv_source_prefer: Vec<String>,
    pub iptv_source_filter: Vec<String>,
    pub default_user_agent: String,
    pub http_proxy: Option<String>,
    pub logo_dir: PathBuf,
    pub local_logo_base_url: Option<String>,
    pub logo_url: Option<String>,
    pub logo_type: String,
}

impl Settings {
    pub fn from_file(path: &Path) -> Result<Self> {
        let root = std::env::current_dir().context("failed to discover current directory")?;
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read config file {}", path.display()))?;
        let mut values = parse_ini_settings(&text);
        apply_env_overrides(&mut values);
        let iptv_source_filter = get_list(&values, "iptv_source_filter");
        let mut iptv_source_prefer = get_list(&values, "iptv_source_prefer");
        if iptv_source_prefer.is_empty() {
            iptv_source_prefer = iptv_source_filter.clone();
        }

        Ok(Self {
            root,
            open_update: get_bool(&values, "open_update", true),
            open_service: get_bool(&values, "open_service", true),
            open_local: get_bool(&values, "open_local", true),
            open_subscribe: get_bool(&values, "open_subscribe", true),
            open_auto_disable_source: get_bool(&values, "open_auto_disable_source", true),
            open_history: get_bool(&values, "open_history", true),
            open_unmatch_category: get_bool(&values, "open_unmatch_category", false),
            open_empty_category: get_bool(&values, "open_empty_category", false),
            open_update_time: get_bool(&values, "open_update_time", true),
            open_url_info: get_bool(&values, "open_url_info", false),
            open_epg: get_bool(&values, "open_epg", true),
            open_m3u_result: get_bool(&values, "open_m3u_result", true),
            update_startup: get_bool(&values, "update_startup", true),
            update_interval: get_u64(&values, "update_interval", 12),
            update_time_position: values
                .get("update_time_position")
                .cloned()
                .unwrap_or_else(|| "top".to_string()),
            nginx_http_port: get_u16(&values, "nginx_http_port", 8080),
            public_scheme: values
                .get("public_scheme")
                .cloned()
                .unwrap_or_else(|| "http".to_string()),
            public_domain: values
                .get("public_domain")
                .cloned()
                .unwrap_or_else(|| "127.0.0.1".to_string()),
            public_port: get_u16(&values, "public_port", 80),
            source_file: get_path(&values, "source_file", "config/demo.txt"),
            local_source_list_file: get_path(
                &values,
                "local_source_list_file",
                "config/local_sources.txt",
            ),
            subscribe_file: get_path(&values, "subscribe_file", "config/subscribe.txt"),
            epg_file: get_path(&values, "epg_file", "config/epg.txt"),
            alias_file: get_path(&values, "alias_file", "config/alias.txt"),
            blacklist_file: get_path(&values, "blacklist_file", "config/blacklist.txt"),
            whitelist_file: get_path(&values, "whitelist_file", "config/whitelist.txt"),
            final_file: get_path(&values, "final_file", "output/result.txt"),
            epg_output_file: get_path(&values, "epg_output_file", "output/epg/epg.xml"),
            urls_limit: get_usize(&values, "urls_limit", 5).max(1),
            request_timeout: get_u64(&values, "request_timeout", 10).max(1),
            ipv_type: values
                .get("ipv_type")
                .map(|value| value.to_ascii_lowercase())
                .unwrap_or_else(|| "all".to_string()),
            ipv_type_prefer: get_list(&values, "ipv_type_prefer"),
            origin_type_prefer: get_list(&values, "origin_type_prefer"),
            iptv_source_prefer,
            iptv_source_filter,
            default_user_agent: values
                .get("default_user_agent")
                .cloned()
                .unwrap_or_else(|| "iptv-rs/0.1".to_string()),
            http_proxy: get_non_empty(&values, "http_proxy"),
            logo_dir: get_path(&values, "logo_dir", "config/logo"),
            local_logo_base_url: get_non_empty(&values, "local_logo_base_url"),
            logo_url: get_non_empty(&values, "logo_url"),
            logo_type: values
                .get("logo_type")
                .cloned()
                .unwrap_or_else(|| "png".to_string()),
        })
    }

    pub fn resolve(&self, path: &Path) -> PathBuf {
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.root.join(path)
        }
    }
}

fn parse_ini_settings(text: &str) -> HashMap<String, String> {
    let mut values = HashMap::new();

    for raw_line in text.lines() {
        let line = raw_line.trim();
        if line.is_empty()
            || line.starts_with('#')
            || line.starts_with(';')
            || line.starts_with('[')
        {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        values.insert(key.trim().to_ascii_lowercase(), value.trim().to_string());
    }

    values
}

fn apply_env_overrides(values: &mut HashMap<String, String>) {
    for (key, value) in std::env::vars() {
        let key = key.to_ascii_lowercase();
        if matches!(
            key.as_str(),
            "open_service"
                | "open_update"
                | "open_local"
                | "open_subscribe"
                | "open_auto_disable_source"
                | "open_history"
                | "open_unmatch_category"
                | "open_empty_category"
                | "open_update_time"
                | "open_url_info"
                | "open_epg"
                | "open_m3u_result"
                | "update_startup"
                | "update_interval"
                | "update_time_position"
                | "nginx_http_port"
                | "public_scheme"
                | "public_domain"
                | "public_port"
                | "source_file"
                | "local_source_list_file"
                | "subscribe_file"
                | "epg_file"
                | "alias_file"
                | "blacklist_file"
                | "whitelist_file"
                | "final_file"
                | "epg_output_file"
                | "urls_limit"
                | "request_timeout"
                | "ipv_type"
                | "ipv_type_prefer"
                | "origin_type_prefer"
                | "iptv_source_prefer"
                | "iptv_source_filter"
                | "default_user_agent"
                | "http_proxy"
                | "logo_dir"
                | "local_logo_base_url"
                | "logo_url"
                | "logo_type"
        ) {
            values.insert(key, value);
        }
    }
}

fn get_bool(values: &HashMap<String, String>, key: &str, default: bool) -> bool {
    values.get(key).map_or(default, |value| {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "true" | "1" | "yes" | "on"
        )
    })
}

fn get_usize(values: &HashMap<String, String>, key: &str, default: usize) -> usize {
    values
        .get(key)
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

fn get_u64(values: &HashMap<String, String>, key: &str, default: u64) -> u64 {
    values
        .get(key)
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

fn get_u16(values: &HashMap<String, String>, key: &str, default: u16) -> u16 {
    values
        .get(key)
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

fn get_path(values: &HashMap<String, String>, key: &str, default: &str) -> PathBuf {
    values
        .get(key)
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(default))
}

fn get_list(values: &HashMap<String, String>, key: &str) -> Vec<String> {
    values
        .get(key)
        .map(|value| {
            value
                .split(',')
                .map(|part| part.trim().to_ascii_lowercase())
                .filter(|part| !part.is_empty() && part != "auto")
                .collect()
        })
        .unwrap_or_default()
}

fn get_non_empty(values: &HashMap<String, String>, key: &str) -> Option<String> {
    values.get(key).and_then(|value| {
        let value = value.trim();
        (!value.is_empty()).then(|| value.to_string())
    })
}
