//! Exercise the production CLI, updater and media listeners together on loopback.
use axum::{
    Router,
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Redirect},
    routing::get,
};
use reqwest::blocking::Client;
use serde_json::json;
use std::{
    fs,
    net::TcpListener,
    path::PathBuf,
    process::{Child, Command},
    thread,
    time::Duration,
};

struct Service {
    child: Child,
    root: PathBuf,
}
impl Drop for Service {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn aggregate_and_relay_over_http() {
    let fixture = TcpListener::bind("127.0.0.1:0").unwrap();
    let relay_socket = TcpListener::bind("127.0.0.1:0").unwrap();
    let agg_socket = TcpListener::bind("127.0.0.1:0").unwrap();
    let relay_address = relay_socket.local_addr().unwrap();
    let agg_address = agg_socket.local_addr().unwrap();
    let origin = format!("http://{}", fixture.local_addr().unwrap());
    let relay = format!("http://{relay_address}");
    let aggregate = format!("http://{agg_address}");
    let catalogue = format!(
        "News,{origin}/redirect?secret=test\nForeign,http://invalid.example/live\nDenied,{origin}/denied\n"
    );
    let channels = format!("#EXTM3U\n#EXTINF:-1,News\n{origin}/raw.ts\n");
    async fn redirect(headers: HeaderMap) -> axum::response::Response {
        if headers.contains_key(header::RANGE) {
            return StatusCode::RANGE_NOT_SATISFIABLE.into_response();
        }
        Redirect::temporary("/master.m3u8").into_response()
    }

    async fn raw(headers: HeaderMap) -> axum::response::Response {
        if headers.get(header::RANGE).is_some_and(|v| v == "bytes=1-3") {
            return (
                StatusCode::PARTIAL_CONTENT,
                [(header::CONTENT_RANGE, "bytes 1-3/7")],
                "123",
            )
                .into_response();
        }
        let chunks = (0..80).map(|_| {
            Ok::<_, std::convert::Infallible>(axum::body::Bytes::from_static(&[0x47; 65536]))
        });
        axum::body::Body::from_stream(futures_util::stream::iter(chunks)).into_response()
    }

    let router = Router::new()
        .route("/unicom", get(move || async move { catalogue }))
        .route("/ac5", get(move || async move { channels }))
        .route("/redirect", get(redirect))
        .route("/denied", get(|| async { Redirect::temporary("http://invalid.example/secret") }))
        .route("/master.m3u8", get(|| async { "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=100\nchild/index.m3u8\n" }))
        .route("/child/index.m3u8", get(|| async { "#EXTM3U\n#EXT-X-KEY:METHOD=AES-128,URI=\"key.bin\"\n#EXTINF:2,\n../raw.ts?ticket=test\n" }))
        .route("/child/key.bin", get(|| async { "fixture-key" }))
        .route("/raw.ts", get(raw));
    fixture.set_nonblocking(true).unwrap();
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.spawn(async move {
        axum::serve(tokio::net::TcpListener::from_std(fixture).unwrap(), router)
            .await
            .unwrap()
    });
    let root = std::env::temp_dir().join(format!(
        "iptv-rs-test-{}-{}",
        std::process::id(),
        relay_address.port()
    ));
    fs::create_dir_all(root.join("config")).unwrap();
    fs::write(root.join("config/config.ini"),format!("[Settings]\nnginx_http_port={}\nopen_local=False\nopen_epg=False\nupdate_interval=0\nopen_update_time=False\n",agg_address.port())).unwrap();
    fs::write(root.join("config/demo.txt"), "Local,#genre#\nNews\n").unwrap();
    fs::write(
        root.join("config/subscribe.txt"),
        format!("{relay}/unicom.txt\n"),
    )
    .unwrap();
    let sources=["unicom","ac5"].map(|id|json!({"id":id,"group":id,"playlist":format!("{origin}/{id}"),"origins":[origin],"format":if id=="ac5" {"ts"} else {"hls"}}));
    fs::write(
        root.join("relay.json"),
        json!({"listeners":[{"bind":relay_address,"public_base":relay}],"sources":sources})
            .to_string(),
    )
    .unwrap();
    drop((agg_socket, relay_socket));
    let child = Command::new(env!("CARGO_BIN_EXE_iptv-rs"))
        .args(["serve", "--relay-config", "relay.json"])
        .current_dir(&root)
        .env_clear()
        .spawn()
        .unwrap();
    let _service = Service { child, root };
    let http = Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(3))
        .build()
        .unwrap();
    for _ in 0..150 {
        if http.get(format!("{aggregate}/health")).send().is_ok() {
            break;
        }
        thread::sleep(Duration::from_millis(40));
    }
    let fetch = |url: &str| {
        http.get(url)
            .send()
            .unwrap()
            .error_for_status()
            .unwrap()
            .text()
            .unwrap()
    };
    let list = fetch(&format!("{relay}/playlist.m3u"));
    assert_eq!(list.matches("#EXTINF").count(), 3);
    assert!(
        list.contains("group-title=\"ac5\"") && !list.contains("secret") && !list.contains(&origin)
    );
    let station = list.lines().find(|s| s.contains("/live/unicom")).unwrap();
    let key = station.trim_end_matches(".m3u8");
    assert!(fetch(&format!("{aggregate}/m3u")).contains(key));
    let master = http
        .get(station)
        .header(header::RANGE, "bytes=0-")
        .send()
        .unwrap()
        .error_for_status()
        .unwrap()
        .text()
        .unwrap();
    let child = master.lines().last().unwrap();
    assert!(child.starts_with("/media/") && child.ends_with(".m3u8"));
    let manifest = fetch(&format!("{relay}{child}"));
    assert!(!manifest.contains("ticket"));
    let resource = manifest
        .split("URI=\"")
        .nth(1)
        .unwrap()
        .split('"')
        .next()
        .unwrap();
    assert_eq!(fetch(&format!("{relay}{resource}")), "fixture-key");
    let segment = manifest.lines().last().unwrap();
    let response = http
        .get(format!("{relay}{segment}"))
        .header(header::RANGE, "bytes=1-3")
        .send()
        .unwrap();
    assert_eq!(response.status(), 206);
    assert_eq!(response.headers()[header::CONTENT_RANGE], "bytes 1-3/7");
    assert_eq!(response.text().unwrap(), "123");
    let deny = list
        .lines()
        .skip_while(|s| !s.ends_with(",Denied"))
        .nth(1)
        .unwrap();
    assert_eq!(http.get(deny).send().unwrap().status(), 502);
    let raw = list.lines().find(|s| s.contains("/live/ac5/")).unwrap();
    thread::scope(|scope| {
        for _ in 0..8 {
            scope.spawn(|| {
                let mut media = http.get(raw).send().unwrap().error_for_status().unwrap();
                assert_eq!(
                    std::io::copy(&mut media, &mut std::io::sink()).unwrap(),
                    5 * 1024 * 1024
                );
            });
        }
    });
}
