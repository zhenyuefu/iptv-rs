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

        let (url, user_agent) = split_user_agent(line);
        if !url.is_empty() {
            entries.push(SourceEntry {
                url,
                user_agent,
                section,
                enabled,
            });
        }
    }

    entries
}

fn looks_like_source_line(line: &str) -> bool {
    line.starts_with("http://")
        || line.starts_with("https://")
        || line.starts_with("file://")
        || line.starts_with('/')
        || line.starts_with("./")
        || line.starts_with("../")
}

fn split_user_agent(line: &str) -> (String, Option<String>) {
    let Some(index) = line.find(" UA=") else {
        return (line.trim().to_string(), None);
    };
    let url = line[..index].trim().to_string();
    let mut value = line[index + 4..].trim();
    if let Some(stripped) = value.strip_prefix('"').and_then(|v| v.strip_suffix('"')) {
        value = stripped;
    }
    let ua = (!value.is_empty()).then(|| value.to_string());
    (url, ua)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ua_and_whitelist() {
        let entries = parse_source_list(
            r#"
https://a.test/live.m3u UA="Agent A"
[WHITELIST]
https://b.test/live.txt UA=AgentB
"#,
        );

        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].user_agent.as_deref(), Some("Agent A"));
        assert_eq!(entries[0].section, SourceSection::Default);
        assert_eq!(entries[1].user_agent.as_deref(), Some("AgentB"));
        assert_eq!(entries[1].section, SourceSection::Whitelist);
    }
}
