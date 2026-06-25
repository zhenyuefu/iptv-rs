mod config;
mod epg;
mod fetch;
mod logo;
mod models;
mod output;
mod playlist;
mod service;
mod source_list;

use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};

use crate::config::Settings;
use crate::epg::EpgAggregator;
use crate::fetch::HttpFetcher;
use crate::logo::LogoResolver;
use crate::output::{write_m3u, write_txt};
use crate::playlist::{aggregate_channels, load_local_sources, parse_playlist};
use crate::source_list::{SourceSection, parse_source_list_file};

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
    let fetcher = HttpFetcher::new(&settings)?;

    let mut parsed_sources = Vec::new();
    let template_text = std::fs::read_to_string(settings.resolve(&settings.source_file))?;
    let template = parse_playlist(
        &template_text,
        settings.source_file.to_string_lossy().as_ref(),
        models::Origin::Template,
        false,
    )?;
    parsed_sources.extend(template);

    if settings.open_local {
        parsed_sources.extend(load_local_sources(&settings)?);
    }

    let subscribe_file = settings.resolve(&settings.subscribe_file);
    if settings.open_subscribe && subscribe_file.exists() {
        let subscriptions = parse_source_list_file(&subscribe_file)?;
        for source in subscriptions {
            if !source.enabled || source.url.trim().is_empty() {
                continue;
            }

            let body = fetcher.fetch_text(&source)?;
            let origin = match source.section {
                SourceSection::Whitelist => models::Origin::SubscribeWhitelist,
                SourceSection::Default => models::Origin::Subscribe,
            };
            parsed_sources.extend(parse_playlist(
                &body,
                &source.url,
                origin,
                source.is_whitelist(),
            )?);
        }
    }

    let mut channels = aggregate_channels(parsed_sources, settings.urls_limit);
    let logo_resolver = LogoResolver::new(&settings);
    logo_resolver.apply(&mut channels);

    let final_path = settings.resolve(&settings.final_file);
    write_txt(&final_path, &channels)?;

    if settings.open_m3u_result {
        let m3u_path = final_path.with_extension("m3u");
        write_m3u(&m3u_path, &channels)?;
    }

    let epg_file = settings.resolve(&settings.epg_file);
    if settings.open_epg && epg_file.exists() {
        let mut aggregator = EpgAggregator::default();
        let epg_sources = parse_source_list_file(&epg_file)?;
        for source in epg_sources {
            if !source.enabled || source.url.trim().is_empty() {
                continue;
            }
            let bytes = fetcher.fetch_bytes(&source)?;
            aggregator.add_document(&source.url, &bytes)?;
        }
        aggregator.retain_for_channels(&channels);
        aggregator.write_to(&settings.resolve(&settings.epg_output_file))?;
    }

    Ok(())
}
