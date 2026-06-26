use anyhow::{Context, Result, anyhow};

use crate::models::{Channel, IpvType, Origin, Stream};

const HEADER: &str = "iptv-rs-metadata-v2";
const LEGACY_HEADER: &str = "iptv-rs-metadata-v1";

pub fn channels_to_metadata_bytes(channels: &[Channel]) -> Vec<u8> {
    let mut output = String::new();
    output.push_str(HEADER);
    output.push('\n');

    for channel in channels {
        push_fields(
            &mut output,
            &[
                "C".to_string(),
                encode(&channel.name),
                encode_option(channel.group.as_deref()),
                encode_option(channel.tvg_id.as_deref()),
                encode_option(channel.logo.as_deref()),
                channel.order.to_string(),
            ],
        );
        for stream in &channel.streams {
            push_fields(
                &mut output,
                &[
                    "S".to_string(),
                    encode(&stream.url),
                    origin_name(stream.origin).to_string(),
                    stream.whitelist.to_string(),
                    stream.source_order.to_string(),
                    encode_option(stream.iptv_source.as_deref()),
                    stream.iptv_restricted.to_string(),
                    stream.ipv_type.as_str().to_string(),
                ],
            );
        }
    }

    output.into_bytes()
}

pub fn channels_from_metadata_bytes(bytes: &[u8]) -> Result<Vec<Channel>> {
    let text = std::str::from_utf8(bytes).context("result metadata must be utf-8")?;
    let mut lines = text.lines();
    let header = lines.next().unwrap_or_default();
    if header != HEADER && header != LEGACY_HEADER {
        return Err(anyhow!("unsupported result metadata format `{header}`"));
    }

    let mut channels: Vec<Channel> = Vec::new();
    for line in lines {
        if line.trim().is_empty() {
            continue;
        }
        let fields: Vec<&str> = line.split('\t').collect();
        match fields.first().copied() {
            Some("C") => channels.push(parse_channel(&fields)?),
            Some("S") => {
                let stream = parse_stream(&fields)?;
                let Some(channel) = channels.last_mut() else {
                    return Err(anyhow!("stream metadata appeared before a channel"));
                };
                channel.streams.push(stream);
            }
            Some(kind) => return Err(anyhow!("unknown result metadata row `{kind}`")),
            None => {}
        }
    }

    Ok(channels)
}

fn parse_channel(fields: &[&str]) -> Result<Channel> {
    if fields.len() != 6 {
        return Err(anyhow!("channel metadata row has {} fields", fields.len()));
    }
    Ok(Channel {
        name: decode(fields[1])?,
        group: decode_option(fields[2])?,
        tvg_id: decode_option(fields[3])?,
        logo: decode_option(fields[4])?,
        streams: Vec::new(),
        order: fields[5]
            .parse()
            .with_context(|| format!("invalid channel order `{}`", fields[5]))?,
    })
}

fn parse_stream(fields: &[&str]) -> Result<Stream> {
    if fields.len() != 7 && fields.len() != 8 {
        return Err(anyhow!("stream metadata row has {} fields", fields.len()));
    }
    let (iptv_restricted, ipv_type) = if fields.len() == 8 {
        (
            fields[6]
                .parse()
                .with_context(|| format!("invalid IPTV restricted flag `{}`", fields[6]))?,
            parse_ipv_type(fields[7]),
        )
    } else {
        (false, parse_ipv_type(fields[6]))
    };
    Ok(Stream {
        url: decode(fields[1])?,
        origin: parse_origin(fields[2])?,
        whitelist: fields[3]
            .parse()
            .with_context(|| format!("invalid whitelist flag `{}`", fields[3]))?,
        source_order: fields[4]
            .parse()
            .with_context(|| format!("invalid source order `{}`", fields[4]))?,
        iptv_source: decode_option(fields[5])?,
        iptv_restricted,
        ipv_type,
    })
}

