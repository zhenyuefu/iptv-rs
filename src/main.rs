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

use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};

use crate::config::Settings;
use crate::epg::EpgAggregator;
use crate::fetch::HttpFetcher;
use crate::logo::LogoResolver;
use crate::metadata::channels_to_metadata_bytes;
use crate::output::{write_m3u, write_txt};
use crate::playlist::{
    aggregate_channels, limit_channel_streams, load_local_sources, parse_playlist,
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
    let cli = Cli::parse();

    match cli.command {
        Command::Update { config } => run_update(config),
        Command::Serve { config } => service::serve(config, run_update),
    }
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
