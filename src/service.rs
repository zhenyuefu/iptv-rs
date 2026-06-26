use std::ffi::OsStr;
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Component, Path, PathBuf};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result};
use url::form_urlencoded;

use crate::config::Settings;
use crate::metadata::channels_from_metadata_bytes;
use crate::models::Channel;
use crate::output::{render_m3u, render_txt};
use crate::playlist::{apply_output_preferences, limit_channel_streams};

type UpdateFn = fn(PathBuf) -> Result<()>;

pub fn serve(config_path: PathBuf, update: UpdateFn) -> Result<()> {
    let settings = Settings::from_file(&config_path)?;
    if !settings.open_service {
        return update(config_path);
    }

    if settings.update_startup {
        run_update_without_stopping_service(&config_path, update);
    }

    if settings.update_interval > 0 {
        spawn_update_loop(config_path.clone(), settings.update_interval, update);
    }

    let bind_addr = format!("0.0.0.0:{}", settings.nginx_http_port);
    let listener = TcpListener::bind(&bind_addr)
        .with_context(|| format!("failed to bind HTTP service on {bind_addr}"))?;
    eprintln!("iptv-rs service listening on http://{bind_addr}");

    for stream in listener.incoming() {
        let settings = settings.clone();
        match stream {
            Ok(stream) => {
                thread::spawn(move || {
                    if let Err(err) = handle_client(stream, &settings) {
                        eprintln!("request failed: {err:?}");
                    }
                });
            }
            Err(err) => eprintln!("connection failed: {err}"),
        }
    }

    Ok(())
}

fn spawn_update_loop(config_path: PathBuf, interval_hours: u64, update: UpdateFn) {
    thread::spawn(move || {
        let interval = Duration::from_secs(interval_hours.saturating_mul(3600));
        loop {
            thread::sleep(interval);
            run_update_without_stopping_service(&config_path, update);
        }
    });
}

fn run_update_without_stopping_service(config_path: &Path, update: UpdateFn) {
    eprintln!("starting IPTV output update");
    match update(config_path.to_path_buf()) {
        Ok(()) => eprintln!("IPTV output update finished"),
        Err(err) => eprintln!("IPTV output update failed: {err:?}"),
    }
}

fn handle_client(mut stream: TcpStream, settings: &Settings) -> Result<()> {
    let mut buffer = [0_u8; 8192];
    let bytes_read = stream.read(&mut buffer)?;
    if bytes_read == 0 {
        return Ok(());
    }

    let request = String::from_utf8_lossy(&buffer[..bytes_read]);
    let Some(target) = parse_request_target(&request) else {
        return write_response(
            &mut stream,
            400,
            "text/plain; charset=utf-8",
            b"bad request",
        );
    };

    match resolve_route(&target.path, settings, &target.iptv_source_prefer) {
        Route::Health => write_response(&mut stream, 200, "text/plain; charset=utf-8", b"ok"),
        Route::Info => write_response(
            &mut stream,
            200,
            "text/plain; charset=utf-8",
            public_url_listing(settings).as_bytes(),
        ),
        Route::File(path) => write_file(&mut stream, &path),
        Route::Generated(format, preferred_sources) => {
            write_generated(&mut stream, settings, format, &preferred_sources)
        }
        Route::NotFound => {
            write_response(&mut stream, 404, "text/plain; charset=utf-8", b"not found")
        }
    }
}

enum Route {
    Health,
    Info,
    File(PathBuf),
    Generated(GeneratedFormat, Vec<String>),
    NotFound,
}

#[derive(Debug, Clone, Copy)]
enum GeneratedFormat {
    Txt,
    M3u,
}

fn resolve_route(path: &str, settings: &Settings, preferred_sources: &[String]) -> Route {
    if !preferred_sources.is_empty() {
        match path {
            "/" | "/txt" | "/content" => {
                return Route::Generated(GeneratedFormat::Txt, preferred_sources.to_vec());
            }
            "/m3u" => return Route::Generated(GeneratedFormat::M3u, preferred_sources.to_vec()),
            _ => {}
        }
    }

    match path {
        "/health" => Route::Health,
        "/info" => Route::Info,
        "/" | "/txt" | "/content" => Route::File(settings.resolve(&settings.final_file)),
        "/m3u" => Route::File(settings.resolve(&settings.final_file).with_extension("m3u")),
        "/epg" | "/epg/" | "/epg/epg.xml" => {
            Route::File(settings.resolve(&settings.epg_output_file))
        }
        _ => {
            if let Some(relative) = path.strip_prefix("/output/") {
                return safe_join(&settings.root.join("output"), relative)
                    .map(Route::File)
                    .unwrap_or(Route::NotFound);
            }
            if let Some(relative) = path.strip_prefix("/config/logo/") {
                return safe_join(&settings.resolve(&settings.logo_dir), relative)
                    .map(Route::File)
                    .unwrap_or(Route::NotFound);
            }
            if let Some(relative) = path.strip_prefix("/logo/") {
                return safe_join(&settings.resolve(&settings.logo_dir), relative)
                    .map(Route::File)
                    .unwrap_or(Route::NotFound);
            }
            Route::NotFound
        }
    }
}

