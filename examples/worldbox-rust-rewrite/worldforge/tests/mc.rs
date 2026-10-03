//! End-to-end tests for the Minecraft bridge, over a real loopback socket.
//!
//! These are the tests that matter for the bridge: they prove that a *client*
//! (a stand-in for the Fabric mod) joining a *running* simulation sees the world,
//! can poke it, and gets the consequences back -- all through the wire format the
//! mod actually speaks.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use worldforge::bridge::{self, DEFAULT_PORT};
use worldforge::mcworld::{parse_message, BlockWorld, IsoOpts, Message};
use worldforge::serve::Server;
use worldforge::world::World;

/// Read lines until one satisfies `want`, or the deadline passes. Never blocks
/// forever, so a broken bridge fails the test instead of hanging it.
fn read_until<F>(reader: &mut BufReader<TcpStream>, mut want: F, deadline: Instant) -> Option<String>
where
    F: FnMut(&str) -> bool,
{
    let mut line = String::new();
    while Instant::now() < deadline {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => return None,
            Ok(_) => {
                let text = line.trim();
                if want(text) {
                    return Some(text.to_string());
                }
            }
            Err(_) => continue, // read timeout: try again until the deadline
        }
    }
    None
}

fn connect(port: u16) -> BufReader<TcpStream> {
    let stream = TcpStream::connect(("127.0.0.1", port)).expect("connect to the bridge");
    stream
        .set_read_timeout(Some(Duration::from_millis(100)))
        .expect("read timeout");
    BufReader::new(stream)
}

fn send(reader: &BufReader<TcpStream>, line: &str) {
    let mut out = reader.get_ref().try_clone().expect("clone socket");
    out.write_all(line.as_bytes()).expect("write");
    out.write_all(b"\n").expect("write newline");
    out.flush().expect("flush");
}

fn seeded_world() -> World {
    let mut world = World::new(24, 16, 20241003, worldforge::worldgen::WorldType::Continents);
    world.seed_life(2, 8, 0);
    world.step_n(60);
    world
}

#[test]
fn a_client_sees_the_world_and_can_poke_it() {
    let stop = Arc::new(AtomicBool::new(false));
    let server = Server::bind_local(0, 60.0).expect("bind");
    let port = server.port();
    assert_ne!(port, 0);
    let handle = server.start(seeded_world(), Arc::clone(&stop));
    let deadline = Instant::now() + Duration::from_secs(10);

    let mut reader = connect(port);

    // 1. hello, first message on the wire
    let hello = read_until(
        &mut reader,
        |line| line.contains("\"t\":\"hello\""),
        deadline,
    )
    .expect("a hello");
    let hello = parse_message(&hello).unwrap();
    let Message::Hello {
        protocol,
        size,
        base_y,
        palette,
        tag,
        ..
    } = hello
    else {
        panic!("first message should be hello");
    };
    assert_eq!(protocol, bridge::PROTOCOL);
    assert_eq!(size, (24, 16));
    assert_eq!(base_y, bridge::BASE_Y);
    assert_eq!(tag, bridge::ENTITY_TAG);
    assert_eq!(palette.len(), bridge::PALETTE.len());

    // 2. the tile field: one column per tile
    let tiles = read_until(
        &mut reader,
        |line| line.contains("\"t\":\"tiles\""),
        deadline,
    )
    .expect("a tile field");
    let Message::Tiles { columns } = parse_message(&tiles).unwrap() else {
        panic!("expected tiles");
    };
    assert_eq!(columns.len(), 24 * 16);

    // 3. build the block world exactly as the mod would
    let mut blocks = BlockWorld::new(size.0, size.1, bridge::BASE_Y, bridge::SEA_LEVEL, bridge::CANVAS_TOP);
    blocks.apply(&Message::Tiles { columns });

    // 4. frames keep coming, with units in them
    let mut frames = 0;
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut last_tick = 0;
    while frames < 3 {
        let Some(line) = read_until(
            &mut reader,
            |line| line.contains("\"t\":\"frame\""),
            deadline,
        ) else {
            panic!("only {frames} frames arrived");
        };
        let message = parse_message(&line).unwrap();
        if let Message::Frame { info, .. } = &message {
            assert!(info.tick >= last_tick, "ticks only move forward");
            last_tick = info.tick;
            assert!(info.pop > 0, "a seeded world has people");
            assert!(info.hash.starts_with("0x"));
        }
        blocks.apply(&message);
        frames += 1;
    }
    assert!(!blocks.units.is_empty(), "units are placed as mobs");
    assert!(blocks.blocks_set > 500, "the ground is built ({} blocks)", blocks.blocks_set);

    // 5. poke it: a nuke must come back as edits
    let before = blocks.messages;
    send(
        &reader,
        "{\"t\":\"power\",\"name\":\"nuke\",\"col\":12,\"row\":8}",
    );
    let edits = read_until(
        &mut reader,
        |line| line.contains("\"t\":\"edits\""),
        Instant::now() + Duration::from_secs(5),
    )
    .expect("edits after a nuke");
    let Message::Edits { tiles } = parse_message(&edits).unwrap() else {
        panic!("expected edits");
    };
    assert!(!tiles.is_empty(), "a nuke changes tiles");
    blocks.apply(&Message::Edits { tiles });
    assert!(blocks.messages > before);

    // 6. pause stops the clock
    send(&reader, "{\"t\":\"pause\",\"on\":true}");
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut settled = 0;
    let mut tick = 0;
    while settled < 3 {
        let Some(line) = read_until(
            &mut reader,
            |line| line.contains("\"t\":\"frame\""),
            deadline,
        ) else {
            panic!("frames stopped arriving while paused");
        };
        if let Message::Frame { info, .. } = parse_message(&line).unwrap() {
            if info.tick == tick {
                settled += 1;
            } else {
                tick = info.tick;
                settled = 0;
            }
        }
    }

    // 7. blocking on a socket for a client that stopped reading must not kill it
    stop.store(true, Ordering::Relaxed);
    handle.join().expect("the sim thread joins");
}

