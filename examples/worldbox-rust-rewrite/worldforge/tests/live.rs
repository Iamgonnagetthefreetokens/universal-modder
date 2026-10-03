//! The live page over a real socket: what a browser gets, without a browser.
//!
//! The unit tests in `live.rs` check the frames; this checks the thing a person
//! actually does — open the address, get a page, poll the state, cast a power —
//! and that the simulation keeps running between requests.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use worldforge::json;
use worldforge::live::Watcher;
use worldforge::world::World;
use worldforge::worldgen::GenParams;

fn small_world() -> World {
    let params = GenParams {
        width: 40,
        height: 28,
        seed: 7,
        ..Default::default()
    };
    let mut world = World::generate(params);
    world.seed_life(4, 8, 0);
    world
}

/// `(status line + headers, body)`, the way a browser sees it.
fn get(port: u16, path: &str) -> (String, String) {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect to the live server");
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("timeout");
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"
    )
    .expect("write request");
    let mut buf = String::new();
    stream.read_to_string(&mut buf).expect("read response");
    let (head, body) = buf.split_once("\r\n\r\n").expect("a complete response");
    (head.to_string(), body.to_string())
}

fn serve(world: World, tps: f32) -> (u16, Arc<AtomicBool>, std::thread::JoinHandle<()>) {
    let watcher = Watcher::bind("127.0.0.1", 0, world, tps).expect("bind an ephemeral port");
    let port = watcher.port();
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    let handle = std::thread::spawn(move || {
        let mut watcher = watcher;
        watcher.serve(&flag);
    });
    (port, stop, handle)
}

#[test]
fn a_browser_gets_the_page_and_the_page_can_reach_itself() {
    let (port, stop, handle) = serve(small_world(), 60.0);

    let (head, body) = get(port, "/");
    assert!(head.starts_with("HTTP/1.1 200"), "{head}");
    assert!(head.contains("text/html"), "{head}");
    assert!(
        !head.to_lowercase().contains("x-frame-options"),
        "a preview proxy must be allowed to embed this page: {head}"
    );
    assert!(body.contains("<canvas"), "the page needs its map canvas");
    assert!(body.contains("worldforge"), "and its own name");
    assert!(body.contains("Chronicle"), "and the panel the user came for");

    let (head, body) = get(port, "/state");
    assert!(head.starts_with("HTTP/1.1 200"), "{head}");
    let state = json::parse(&body).expect("state must be valid JSON");
    assert!(state.get("tick").is_some() && state.get("wars").is_some());

    let (head, body) = get(port, "/terrain");
    assert!(head.starts_with("HTTP/1.1 200"), "{head}");
    let terrain = json::parse(&body).expect("terrain must be valid JSON");
    assert_eq!(terrain.get("w").unwrap().as_i64().unwrap(), 40);
    assert_eq!(
        terrain.get("biome").unwrap().as_str().unwrap().len(),
        40 * 28
    );

    let (head, body) = get(port, "/territory");
    assert!(head.starts_with("HTTP/1.1 200"), "{head}");
    assert_eq!(
        json::parse(&body)
            .unwrap()
            .get("owner")
            .unwrap()
            .as_str()
            .unwrap()
            .len(),
        40 * 28
    );

    let (head, _) = get(port, "/nope");
    assert!(head.starts_with("HTTP/1.1 404"), "{head}");

    stop.store(true, Ordering::Relaxed);
    handle.join().expect("the serve loop stops cleanly");
}

#[test]
fn the_world_runs_between_requests_and_takes_orders() {
    let (port, stop, handle) = serve(small_world(), 240.0);

    let tick_of = |port: u16| -> i64 {
        let (_, body) = get(port, "/state");
        json::parse(&body).unwrap().get("tick").unwrap().as_i64().unwrap()
    };

    let first = tick_of(port);
    std::thread::sleep(Duration::from_millis(250));
    let second = tick_of(port);
    assert!(
        second > first,
        "the simulation should advance while serving: {first} then {second}"
    );

    // pause: the page's button, exactly
    let (_, body) = get(port, "/cmd?pause=1");
    assert!(json::parse(&body).unwrap().get("ok").unwrap().as_bool().unwrap());
    let paused_at = tick_of(port);
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(tick_of(port), paused_at, "paused means paused");

    // step by hand, then cast a power at a tile
    get(port, "/cmd?step=25");
    assert_eq!(tick_of(port), paused_at + 25);
    let (_, body) = get(port, "/cmd?power=0&col=8&row=8");
    assert!(json::parse(&body).unwrap().get("ok").unwrap().as_bool().unwrap());
    let (_, body) = get(port, "/cmd?spawn=1&col=9&row=9");
    assert!(json::parse(&body).unwrap().get("ok").unwrap().as_bool().unwrap());
    let (_, body) = get(port, "/cmd?tps=30&resume=1");
    assert!(json::parse(&body).unwrap().get("ok").unwrap().as_bool().unwrap());

    stop.store(true, Ordering::Relaxed);
    handle.join().expect("the serve loop stops cleanly");
}
