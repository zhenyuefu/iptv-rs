//! Publish network-specific views of the same alias-merged channel catalogue.
use super::*;
use crate::{
    config::Settings,
    models::{Channel, ParsedChannel},
    output::render_m3u,
    playlist::{apply_output_preferences, limit_channel_streams},
    service::generated_settings,
};
use std::io::Write;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Profile {
    name: String,
    source_order: Vec<String>,
    relay_sources: Vec<String>,
    relay_base: Url,
    epg_url: Option<Url>,
}

pub(super) fn validate(config: &Config) -> Result<()> {
    let mut names = HashSet::new();
    for p in &config.profiles {
        ensure!(
            !p.name.is_empty()
                && p.name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
                && names.insert(&p.name),
            "invalid or duplicate profile name"
        );
        let mut ids = HashSet::new();
        ensure!(
            !p.source_order.is_empty()
                && p.source_order
                    .iter()
                    .all(|id| ids.insert(id) && config.sources.iter().any(|s| s.id == *id))
                && p.relay_sources.iter().all(|id| ids.contains(id)),
            "invalid source selection for {}",
            p.name
        );
        ensure!(
            bare_origin(&p.relay_base) && p.epg_url.as_ref().is_none_or(http_url),
            "invalid public URL for {}",
            p.name
        );
    }
    Ok(())
}

type ChannelKeys = HashMap<String, HashMap<String, String>>;

impl RunningRelay {
    // Capture the upstream name before aliasing. The media relay uses this same key.
    pub fn profile_keys(&self, rows: &[ParsedChannel]) -> ChannelKeys {
        let relayed: HashSet<_> = self
            .profiles
            .iter()
            .flat_map(|p| &p.relay_sources)
            .collect();
        let mut keys = ChannelKeys::new();
        if relayed.is_empty() {
            return keys;
        }
        for row in rows {
            let Some(stream) = &row.stream else { continue };
            let Some(id) = stream
                .iptv_source
                .as_ref()
                .filter(|id| relayed.contains(id))
            else {
                continue;
            };
            if let Ok(url) = Url::parse(&stream.url) {
                keys.entry(id.clone())
                    .or_default()
                    .entry(stream.url.clone())
                    .or_insert_with(|| channel_key(&row.name, &url));
            }
        }
        keys
    }

    pub fn export_profiles(
        &self,
        settings: &Settings,
        channels: &[Channel],
        keys: &ChannelKeys,
    ) -> Result<()> {
        let mut failures = Vec::new();
        for profile in &self.profiles {
            if let Err(error) = self.publish(profile, settings, channels, keys) {
                failures.push(format!("{}: {error:#}", profile.name));
            }
        }
        ensure!(
            failures.is_empty(),
            "playlist profiles: {}",
            failures.join(", ")
        );
        Ok(())
    }

    fn publish(
        &self,
        profile: &Profile,
        settings: &Settings,
        channels: &[Channel],
        keys: &ChannelKeys,
    ) -> Result<()> {
        let dir = settings.root.join("output");
        std::fs::create_dir_all(&dir)?;
        let path = dir.join(format!("{}.m3u", profile.name));
        ensure!(
            path != settings.resolve(&settings.final_file).with_extension("m3u"),
            "profile would replace the main playlist"
        );
        let mut catalogs = HashMap::new();
        for id in &profile.relay_sources {
            let idx = self.relay.ids[id];
            let catalog = self
                .runtime
                .block_on(self.relay.catalog(idx))
                .map_err(|_| anyhow::anyhow!("relay catalogue unavailable for {id}"))?;
            catalogs.insert(
                id.as_str(),
                (catalog, self.relay.sources[idx].format.suffix()),
            );
        }
        let mut output = channels.to_vec();
        let prefs = generated_settings(settings, &profile.source_order);
        let mut seen = HashSet::new();
        for channel in &mut output {
            channel.streams.retain(|s| {
                s.iptv_source
                    .as_ref()
                    .is_some_and(|id| profile.source_order.contains(id))
            });
        }
        apply_output_preferences(&mut output, &prefs);
        // Sort and limit using original URLs, before substituting any relay URLs.
        for stream in output.iter_mut().flat_map(|c| &mut c.streams) {
            let id = stream
                .iptv_source
                .as_deref()
                .expect("profile streams are labelled");
            seen.insert(id.to_owned());
            if let Some((catalog, suffix)) = catalogs.get(id) {
                let key = keys
                    .get(id)
                    .and_then(|urls| urls.get(&stream.url))
                    .context("missing original channel key")?;
                // An upstream may advertise its LAN address to the aggregator and its
                // loopback address to the relay. Only emit keys its own catalogue accepts.
                ensure!(
                    catalog.urls.contains_key(key),
                    "channel missing from {id} relay catalogue"
                );
                stream.url = format!(
                    "{}/live/{id}/{key}{suffix}",
                    profile.relay_base.origin().ascii_serialization()
                );
            }
        }
        ensure!(
            seen.len() == profile.source_order.len(),
            "one or more sources unavailable"
        );
        limit_channel_streams(&mut output, &prefs);
        let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
        render_m3u(tmp.as_file_mut(), &output, profile.epg_url.as_ref())?;
        tmp.flush()?;
        tmp.persist(path)?;
        Ok(())
    }
}
