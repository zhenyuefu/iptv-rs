use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result};
use fancy_regex::Regex;

#[derive(Debug, Default)]
pub struct AliasMatcher {
    primary_to_aliases: HashMap<String, Vec<String>>,
    alias_to_primary: HashMap<String, String>,
    normalized_to_primary: HashMap<String, String>,
    patterns: Vec<(Regex, String)>,
}

impl AliasMatcher {
    pub fn from_file(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }

        let text = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read alias file {}", path.display()))?;
        let mut matcher = Self::default();

        for raw_line in text.lines() {
            let line = raw_line.trim();
            if line.is_empty() || line.starts_with('#') || !line.contains(',') {
                continue;
            }

            let mut parts = line
                .split(',')
                .map(str::trim)
                .filter(|part| !part.is_empty());
            let Some(primary) = parts.next() else {
                continue;
            };
            let primary = primary.to_string();
            matcher
                .alias_to_primary
                .insert(primary.clone(), primary.clone());

            matcher
                .normalized_to_primary
                .insert(format_name(&primary), primary.clone());
            let aliases: Vec<String> = parts.map(ToString::to_string).collect();

            for alias in &aliases {
                matcher
                    .alias_to_primary
                    .insert(alias.clone(), primary.clone());
                if let Some(pattern) = alias.strip_prefix("re:") {
                    match Regex::new(pattern) {
                        Ok(regex) => matcher.patterns.push((regex, primary.clone())),
                        Err(error) => eprintln!(
                            "invalid alias regex for {primary} in {}: {error}",
                            path.display()
                        ),
                    }
                }
            }
            matcher.primary_to_aliases.insert(primary, aliases);
        }

        Ok(matcher)
    }

    pub fn primary_name<'a>(&'a self, name: &'a str) -> String {
        if let Some(primary) = self.alias_to_primary.get(name) {
            return primary.clone();
        }

        for (pattern, primary) in &self.patterns {
            if pattern.is_match(name).unwrap_or(false) {
                return primary.clone();
            }
        }

        let formatted = format_name(name);
        self.alias_to_primary
            .get(&formatted)
            .or_else(|| self.normalized_to_primary.get(&formatted))
            .cloned()
            .unwrap_or_else(|| name.to_string())
    }
}

#[derive(Debug, Default)]
pub struct FilterRules {
    blacklist: Vec<String>,
    whitelist_exact: HashMap<String, Vec<String>>,
    whitelist_keywords: HashMap<String, Vec<String>>,
}

impl FilterRules {
    pub fn from_files(blacklist_path: &Path, whitelist_path: &Path) -> Result<Self> {
        Ok(Self {
            blacklist: load_plain_list(blacklist_path)?,
            whitelist_exact: load_whitelist(whitelist_path, false)?,
            whitelist_keywords: load_whitelist(whitelist_path, true)?,
        })
    }

    pub fn is_blacklisted(&self, url: &str) -> bool {
        self.blacklist.iter().any(|keyword| url.contains(keyword))
    }

    pub fn is_whitelisted(&self, url: &str, channel_name: &str) -> bool {
        self.exact_matches("", url)
            || self.exact_matches(channel_name, url)
            || self.keyword_matches("", url)
            || self.keyword_matches(channel_name, url)
    }

    pub fn whitelist_urls_for(&self, channel_name: &str) -> Vec<String> {
        let mut values = Vec::new();
        values.extend(self.whitelist_exact.get("").into_iter().flatten().cloned());
        values.extend(
            self.whitelist_exact
                .get(channel_name)
                .into_iter()
                .flatten()
                .cloned(),
        );
        dedupe(values)
    }

    fn exact_matches(&self, channel_name: &str, url: &str) -> bool {
        self.whitelist_exact
            .get(channel_name)
            .map(|urls| urls.iter().any(|candidate| candidate == url))
            .unwrap_or(false)
    }

    fn keyword_matches(&self, channel_name: &str, url: &str) -> bool {
        self.whitelist_keywords
            .get(channel_name)
            .map(|keywords| keywords.iter().any(|keyword| url.contains(keyword)))
            .unwrap_or(false)
    }
}

fn load_plain_list(path: &Path) -> Result<Vec<String>> {
    if !path.exists() {
        return Ok(Vec::new());
    }

    let text = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read list {}", path.display()))?;
    Ok(text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(ToString::to_string)
        .collect())
}

fn load_whitelist(path: &Path, keyword_section: bool) -> Result<HashMap<String, Vec<String>>> {
    let mut map: HashMap<String, Vec<String>> = HashMap::new();
    if !path.exists() {
        return Ok(map);
    }

    let text = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read whitelist {}", path.display()))?;
    let mut in_keywords = false;
    for raw_line in text.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        if line.starts_with('[') && line.ends_with(']') {
            in_keywords = line.eq_ignore_ascii_case("[KEYWORDS]");
            continue;
        }
        if in_keywords != keyword_section {
            continue;
        }

        let (key, value) = line
            .split_once(',')
            .map(|(name, value)| (name.trim(), value.trim()))
            .unwrap_or(("", line));
        if value.is_empty() {
            continue;
        }

        let values = map.entry(key.to_string()).or_default();
        if !values.iter().any(|existing| existing == value) {
            values.push(value.to_string());
        }
    }

    Ok(map)
}

