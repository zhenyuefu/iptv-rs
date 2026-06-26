use std::path::Path;

use anyhow::{Context, Result};

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum SourceSection {
    Default,
    Whitelist,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct SourceEntry {
    pub url: String,
    pub user_agent: Option<String>,
    pub iptv_source: Option<String>,
    pub iptv_restricted: bool,
    pub section: SourceSection,
    pub enabled: bool,
}

impl SourceEntry {
    pub fn is_whitelist(&self) -> bool {
        self.section == SourceSection::Whitelist
    }
}

pub fn parse_source_list_file(path: &Path) -> Result<Vec<SourceEntry>> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read source list {}", path.display()))?;
    Ok(parse_source_list(&text))
}

pub fn disable_source_entry(path: &Path, url: &str) -> Result<bool> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read source list {}", path.display()))?;
    let mut changed = false;
    let mut lines = Vec::new();

    for raw_line in text.lines() {
        let trimmed = raw_line.trim();
        if !changed && !trimmed.starts_with('#') {
            let candidate = parse_source_line(trimmed).url;
            if candidate == url {
                lines.push(format!("#{raw_line}"));
                changed = true;
                continue;
            }
        }
        lines.push(raw_line.to_string());
    }

    if changed {
        let mut output = lines.join("\n");
        if text.ends_with('\n') {
            output.push('\n');
        }
        std::fs::write(path, output)
            .with_context(|| format!("failed to update source list {}", path.display()))?;
    }

    Ok(changed)
}

pub fn parse_source_list(text: &str) -> Vec<SourceEntry> {
    let mut section = SourceSection::Default;
    let mut entries = Vec::new();

    for raw_line in text.lines() {
        let mut line = raw_line.trim();
        if line.is_empty() {
            continue;
        }
        if line.eq_ignore_ascii_case("[WHITELIST]") {
            section = SourceSection::Whitelist;
            continue;
        }

        let enabled = !line.starts_with('#');
        if !enabled {
            line = line.trim_start_matches('#').trim();
            if line.is_empty() || line.starts_with(' ') || !looks_like_source_line(line) {
                continue;
            }
        }
        if line.starts_with('#') || !looks_like_source_line(line) {
            continue;
        }

        let parsed = parse_source_line(line);
        let url = parsed.url;
        if !url.is_empty() {
            entries.push(SourceEntry {
                url,
                user_agent: parsed.user_agent,
                iptv_source: parsed.iptv_source,
                iptv_restricted: parsed.iptv_restricted,
                section,
                enabled,
            });
        }
    }

    entries
}

fn looks_like_source_line(line: &str) -> bool {
    let path = line.split_whitespace().next().unwrap_or_default();
    path.starts_with("http://")
        || path.starts_with("https://")
        || path.starts_with("file://")
        || path.starts_with('/')
        || path.starts_with("./")
        || path.starts_with("../")
        || has_known_source_extension(path)
}

fn has_known_source_extension(path: &str) -> bool {
    Path::new(path)
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| {
            matches!(
                ext.to_ascii_lowercase().as_str(),
                "txt" | "m3u" | "m3u8" | "xml" | "gz"
            )
        })
        .unwrap_or(false)
}

#[derive(Debug, Default)]
struct ParsedSourceLine {
    url: String,
    user_agent: Option<String>,
    iptv_source: Option<String>,
    iptv_restricted: bool,
}

fn parse_source_line(line: &str) -> ParsedSourceLine {
    let line = line.trim();
    let Some((url, options)) = split_url_and_options(line) else {
        return ParsedSourceLine {
            url: line.to_string(),
            ..ParsedSourceLine::default()
        };
    };

    let mut parsed = ParsedSourceLine {
        url: url.to_string(),
        ..ParsedSourceLine::default()
    };
    for (key, value) in parse_options(options) {
        match key.to_ascii_lowercase().as_str() {
            "ua" | "useragent" | "user-agent" => parsed.user_agent = Some(value),
            "iptv" | "iptv_source" | "iptv-source" => {
                parsed.iptv_source = Some(value);
                parsed.iptv_restricted = true;
            }
            "source" => parsed.iptv_source = Some(value),
            _ => {}
        }
    }
    parsed
}

fn split_url_and_options(line: &str) -> Option<(&str, &str)> {
    let bytes = line.as_bytes();
    for index in 0..bytes.len() {
        if !bytes[index].is_ascii_whitespace() {
            continue;
        }
        let rest = line[index..].trim_start();
        let Some((key, _)) = rest.split_once('=') else {
            continue;
        };
        if key
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-'))
        {
            return Some((line[..index].trim(), rest));
        }
    }
    None
}

fn parse_options(options: &str) -> Vec<(String, String)> {
    let mut pairs = Vec::new();
    let mut rest = options.trim();

    while !rest.is_empty() {
        let Some(eq_index) = rest.find('=') else {
            break;
        };
        let key = rest[..eq_index].trim();
        if key.is_empty() {
            break;
        }
        let mut value_start = rest[eq_index + 1..].trim_start();
        let (value, consumed) = if let Some(quote) = value_start
            .chars()
            .next()
            .filter(|ch| matches!(ch, '"' | '\''))
        {
            value_start = &value_start[quote.len_utf8()..];
            if let Some(end) = value_start.find(quote) {
                (value_start[..end].to_string(), end + quote.len_utf8())
            } else {
                (value_start.to_string(), value_start.len())
            }
        } else {
            let end = value_start
                .find(char::is_whitespace)
                .unwrap_or(value_start.len());
            (value_start[..end].to_string(), end)
        };

        if !value.is_empty() {
            pairs.push((key.to_string(), value));
        }
        rest = value_start[consumed..].trim_start();
    }

    pairs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ua_iptv_source_and_whitelist() {
        let entries = parse_source_list(
            r#"
https://a.test/live.m3u IPTV="home" UA="Agent A"
[WHITELIST]
https://b.test/live.txt UA=AgentB
"#,
        );

        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].user_agent.as_deref(), Some("Agent A"));
        assert_eq!(entries[0].iptv_source.as_deref(), Some("home"));
        assert!(entries[0].iptv_restricted);
        assert_eq!(entries[0].section, SourceSection::Default);
        assert_eq!(entries[1].user_agent.as_deref(), Some("AgentB"));
        assert_eq!(entries[1].iptv_source, None);
        assert!(!entries[1].iptv_restricted);
        assert_eq!(entries[1].section, SourceSection::Whitelist);
    }

    #[test]
    fn parses_local_paths_and_plain_source_labels() {
        let entries = parse_source_list(
            r#"
config/local/sh-unicom.m3u IPTV="sh-unicom"
file:///data/public.txt SOURCE=public
"#,
        );

        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].url, "config/local/sh-unicom.m3u");
        assert_eq!(entries[0].iptv_source.as_deref(), Some("sh-unicom"));
        assert!(entries[0].iptv_restricted);
        assert_eq!(entries[1].url, "file:///data/public.txt");
        assert_eq!(entries[1].iptv_source.as_deref(), Some("public"));
        assert!(!entries[1].iptv_restricted);
    }
}