#[test]
fn a_client_can_render_what_it_receives() {
    let stop = Arc::new(AtomicBool::new(false));
    let server = Server::bind_local(0, 60.0).expect("bind");
    let port = server.port();
    let handle = server.start(seeded_world(), Arc::clone(&stop));
    let deadline = Instant::now() + Duration::from_secs(10);

    let mut reader = connect(port);
    let mut blocks: Option<BlockWorld> = None;
    let mut frames = 0;
    while frames < 5 {
        let Some(line) = read_until(
            &mut reader,
            |line| {
                line.contains("\"t\":\"hello\"")
                    || line.contains("\"t\":\"tiles\"")
                    || line.contains("\"t\":\"structures\"")
                    || line.contains("\"t\":\"frame\"")
            },
            deadline,
        ) else {
            panic!("the bridge went quiet");
        };
        let message = parse_message(&line).unwrap();
        if let Message::Hello {
            size,
            base_y,
            sea_level,
            canvas_top,
            ..
        } = &message
        {
            blocks = Some(BlockWorld::new(size.0, size.1, *base_y, *sea_level, *canvas_top));
        }
        let world = blocks.as_mut().expect("hello first");
        world.apply(&message);
        if matches!(message, Message::Frame { .. }) {
            frames += 1;
        }
    }

    let world = blocks.expect("a block world");
    let img = world.render_iso(&IsoOpts {
        scale: 6,
        ..IsoOpts::default()
    });
    assert!(img.width > 100 && img.height > 100);
    let bytes = worldforge::png::encode_png(&img);
    assert_eq!(
        worldforge::png::probe_png(&bytes),
        Some((img.width, img.height)),
        "the picture is a real PNG"
    );

    stop.store(true, Ordering::Relaxed);
    handle.join().expect("the sim thread joins");
}

#[test]
fn a_second_client_can_join_while_the_first_is_running() {
    let stop = Arc::new(AtomicBool::new(false));
    let server = Server::bind_local(0, 60.0).expect("bind");
    let port = server.port();
    let handle = server.start(seeded_world(), Arc::clone(&stop));

    let mut first = connect(port);
    let deadline = Instant::now() + Duration::from_secs(10);
    read_until(&mut first, |l| l.contains("\"t\":\"frame\""), deadline)
        .expect("the first client sees a frame");

    // a client that connects and immediately vanishes must not take the bridge down
    drop(connect(port));

    let mut second = connect(port);
    let deadline = Instant::now() + Duration::from_secs(10);
    let hello = read_until(&mut second, |l| l.contains("\"t\":\"hello\""), deadline)
        .expect("the second client is served");
    assert!(hello.contains("\"protocol\""));

    stop.store(true, Ordering::Relaxed);
    handle.join().expect("the sim thread joins");
}

