mod config;
mod epg;
mod fetch;
mod logo;
mod metadata;
mod models;
mod output;
mod playlist;
mod rules;
mod service;
mod source_list;

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

use crate::config::Settings;
use crate::epg::EpgAggregator;
use crate::fetch::HttpFetcher;
use crate::logo::LogoResolver;
use crate::metadata::channels_to_metadata_bytes;
use crate::output::{write_m3u, write_txt};
use crate::playlist::{
    aggregate_channels, apply_output_preferences, limit_channel_streams, load_local_sources,
    parse_playlist, source_iptv_allowed,
};
use crate::rules::{AliasMatcher, FilterRules};
use crate::source_list::{SourceSection, disable_source_entry, parse_source_list_file};

#[derive(Debug, Parser)]
#[command(name = "iptv-rs")]
#[command(about = "IPTV live-source and EPG aggregator")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Aggregate live sources and EPG into output files.
    Update {
        /// Path to config/config.ini.
        #[arg(short, long, default_value = "config/config.ini")]
        config: PathBuf,
    },
    /// Run the updater and expose generated output files over HTTP.
    Serve {
        /// Path to config/config.ini.
        #[arg(short, long, default_value = "config/config.ini")]
        config: PathBuf,
    },
}

fn main() -> Result<()> {
    init_container_runtime()?;

    let cli = Cli::parse_from(args_with_config_env());

    match cli.command {
        Command::Update { config } => run_update(config),
        Command::Serve { config } => service::serve(config, run_update),
    }
}

fn args_with_config_env() -> Vec<OsString> {
    let mut args: Vec<OsString> = std::env::args_os().collect();
    let Some(config_path) = std::env::var_os("CONFIG_PATH") else {
        return args;
    };
    if args_has_config_flag(&args) {
        return args;
    }
    let Some(command_index) = args
        .iter()
        .position(|arg| arg == OsStr::new("update") || arg == OsStr::new("serve"))
    else {
        return args;
    };

    args.insert(command_index + 1, OsString::from("--config"));
    args.insert(command_index + 2, config_path);
    args
}

fn args_has_config_flag(args: &[OsString]) -> bool {
    args.iter().any(|arg| {
        arg == OsStr::new("-c")
            || arg == OsStr::new("--config")
            || arg
                .to_str()
                .is_some_and(|value| value.starts_with("--config="))
    })
}

fn init_container_runtime() -> Result<()> {
    let Some(default_config_dir) = std::env::var_os("IPTV_RS_DEFAULT_CONFIG_DIR") else {
        return Ok(());
    };

    let workdir = std::env::var_os("APP_WORKDIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/iptv-rs"));
    let config_dir = workdir.join("config");
    let output_dir = workdir.join("output");

    std::fs::create_dir_all(&config_dir)
        .with_context(|| format!("failed to create {}", config_dir.display()))?;
    std::fs::create_dir_all(&output_dir)
        .with_context(|| format!("failed to create {}", output_dir.display()))?;

    copy_missing_config_entries(Path::new(&default_config_dir), &config_dir)
}

fn copy_missing_config_entries(source_dir: &Path, target_dir: &Path) -> Result<()> {
    if !source_dir.exists() {
        return Ok(());
    }

    for entry in std::fs::read_dir(source_dir)
        .with_context(|| format!("failed to read {}", source_dir.display()))?
    {
        let entry = entry?;
        let source = entry.path();
        let target = target_dir.join(entry.file_name());
        copy_missing_config_entry(&source, &target)?;
    }

    Ok(())
}

fn copy_missing_config_entry(source: &Path, target: &Path) -> Result<()> {
    if target.exists() {
        return Ok(());
    }

    let metadata = std::fs::metadata(source)
        .with_context(|| format!("failed to inspect {}", source.display()))?;
    if metadata.is_dir() {
        std::fs::create_dir_all(target)
            .with_context(|| format!("failed to create {}", target.display()))?;
        for entry in std::fs::read_dir(source)
            .with_context(|| format!("failed to read {}", source.display()))?
        {
            let entry = entry?;
            copy_missing_config_entry(&entry.path(), &target.join(entry.file_name()))?;
        }
    } else if metadata.is_file() {
        std::fs::copy(source, target).with_context(|| {
            format!(
                "failed to copy {} to {}",
                source.display(),
                target.display()
            )
        })?;
    }

    Ok(())
}

