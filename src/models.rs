use std::collections::HashMap;

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub enum Origin {
    Template,
    Local,
    Subscribe,
    SubscribeWhitelist,
}

impl Origin {
    pub fn priority(self) -> usize {
        match self {
            Origin::Template => 0,
            Origin::SubscribeWhitelist => 1,
            Origin::Local => 2,
            Origin::Subscribe => 3,
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Stream {
    pub url: String,
    pub origin: Origin,
    pub whitelist: bool,
    pub source_order: usize,
    pub ipv_type: IpvType,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub enum IpvType {
    Ipv4,
    Ipv6,
    Unknown,
}

impl IpvType {
    pub fn as_str(self) -> &'static str {
        match self {
            IpvType::Ipv4 => "ipv4",
            IpvType::Ipv6 => "ipv6",
            IpvType::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ParsedChannel {
    pub name: String,
    pub group: Option<String>,
    pub tvg_id: Option<String>,
    pub logo: Option<String>,
    pub stream: Option<Stream>,
    pub order: usize,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Channel {
    pub name: String,
    pub group: Option<String>,
    pub tvg_id: Option<String>,
    pub logo: Option<String>,
    pub streams: Vec<Stream>,
    pub order: usize,
}

impl Channel {
    pub fn new(item: ParsedChannel) -> Self {
        let streams = item.stream.into_iter().collect();
        Self {
            name: item.name,
            group: item.group,
            tvg_id: item.tvg_id,
            logo: item.logo,
            streams,
            order: item.order,
        }
    }

    pub fn merge(&mut self, item: ParsedChannel) {
        if self.group.is_none() {
            self.group = item.group;
        }
        if self.tvg_id.is_none() {
            self.tvg_id = item.tvg_id;
        }
        if self.logo.is_none() {
            self.logo = item.logo;
        }
        self.order = self.order.min(item.order);
        if let Some(stream) = item.stream {
            if !self
                .streams
                .iter()
                .any(|existing| existing.url == stream.url)
            {
                self.streams.push(stream);
            }
        }
    }

    pub fn epg_keys(&self) -> impl Iterator<Item = &str> {
        self.tvg_id
            .iter()
            .map(String::as_str)
            .chain(std::iter::once(self.name.as_str()))
    }
}

pub type ChannelMap = HashMap<String, Channel>;