#[derive(Debug, Default)]
struct RequestTarget {
    path: String,
    iptv_source_prefer: Vec<String>,
}

fn parse_request_target(request: &str) -> Option<RequestTarget> {
    let mut parts = request.lines().next()?.split_whitespace();
    let method = parts.next()?;
    if method != "GET" && method != "HEAD" {
        return None;
    }
    let raw_path = parts.next()?;
    let (path, query) = raw_path.split_once('?').unwrap_or((raw_path, ""));
    Some(RequestTarget {
        path: path.to_string(),
        iptv_source_prefer: parse_iptv_source_prefer(query),
    })
}

fn parse_iptv_source_prefer(query: &str) -> Vec<String> {
    form_urlencoded::parse(query.as_bytes())
        .filter(|(key, _)| matches!(key.as_ref(), "iptv" | "source" | "iptv_source"))
        .flat_map(|(_, value)| {
            value
                .split(',')
                .map(|part| part.trim().to_ascii_lowercase())
                .filter(|part| !part.is_empty())
                .collect::<Vec<_>>()
        })
        .collect()
}

fn write_file(stream: &mut TcpStream, path: &Path) -> Result<()> {
    match fs::read(path) {
        Ok(bytes) => write_response(stream, 200, content_type(path), &bytes),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            write_response(stream, 404, "text/plain; charset=utf-8", b"not found")
        }
        Err(err) => Err(err).with_context(|| format!("failed to read {}", path.display())),
    }
}

fn write_generated(
    stream: &mut TcpStream,
    settings: &Settings,
    format: GeneratedFormat,
    preferred_sources: &[String],
) -> Result<()> {
    let metadata_path = settings
        .resolve(&settings.final_file)
        .with_extension("metadata.tsv");
    let bytes = match fs::read(&metadata_path) {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return write_response(stream, 404, "text/plain; charset=utf-8", b"not found");
        }
        Err(err) => {
            return Err(err).with_context(|| format!("failed to read {}", metadata_path.display()));
        }
    };

    let mut channels = channels_from_metadata_bytes(&bytes)
        .with_context(|| format!("failed to parse {}", metadata_path.display()))?;
    let dynamic_settings = generated_settings(settings, preferred_sources);
    prepare_generated_channels(&mut channels, &dynamic_settings);

    let mut body = Vec::new();
    let content_type = match format {
        GeneratedFormat::Txt => {
            render_txt(&mut body, &channels, &dynamic_settings)?;
            "text/plain; charset=utf-8"
        }
        GeneratedFormat::M3u => {
            render_m3u(&mut body, &channels)?;
            "audio/x-mpegurl; charset=utf-8"
        }
    };
    write_response(stream, 200, content_type, &body)
}

fn generated_settings(settings: &Settings, preferred_sources: &[String]) -> Settings {
    let mut dynamic_settings = settings.clone();
    dynamic_settings.iptv_source_filter = preferred_sources.to_vec();
    dynamic_settings.iptv_source_prefer = preferred_sources.to_vec();
    dynamic_settings
}

fn prepare_generated_channels(channels: &mut Vec<Channel>, settings: &Settings) {
    apply_output_preferences(channels, settings);
    limit_channel_streams(channels, settings);
}

fn write_response(
    stream: &mut TcpStream,
    status: u16,
    content_type: &str,
    body: &[u8],
) -> Result<()> {
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        _ => "Internal Server Error",
    };
    write!(
        stream,
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    stream.write_all(body)?;
    Ok(())
}

fn content_type(path: &Path) -> &'static str {
    match path.extension().and_then(OsStr::to_str).unwrap_or_default() {
        "m3u" | "m3u8" => "audio/x-mpegurl; charset=utf-8",
        "xml" => "application/xml; charset=utf-8",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "json" => "application/json; charset=utf-8",
        _ => "text/plain; charset=utf-8",
    }
}

fn public_url_listing(settings: &Settings) -> String {
    let base = public_base_url(settings);
    format!(
        "iptv-rs\n\n/txt  {base}/txt\n/m3u  {base}/m3u\n/epg  {base}/epg\n/content  {base}/content\n"
    )
}