#[test]
fn junk_commands_do_not_upset_the_bridge() {
    let stop = Arc::new(AtomicBool::new(false));
    let server = Server::bind_local(0, 60.0).expect("bind");
    let port = server.port();
    let handle = server.start(seeded_world(), Arc::clone(&stop));
    let mut reader = connect(port);

    for junk in [
        "not json at all",
        "{}",
        "{\"t\":\"power\",\"name\":\"no_such_power\",\"col\":1,\"row\":1}",
        "{\"t\":\"spawn\",\"race\":\"Kraken\",\"col\":1,\"row\":1}",
        "{\"t\":\"speed\",\"tps\":\"fast\"}",
    ] {
        send(&reader, junk);
    }
    // the bridge keeps streaming: a good client still gets frames
    let deadline = Instant::now() + Duration::from_secs(5);
    read_until(&mut reader, |l| l.contains("\"t\":\"frame\""), deadline).expect("still alive");

    // and a real command still works afterwards
    send(
        &reader,
        "{\"t\":\"spawn\",\"race\":\"Orc\",\"soldier\":true,\"col\":12,\"row\":8}",
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut seen = false;
    while !seen {
        let Some(line) = read_until(&mut reader, |l| l.contains("\"t\":\"frame\""), deadline) else {
            panic!("no frames after a spawn");
        };
        if let Message::Frame { units, .. } = parse_message(&line).unwrap() {
            seen = units
                .iter()
                .any(|u| u.name.contains("Orc") || u.mob == bridge::block_id("pillager"));
        }
    }

    stop.store(true, Ordering::Relaxed);
    handle.join().expect("the sim thread joins");
}

#[test]
fn every_line_on_the_wire_is_json() {
    // A debug string once slipped into the stream: a client that trusts the
    // protocol must never see anything but JSON.
    let stop = Arc::new(AtomicBool::new(false));
    let server = Server::bind_local(0, 60.0).expect("bind");
    let port = server.port();
    let handle = server.start(seeded_world(), Arc::clone(&stop));
    let mut reader = connect(port);

    // Say everything a client can say, then watch the reply stream for a while.
    for command in worldforge::bridge::EXAMPLE_COMMANDS {
        send(&reader, command);
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut lines = 0;
    let mut line = String::new();
    while Instant::now() < deadline {
        line.clear();
        match std::io::BufRead::read_line(&mut reader, &mut line) {
            Ok(0) => break,
            Ok(_) => {
                let text = line.trim();
                if text.is_empty() {
                    panic!("the bridge sent a blank line");
                }
                if let Err(e) = worldforge::json::parse(text) {
                    panic!("the bridge sent a non-JSON line: {e} in `{text}`");
                }
                lines += 1;
            }
            Err(_) => continue,
        }
    }
    assert!(lines > 5, "the bridge sent {lines} lines in five seconds");
    stop.store(true, Ordering::Relaxed);
    handle.join().expect("the sim thread joins");
}

#[test]
fn a_refused_command_says_why() {
    // Spawning on something that is not walkable must come back as a notice, not
    // as silence: the mod shows those in chat.
    let stop = Arc::new(AtomicBool::new(false));
    let server = Server::bind_local(0, 60.0).expect("bind");
    let port = server.port();
    let handle = server.start(seeded_world(), Arc::clone(&stop));
    let mut reader = connect(port);
    let deadline = Instant::now() + Duration::from_secs(10);

    // The tile field arrives once, on connect: keep it.
    let mut columns = None;
    while columns.is_none() {
        let Some(line) = read_until(&mut reader, |l| l.contains("\"t\":\"tiles\""), deadline) else {
            panic!("no tile field");
        };
        if let Message::Tiles { columns: parsed } = parse_message(&line).unwrap() {
            columns = Some(parsed);
        }
    }
    let columns = columns.unwrap();
    let water = columns
        .iter()
        .position(|c| bridge::block_name(c.surface) == "water")
        .expect("deep water somewhere on a continents map");
    let (col, row) = (water as i32 % 24, water as i32 / 24);

    send(
        &reader,
        &format!("{{\"t\":\"spawn\",\"race\":\"Human\",\"col\":{col},\"row\":{row}}}"),
    );
    let notice = read_until(&mut reader, |l| l.contains("\"t\":\"notice\""), deadline)
        .expect("a notice for a refused spawn");
    let Message::Notice { text } = parse_message(&notice).unwrap() else {
        panic!("expected a notice");
    };
    assert!(text.contains("Human"), "the notice names the race: {text}");
    assert!(text.contains("walkable"), "the notice says why: {text}");

    // a ping is answered too, on the same connection
    send(&reader, "{\"t\":\"ping\"}");
    let pong = read_until(&mut reader, |l| l.contains("\"t\":\"pong\""), deadline).expect("a pong");
    assert!(pong.contains("\"tick\""), "the pong carries the tick: {pong}");

    stop.store(true, Ordering::Relaxed);
    handle.join().expect("the sim thread joins");
}

#[test]
fn a_frame_describes_its_villages_once() {
    // The kingdoms/villages *counts* and the villages/kingdoms *arrays* once shared
    // a key, and the array silently disappeared. This is the guard.
    let stop = Arc::new(AtomicBool::new(false));
    let server = Server::bind_local(0, 60.0).expect("bind");
    let port = server.port();
    let handle = server.start(seeded_world(), Arc::clone(&stop));
    let mut reader = connect(port);
    let deadline = Instant::now() + Duration::from_secs(10);

    let mut checked = false;
    while !checked {
        let Some(line) = read_until(
            &mut reader,
            |line| line.contains("\"t\":\"frame\""),
            deadline,
        ) else {
            panic!("no frames arrived");
        };
        if let Message::Frame {
            info,
            villages,
            kingdoms,
            ..
        } = parse_message(&line).unwrap()
        {
            assert_eq!(
                villages.len() as u32,
                info.villages,
                "every living village is in the frame"
            );
            assert_eq!(kingdoms.len() as u32, info.kingdoms);
            if info.villages > 0 {
                assert!(!villages[0].name.is_empty(), "villages have names");
                checked = true;
            }
        }
    }

    stop.store(true, Ordering::Relaxed);
    handle.join().expect("the sim thread joins");
}

#[test]
fn the_default_port_is_the_one_documented() {
    assert_eq!(DEFAULT_PORT, 25607);
}