fn run_update(config_path: PathBuf) -> Result<()> {
    let settings = Settings::from_file(&config_path)?;
    if !settings.open_update {
        eprintln!("open_update is false; skipping update");
        return Ok(());
    }

    let fetcher = HttpFetcher::new(&settings)?;
    let aliases = AliasMatcher::from_file(&settings.resolve(&settings.alias_file))?;
    let rules = FilterRules::from_files(
        &settings.resolve(&settings.blacklist_file),
        &settings.resolve(&settings.whitelist_file),
    )?;

    let mut parsed_sources = Vec::new();
    let template_text = std::fs::read_to_string(settings.resolve(&settings.source_file))?;
    let template = parse_playlist(
        &template_text,
        settings.source_file.to_string_lossy().as_ref(),
        models::Origin::Template,
        false,
        0,
        None,
        false,
    )?;
    parsed_sources.extend(template);

    if settings.open_local {
        parsed_sources.extend(load_local_sources(&settings)?);
    }

    if settings.open_history {
        let final_path = settings.resolve(&settings.final_file);
        if final_path.exists() {
            let text = std::fs::read_to_string(&final_path)?;
            parsed_sources.extend(parse_playlist(
                &text,
                final_path.to_string_lossy().as_ref(),
                models::Origin::Local,
                false,
                0,
                None,
                false,
            )?);
        }
    }

    let subscribe_file = settings.resolve(&settings.subscribe_file);
    if settings.open_subscribe && subscribe_file.exists() {
        let subscriptions = parse_source_list_file(&subscribe_file)?;
        for (index, source) in subscriptions.into_iter().enumerate() {
            if !source.enabled || source.url.trim().is_empty() {
                continue;
            }
            if !source_iptv_allowed(&source, &settings) {
                continue;
            }

            let body = match fetcher.fetch_text(&source) {
                Ok(body) => body,
                Err(err) => {
                    eprintln!("failed to fetch subscription {}: {err:#}", source.url);
                    if settings.open_auto_disable_source {
                        let _ = disable_source_entry(&subscribe_file, &source.url);
                    }
                    continue;
                }
            };
            if body.trim().is_empty() {
                if settings.open_auto_disable_source {
                    let _ = disable_source_entry(&subscribe_file, &source.url);
                }
                continue;
            }
            let origin = match source.section {
                SourceSection::Whitelist => models::Origin::SubscribeWhitelist,
                SourceSection::Default => models::Origin::Subscribe,
            };
            let parsed = parse_playlist(
                &body,
                &source.url,
                origin,
                source.is_whitelist(),
                index,
                source.iptv_source.as_deref(),
                source.iptv_restricted,
            )?;
            if parsed.is_empty() && settings.open_auto_disable_source {
                let _ = disable_source_entry(&subscribe_file, &source.url);
            }
            parsed_sources.extend(parsed);
        }
    }

    let mut channels = aggregate_channels(parsed_sources, &settings, &aliases, &rules);
    let logo_resolver = LogoResolver::new(&settings);
    logo_resolver.apply(&mut channels);
    let metadata_channels = channels.clone();
    apply_output_preferences(&mut channels, &settings);
    limit_channel_streams(&mut channels, &settings);

    let final_path = settings.resolve(&settings.final_file);
    write_txt(&final_path, &channels, &settings)?;

    if settings.open_m3u_result {
        let m3u_path = final_path.with_extension("m3u");
        write_m3u(&m3u_path, &channels)?;
    }
    std::fs::write(
        final_path.with_extension("metadata.tsv"),
        channels_to_metadata_bytes(&metadata_channels),
    )?;

    let epg_file = settings.resolve(&settings.epg_file);
    if settings.open_epg && epg_file.exists() {
        let mut aggregator = EpgAggregator::default();
        let epg_sources = parse_source_list_file(&epg_file)?;
        for source in epg_sources {
            if !source.enabled || source.url.trim().is_empty() {
                continue;
            }
            let bytes = match fetcher.fetch_bytes(&source) {
                Ok(bytes) => bytes,
                Err(err) => {
                    eprintln!("failed to fetch EPG {}: {err:#}", source.url);
                    if settings.open_auto_disable_source {
                        let _ = disable_source_entry(&epg_file, &source.url);
                    }
                    continue;
                }
            };
            if bytes.is_empty() {
                if settings.open_auto_disable_source {
                    let _ = disable_source_entry(&epg_file, &source.url);
                }
                continue;
            }
            aggregator.add_document(&source.url, &bytes)?;
        }
        aggregator.retain_for_channels(&channels);
        aggregator.write_to(&settings.resolve(&settings.epg_output_file))?;
    }

    Ok(())
}