fn public_base_url(settings: &Settings) -> String {
    let default_port = match settings.public_scheme.as_str() {
        "https" => 443,
        _ => 80,
    };
    if settings.public_port == default_port {
        format!("{}://{}", settings.public_scheme, settings.public_domain)
    } else {
        format!(
            "{}://{}:{}",
            settings.public_scheme, settings.public_domain, settings.public_port
        )
    }
}

fn safe_join(base: &Path, relative: &str) -> Option<PathBuf> {
    let mut path = base.to_path_buf();
    for component in Path::new(relative).components() {
        match component {
            Component::Normal(part) => path.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    Some(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metadata::{channels_from_metadata_bytes, channels_to_metadata_bytes};
    use crate::models::{IpvType, Origin, Stream};

    fn test_settings() -> Settings {
        Settings {
            root: PathBuf::from("."),
            open_update: true,
            open_service: true,
            open_local: true,
            open_subscribe: true,
            open_auto_disable_source: true,
            open_history: true,
            open_unmatch_category: true,
            open_empty_category: false,
            open_update_time: true,
            open_url_info: false,
            open_epg: true,
            open_m3u_result: true,
            update_startup: true,
            update_interval: 12,
            update_time_position: "top".into(),
            nginx_http_port: 8080,
            public_scheme: "http".into(),
            public_domain: "127.0.0.1".into(),
            public_port: 80,
            source_file: "config/demo.txt".into(),
            local_source_list_file: "config/local_sources.txt".into(),
            subscribe_file: "config/subscribe.txt".into(),
            epg_file: "config/epg.txt".into(),
            alias_file: "config/alias.txt".into(),
            blacklist_file: "config/blacklist.txt".into(),
            whitelist_file: "config/whitelist.txt".into(),
            final_file: "output/result.txt".into(),
            epg_output_file: "output/epg/epg.xml".into(),
            urls_limit: 5,
            request_timeout: 10,
            ipv_type: "all".into(),
            ipv_type_prefer: Vec::new(),
            origin_type_prefer: Vec::new(),
            iptv_source_prefer: Vec::new(),
            iptv_source_filter: Vec::new(),
            default_user_agent: "iptv-rs/0.1".into(),
            http_proxy: None,
            logo_dir: "config/logo".into(),
            local_logo_base_url: None,
            logo_url: None,
            logo_type: "png".into(),
        }
    }

    #[test]
    fn rejects_path_traversal() {
        assert!(safe_join(Path::new("/tmp/output"), "result.txt").is_some());
        assert!(safe_join(Path::new("/tmp/output"), "../config.ini").is_none());
        assert!(safe_join(Path::new("/tmp/output"), "/etc/passwd").is_none());
    }

    #[test]
    fn parses_iptv_source_query() {
        let target = parse_request_target("GET /txt?iptv=home,backup&x=1 HTTP/1.1").unwrap();

        assert_eq!(target.path, "/txt");
        assert_eq!(target.iptv_source_prefer, vec!["home", "backup"]);
    }

    #[test]
    fn generated_output_filters_restricted_iptv_sources_from_query() {
        let channels = vec![Channel {
            name: "CCTV-1".into(),
            group: Some("央视".into()),
            tvg_id: None,
            logo: None,
            streams: vec![
                stream("http://zj.test/live.m3u8", Some("zj-telecom"), true),
                stream("http://public.test/live.m3u8", None, false),
                stream("http://sh.test/live.m3u8", Some("sh-unicom"), true),
            ],
            order: 1,
        }];
        let bytes = channels_to_metadata_bytes(&channels);
        let mut decoded = channels_from_metadata_bytes(&bytes).unwrap();
        let dynamic_settings = generated_settings(&test_settings(), &["sh-unicom".into()]);

        prepare_generated_channels(&mut decoded, &dynamic_settings);

        let urls: Vec<&str> = decoded[0]
            .streams
            .iter()
            .map(|stream| stream.url.as_str())
            .collect();
        assert_eq!(
            urls,
            vec!["http://sh.test/live.m3u8", "http://public.test/live.m3u8"]
        );
    }

    fn stream(url: &str, iptv_source: Option<&str>, iptv_restricted: bool) -> Stream {
        Stream {
            url: url.into(),
            origin: Origin::Subscribe,
            whitelist: false,
            source_order: 0,
            iptv_source: iptv_source.map(ToString::to_string),
            iptv_restricted,
            ipv_type: IpvType::Ipv4,
        }
    }
}