fn push_fields(output: &mut String, fields: &[String]) {
    output.push_str(&fields.join("\t"));
    output.push('\n');
}

fn encode_option(value: Option<&str>) -> String {
    value.map(encode).unwrap_or_default()
}

fn decode_option(value: &str) -> Result<Option<String>> {
    if value.is_empty() {
        Ok(None)
    } else {
        decode(value).map(Some)
    }
}

fn encode(value: &str) -> String {
    let mut encoded = String::new();
    for ch in value.chars() {
        match ch {
            '%' | '\t' | '\n' | '\r' => {
                let byte = ch as u8;
                encoded.push('%');
                encoded.push(hex_char(byte >> 4));
                encoded.push(hex_char(byte & 0x0f));
            }
            _ => encoded.push(ch),
        }
    }
    encoded
}

fn decode(value: &str) -> Result<String> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len() {
                return Err(anyhow!("invalid percent escape in metadata"));
            }
            let high = hex_value(bytes[index + 1])?;
            let low = hex_value(bytes[index + 2])?;
            decoded.push((high << 4) | low);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).context("metadata field is not valid utf-8")
}

fn hex_char(value: u8) -> char {
    match value {
        0..=9 => (b'0' + value) as char,
        10..=15 => (b'A' + value - 10) as char,
        _ => unreachable!("hex nybble out of range"),
    }
}

fn hex_value(value: u8) -> Result<u8> {
    match value {
        b'0'..=b'9' => Ok(value - b'0'),
        b'a'..=b'f' => Ok(value - b'a' + 10),
        b'A'..=b'F' => Ok(value - b'A' + 10),
        _ => Err(anyhow!("invalid hex digit `{}` in metadata", value as char)),
    }
}

fn origin_name(origin: Origin) -> &'static str {
    match origin {
        Origin::Template => "template",
        Origin::Local => "local",
        Origin::Subscribe => "subscribe",
        Origin::SubscribeWhitelist => "subscribe_whitelist",
    }
}

fn parse_origin(value: &str) -> Result<Origin> {
    match value {
        "template" => Ok(Origin::Template),
        "local" => Ok(Origin::Local),
        "subscribe" => Ok(Origin::Subscribe),
        "subscribe_whitelist" => Ok(Origin::SubscribeWhitelist),
        other => Err(anyhow!("unknown result metadata origin `{other}`")),
    }
}

fn parse_ipv_type(value: &str) -> IpvType {
    match value {
        "ipv4" => IpvType::Ipv4,
        "ipv6" => IpvType::Ipv6,
        _ => IpvType::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_unicode_metadata() {
        let channels = vec![Channel {
            name: "央视一套".into(),
            group: Some("央视".into()),
            tvg_id: Some("cctv1".into()),
            logo: None,
            streams: vec![Stream {
                url: "http://a.test/live%20/央视.m3u8".into(),
                origin: Origin::Local,
                whitelist: false,
                source_order: 1,
                iptv_source: Some("home".into()),
                iptv_restricted: true,
                ipv_type: IpvType::Ipv4,
            }],
            order: 7,
        }];

        let bytes = channels_to_metadata_bytes(&channels);
        let decoded = channels_from_metadata_bytes(&bytes).unwrap();

        assert_eq!(decoded, channels);
    }

    #[test]
    fn reads_legacy_metadata_without_restricted_flag() {
        let decoded = channels_from_metadata_bytes(
            b"iptv-rs-metadata-v1\nC\tCCTV-1\t\t\t\t1\nS\thttp://a.test/live.m3u8\tlocal\tfalse\t0\thome\tipv4\n",
        )
        .unwrap();

        let stream = &decoded[0].streams[0];
        assert_eq!(stream.iptv_source.as_deref(), Some("home"));
        assert!(!stream.iptv_restricted);
        assert_eq!(stream.ipv_type, IpvType::Ipv4);
    }
}