fn dedupe(values: Vec<String>) -> Vec<String> {
    let mut out = Vec::new();
    for value in values {
        if !value.is_empty() && !out.iter().any(|existing| existing == &value) {
            out.push(value);
        }
    }
    out
}

fn format_name(name: &str) -> String {
    let replacements = [("plus", "+"), ("PLUS", "+"), ("＋", "+")];
    let mut value = name.to_string();
    for (old, new) in replacements {
        value = value.replace(old, new);
    }

    let mut out = String::new();
    let mut skip_depth = 0_u32;
    for ch in value.chars() {
        match ch {
            '(' | '（' | '[' | '「' => {
                skip_depth += 1;
                continue;
            }
            ')' | '）' | ']' | '」' => {
                skip_depth = skip_depth.saturating_sub(1);
                continue;
            }
            _ if skip_depth > 0 => continue,
            '-' | '_' | ' ' | '｜' => continue,
            _ => out.push(ch),
        }
    }

    for word in [
        "频道",
        "普清",
        "标清",
        "高清",
        "HD",
        "hd",
        "超清",
        "超高",
        "超高清",
        "4K",
        "4k",
        "中央",
        "央视",
        "电视台",
        "台",
        "电信",
        "联通",
        "移动",
    ] {
        out = out.replace(word, "");
    }

    out.to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_alias_catalog_is_usable() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("config/alias.txt");
        let matcher = AliasMatcher::from_file(&path).unwrap();
        let text = std::fs::read_to_string(path).unwrap();
        let mut pattern_count = 0;
        for line in text.lines().map(str::trim) {
            if line.starts_with('#') || !line.contains(',') {
                continue;
            }
            let mut parts = line.split(',').map(str::trim);
            let primary = parts.next().unwrap();
            assert_eq!(matcher.primary_name(primary), primary);
            for alias in parts.filter(|alias| !alias.is_empty()) {
                if let Some(pattern) = alias.strip_prefix("re:") {
                    Regex::new(pattern).unwrap_or_else(|error| {
                        panic!("invalid bundled pattern for {primary}: {error}")
                    });
                    pattern_count += 1;
                } else {
                    assert_eq!(matcher.primary_name(alias), primary, "alias: {alias}");
                }
            }
        }
        assert!(pattern_count > 0);
        assert_eq!(matcher.patterns.len(), pattern_count);
    }

    #[test]
    fn bundled_aliases_keep_distinct_channels_separate() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("config/alias.txt");
        let matcher = AliasMatcher::from_file(&path).unwrap();
        for (input, expected) in [
            ("cctv_01 [test feed]", "CCTV-1"),
            ("CCTV5＋体育赛事", "CCTV-5+"),
            ("CCTV4中文國際歐洲IPV6", "CCTV-4欧洲"),
            ("CCTV4 America test feed", "CCTV-4美洲"),
            ("CCTV-1綜合", "CCTV-1"),
            ("4kfitness", "4K健身"),
            ("TVB J1频道", "TVB J1"),
            ("CCTV5P", "CCTV-5+"),
            ("cctv1风云剧场", "风云剧场"),
            ("CCTV-1未知频道", "CCTV-1未知频道"),
            ("第一剧场", "第一剧场"),
            ("风云剧场", "风云剧场"),
            ("中国电影", "中国电影"),
            ("翡翠", "翡翠台"),
            ("翡翠台4K", "翡翠台4K"),
            ("魅力潇湘", "NewTV魅力潇湘"),
        ] {
            assert_eq!(matcher.primary_name(input), expected, "input: {input}");
        }
    }

    #[test]
    fn alias_regex_maps_to_primary() {
        let mut matcher = AliasMatcher::default();
        matcher.patterns.push((
            Regex::new(r"(?i)^cctv[-\s_]*0?1$").unwrap(),
            "CCTV-1".into(),
        ));

        assert_eq!(matcher.primary_name("cctv 01"), "CCTV-1");
    }

    #[test]
    fn whitelist_supports_exact_and_keywords() {
        let mut rules = FilterRules::default();
        rules
            .whitelist_exact
            .insert("CCTV-1".into(), vec!["http://a".into()]);
        rules
            .whitelist_keywords
            .insert("".into(), vec!["trusted".into()]);

        assert!(rules.is_whitelisted("http://a", "CCTV-1"));
        assert!(rules.is_whitelisted("http://trusted/live", "CCTV-2"));
        assert_eq!(rules.whitelist_urls_for("CCTV-1"), vec!["http://a"]);
    }
}
