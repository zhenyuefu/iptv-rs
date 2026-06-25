use std::ffi::OsStr;
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Component, Path, PathBuf};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result};

use crate::config::Settings;

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
    let Some(path) = parse_path(&request) else {
        return write_response(
            &mut stream,
            400,
            "text/plain; charset=utf-8",
            b"bad request",
        );
    };

    match resolve_route(&path, settings) {
        Route::Health => write_response(&mut stream, 200, "text/plain; charset=utf-8", b"ok"),
        Route::Info => write_response(
            &mut stream,
            200,
            "text/plain; charset=utf-8",
            public_url_listing(settings).as_bytes(),
        ),
        Route::File(path) => write_file(&mut stream, &path),
        Route::NotFound => {
            write_response(&mut stream, 404, "text/plain; charset=utf-8", b"not found")
        }
    }
}

enum Route {
    Health,
    Info,
    File(PathBuf),
    NotFound,
}

fn resolve_route(path: &str, settings: &Settings) -> Route {
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

fn parse_path(request: &str) -> Option<String> {
    let mut parts = request.lines().next()?.split_whitespace();
    let method = parts.next()?;
    if method != "GET" && method != "HEAD" {
        return None;
    }
    let raw_path = parts.next()?;
    Some(raw_path.split('?').next().unwrap_or(raw_path).to_string())
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

    #[test]
    fn rejects_path_traversal() {
        assert!(safe_join(Path::new("/tmp/output"), "result.txt").is_some());
        assert!(safe_join(Path::new("/tmp/output"), "../config.ini").is_none());
        assert!(safe_join(Path::new("/tmp/output"), "/etc/passwd").is_none());
    }
}
