use serde_json::json;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::process::Command;

#[test]
fn export_two_views_and_keep_last_good_on_incomplete_update() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    fs::create_dir(root.join("config")).unwrap();
    fs::write(root.join("config/config.ini"), "[Settings]\nopen_local=False\nopen_epg=False\nopen_auto_disable_source=False\nopen_history=False\n").unwrap();
    fs::write(
        root.join("config/demo.txt"),
        "News stations,#genre#\nNews\n",
    )
    .unwrap();
    fs::write(root.join("config/alias.txt"), "News,Station A,Station B\n").unwrap();
    fs::write(
        root.join("config/subscribe.txt"),
        "a.txt Source=\"a\"\nb.txt Source=\"b\"\n",
    )
    .unwrap();
    let a = "http://192.0.2.1/channel?token=a";
    let b = "http://192.0.2.2/events";
    fs::write(root.join("a.txt"), format!("Station A,{a}\n")).unwrap();
    fs::write(root.join("b.txt"), format!("Station B,{b}\n")).unwrap();

    let socket = TcpListener::bind("127.0.0.1:0").unwrap();
    let origin = format!("http://{}", socket.local_addr().unwrap());
    let server_origin = origin.clone();
    let fixture = std::thread::spawn(move || {
        for client in socket.incoming().take(4) {
            let mut client = client.unwrap();
            let mut line = String::new();
            BufReader::new(&client).read_line(&mut line).unwrap();
            let body = if line.starts_with("GET /a ") {
                format!("Station A,{server_origin}/channel?token=rotated\n")
            } else {
                format!("Station B,{server_origin}/events\n")
            };
            write!(
                client,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
        }
    });
    let sources = ["a","b"].map(|id| json!({"id":id,"group":id,"playlist":format!("{origin}/{id}"),"format":"hls","origins":[origin]}));
    let profiles = [("home-a", "a", "b"), ("home-b", "b", "a")].map(|(name, local, remote)| {
        json!({
            "name":name,"source_order":[local,remote],"relay_sources":[remote],"relay_base":origin,
            "epg_url":format!("{origin}/epg")
        })
    });
    fs::write(root.join("relay.json"), json!({"listeners":[{"bind":"127.0.0.1:0","public_base":origin}],"sources":sources,"profiles":profiles}).to_string()).unwrap();
    let update = || {
        Command::new(env!("CARGO_BIN_EXE_iptv-rs"))
            .current_dir(root)
            .env_clear()
            .args(["update", "--relay-config", "relay.json"])
            .output()
            .unwrap()
    };
    let result = update();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let read = |name: &str| fs::read_to_string(root.join(format!("output/{name}.m3u"))).unwrap();
    let before = [read("home-a"), read("home-b")];
    for (list, local, remote) in [(&before[0], a, "b"), (&before[1], b, "a")] {
        let lines: Vec<_> = list.lines().collect();
        assert_eq!(lines.len(), 5);
        assert_eq!(lines[0], format!("#EXTM3U x-tvg-url=\"{origin}/epg\""));
        assert_eq!(lines[1], lines[3]);
        assert!(lines[1].ends_with("group-title=\"News stations\",News"));
        assert_eq!(lines[2], local);
        assert!(
            lines[4].starts_with(&format!("{origin}/live/{remote}/"))
                && lines[4].ends_with(".m3u8")
        );
    }
    assert!(read("result").contains(a) && read("result").contains(b));
    fs::write(root.join("b.txt"), "").unwrap();
    assert!(!update().status.success());
    assert_eq!([read("home-a"), read("home-b")], before);
    fixture.join().unwrap();
}
