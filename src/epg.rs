use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Write;
use std::path::Path;

use anyhow::{Context, Result};
use chrono::Utc;

use crate::models::Channel;

#[derive(Debug, Default)]
pub struct EpgAggregator {
    channels: HashMap<String, String>,
    programmes: Vec<EpgProgramme>,
    programme_keys: HashSet<String>,
}

#[derive(Debug, Clone)]
struct EpgProgramme {
    channel_id: String,
    xml: String,
}

impl EpgAggregator {
    pub fn add_document(&mut self, source_name: &str, bytes: &[u8]) -> Result<()> {
        let text = String::from_utf8_lossy(bytes);
        for element in extract_elements(&text, "channel") {
            if let Some(id) = attr_value(&element.start_tag, "id") {
                self.channels.entry(id).or_insert(element.xml);
            }
        }

        for element in extract_elements(&text, "programme") {
            let Some(channel_id) = attr_value(&element.start_tag, "channel") else {
                continue;
            };
            let key = format!("{source_name}\n{channel_id}\n{}", element.xml);
            if self.programme_keys.insert(key) {
                self.programmes.push(EpgProgramme {
                    channel_id,
                    xml: element.xml,
                });
            }
        }

        Ok(())
    }

    pub fn retain_for_channels(&mut self, channels: &[Channel]) {
        let wanted: HashSet<String> = channels
            .iter()
            .flat_map(Channel::epg_keys)
            .map(normalize_epg_key)
            .collect();

        if wanted.is_empty() {
            return;
        }

        self.channels
            .retain(|id, _| wanted.contains(&normalize_epg_key(id)));
        self.programmes
            .retain(|programme| wanted.contains(&normalize_epg_key(&programme.channel_id)));
    }

    pub fn write_to(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create EPG directory {}", parent.display()))?;
        }
        let mut file = fs::File::create(path)
            .with_context(|| format!("failed to create {}", path.display()))?;

        writeln!(file, r#"<?xml version="1.0" encoding="UTF-8"?>"#)?;
        writeln!(
            file,
            r#"<tv generator-info-name="iptv-rs" generator-info-url="https://github.com/Guovin/iptv-api" date="{}">"#,
            Utc::now().format("%Y%m%d%H%M%S %z")
        )?;

        let mut channel_ids: Vec<_> = self.channels.keys().collect();
        channel_ids.sort();
        for id in channel_ids {
            writeln!(file, "{}", self.channels[id])?;
        }

        for programme in &self.programmes {
            writeln!(file, "{}", programme.xml)?;
        }

        writeln!(file, "</tv>")?;
        Ok(())
    }
}

#[derive(Debug)]
struct XmlElement {
    start_tag: String,
    xml: String,
}

fn extract_elements(text: &str, tag: &str) -> Vec<XmlElement> {
    let mut elements = Vec::new();
    let mut search_start = 0;
    let open_prefix = format!("<{tag}");
    let close_tag = format!("</{tag}>");

    while let Some(relative_start) = text[search_start..].find(&open_prefix) {
        let start = search_start + relative_start;
        if !is_tag_boundary(text, start + open_prefix.len()) {
            search_start = start + open_prefix.len();
            continue;
        }

        let Some(start_tag_end) = text[start..].find('>').map(|idx| start + idx + 1) else {
            break;
        };
        let start_tag = text[start..start_tag_end].to_string();
        let end = if start_tag.trim_end().ends_with("/>") {
            start_tag_end
        } else {
            let Some(close_start) = text[start_tag_end..]
                .find(&close_tag)
                .map(|idx| start_tag_end + idx)
            else {
                break;
            };
            close_start + close_tag.len()
        };

        elements.push(XmlElement {
            start_tag,
            xml: text[start..end].trim().to_string(),
        });
        search_start = end;
    }

    elements
}

fn attr_value(start_tag: &str, attr: &str) -> Option<String> {
    let needle = format!("{attr}=");
    let index = start_tag.find(&needle)? + needle.len();
    let quote = start_tag[index..].chars().next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let value_start = index + quote.len_utf8();
    let value_end = start_tag[value_start..].find(quote)? + value_start;
    Some(xml_unescape(&start_tag[value_start..value_end]))
}

fn xml_unescape(value: &str) -> String {
    value
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

fn is_tag_boundary(text: &str, index: usize) -> bool {
    text[index..]
        .chars()
        .next()
        .map(|ch| ch.is_whitespace() || ch == '>' || ch == '/')
        .unwrap_or(false)
}

fn normalize_epg_key(value: &str) -> String {
    value
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Channel, Stream};

    #[test]
    fn extracts_and_filters_epg() {
        let xml = br#"<?xml version="1.0"?>
<tv>
<channel id="cctv1"><display-name>CCTV-1</display-name></channel>
<channel id="other"><display-name>Other</display-name></channel>
<programme channel="cctv1" start="20240101000000 +0800"><title>News</title></programme>
<programme channel="other" start="20240101000000 +0800"><title>Other</title></programme>
</tv>"#;
        let mut epg = EpgAggregator::default();
        epg.add_document("test", xml).unwrap();
        epg.retain_for_channels(&[Channel {
            name: "CCTV-1".into(),
            group: None,
            tvg_id: Some("cctv1".into()),
            logo: None,
            streams: vec![Stream {
                url: "http://a".into(),
                origin: crate::models::Origin::Local,
                whitelist: false,
                source_order: 0,
                ipv_type: crate::models::IpvType::Ipv4,
            }],
            order: 0,
        }]);

        assert!(epg.channels.contains_key("cctv1"));
        assert_eq!(epg.programmes.len(), 1);
    }
}
