use std::path::{Path, PathBuf};

use crate::config::Settings;
use crate::models::Channel;

pub struct LogoResolver {
    root: PathBuf,
    logo_dir: PathBuf,
    local_base_url: Option<String>,
    remote_base_url: Option<String>,
    logo_type: String,
}

impl LogoResolver {
    pub fn new(settings: &Settings) -> Self {
        Self {
            root: settings.root.clone(),
            logo_dir: settings.resolve(&settings.logo_dir),
            local_base_url: settings.local_logo_base_url.clone(),
            remote_base_url: settings.logo_url.clone(),
            logo_type: settings.logo_type.clone(),
        }
    }

    pub fn apply(&self, channels: &mut [Channel]) {
        for channel in channels {
            if let Some(local_logo) = self.find_local_logo(channel) {
                channel.logo = Some(local_logo);
                continue;
            }

            if channel.logo.is_some() {
                continue;
            }

            if let Some(base) = &self.remote_base_url {
                let file_name = format!("{}.{}", sanitize_file_stem(&channel.name), self.logo_type);
                channel.logo = Some(format!("{}/{}", base.trim_end_matches('/'), file_name));
            }
        }
    }

    fn find_local_logo(&self, channel: &Channel) -> Option<String> {
        let mut stems = vec![channel.name.as_str()];
        if let Some(tvg_id) = &channel.tvg_id {
            stems.push(tvg_id);
        }

        let mut candidates = Vec::new();
        let extensions = ["png", "jpg", "jpeg", "webp", "svg"];
        for stem in stems {
            candidates.push(format!("{}.{}", stem, self.logo_type));
            candidates.push(format!("{}.{}", sanitize_file_stem(stem), self.logo_type));
            for ext in extensions {
                candidates.push(format!("{stem}.{ext}"));
                candidates.push(format!("{}.{ext}", sanitize_file_stem(stem)));
            }
        }

        candidates.sort();
        candidates.dedup();

        candidates
            .into_iter()
            .map(|name| self.logo_dir.join(name))
            .find(|path| path.is_file())
            .map(|path| self.local_logo_reference(&path))
    }

    fn local_logo_reference(&self, path: &Path) -> String {
        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        if let Some(base) = &self.local_base_url {
            return format!("{}/{}", base.trim_end_matches('/'), file_name);
        }

        path.strip_prefix(&self.root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/")
    }
}

fn sanitize_file_stem(value: &str) -> String {
    let mut out = String::new();
    let mut last_was_dash = false;

    for ch in value.trim().chars() {
        if ch.is_alphanumeric() || matches!(ch, '-' | '_' | '+' | '.') {
            out.push(ch);
            last_was_dash = false;
        } else if !last_was_dash {
            out.push('-');
            last_was_dash = true;
        }
    }

    out.trim_matches('-').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitizes_file_stems() {
        assert_eq!(sanitize_file_stem("CCTV 5+"), "CCTV-5+");
        assert_eq!(sanitize_file_stem("湖南卫视"), "湖南卫视");
    }
}
