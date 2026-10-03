//! Conformance tests for `um-bridge`.
//!
//! Every expectation here is a vector produced by the Python reference implementation
//! (`fakes/test_bridge.py --write-vectors`), so this crate and the fakes cannot drift apart without a
//! failing test: run `python3 fakes/test_bridge.py --write-vectors` after any protocol change, then
//! `cargo test -p um-bridge`.
//!
//! These tests also run without either game, which is the point: the protocol is the contract, and it
//! is checked on both sides long before a GPU or a 40 GB install is involved.

use std::path::PathBuf;

use um_bridge::control::Msg;
use um_bridge::mapping::{self, Cell, TerrainHeader};
use um_bridge::protocol::{self, Entity, Event, FrameHeader, StateHeader};
use um_bridge::region::{Region, FLAG_WRITER_ALIVE};
use um_bridge::{Role, Watchdog, WatchdogState, BridgeConfig, PROTOCOL_VERSION};

#[path = "../testdata/vectors.rs"]
mod vectors;

fn hex_to_bytes(hex: &str) -> Vec<u8> {
    (0..hex.len() / 2)
        .map(|i| u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).expect("hex"))
        .collect()
}

fn approx(a: f32, b: f32, tol: f32) -> bool {
    (a - b).abs() <= tol
}

fn temp_path(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("um-bridge-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir.join(name)
}

#[test]
fn sizes_match_the_spec() {
    assert_eq!(protocol::STATE_HEADER_SIZE, 32);
    assert_eq!(protocol::ENTITY_SIZE, 40);
    assert_eq!(protocol::EVENT_SIZE, 24);
    assert_eq!(protocol::FRAME_HEADER_SIZE, 32);
    assert_eq!(mapping::TERRAIN_HEADER_SIZE, 32);
    assert_eq!(mapping::CELL_SIZE, 8);
    assert_eq!(um_bridge::region::HEADER_SIZE, 64);
    assert_eq!(um_bridge::region::COMMIT_LEN, 28);
    assert_eq!(vectors::PROTOCOL_VERSION, PROTOCOL_VERSION);
    assert_eq!(um_bridge::MAGIC, vectors::MAGIC);
}

#[test]
fn entity_bytes_match_the_python_reference() {
    // The reference packed this entity and hex-encoded it; we must produce the same bytes.
    let e = Entity {
        id: 0x0102_0304,
        kind: 2,
        team: 3,
        flags: 0b101,
        pad: 0,
        x: 1.0,
        y: 2.0,
        z: 3.0,
        angle: 4.0,
        turret_angle: 5.0,
        vx: 6.0,
        vy: 7.0,
        hp_frac: 0.5,
    };
    let mut buf = [0u8; protocol::ENTITY_SIZE];
    e.write_to(&mut buf, 0);
    assert_eq!(hex::encode(&buf), vectors::ENTITY_SAMPLE_HEX);
    // and reading those bytes back gives the same entity
    assert_eq!(Entity::read_from(&buf, 0), e);
}

#[test]
fn state_roundtrip_and_payload_size() {
    let entities: Vec<Entity> = (0..5)
        .map(|i| Entity {
            id: i,
            kind: protocol::ENTITY_TANK,
            team: (i % 2) as u8,
            flags: 1,
            x: i as f32 * 1.5,
            y: -(i as f32) * 2.0,
            hp_frac: 1.0,
            ..Entity::default()
        })
        .collect();
    let events = vec![
        Event {
            kind: protocol::EV_EXPLOSION,
            arg16: 48,
            t_ms: 100,
            x: 3.0,
            y: 4.0,
            ..Event::default()
        };
        3
    ];
    let header = StateHeader {
        tick: 42,
        game_time_ms: 700,
        ..StateHeader::default()
    };
    let packed = protocol::pack_state(header, &entities, &events);
    assert_eq!(
        packed.len(),
        protocol::STATE_HEADER_SIZE + 5 * protocol::ENTITY_SIZE + 3 * protocol::EVENT_SIZE
    );
    let (h2, e2, v2) = protocol::unpack_state(&packed).expect("parse");
    assert_eq!(h2.ent_count, 5);
    assert_eq!(h2.event_count, 3);
    assert_eq!(h2.tick, 42);
    assert_eq!(e2[3].x, 4.5);
    assert_eq!(v2[2].t_ms, 102);
    // the reference's own bytes parse too
    let sample = hex_to_bytes(vectors::STATE_SAMPLE_HEX);
    let (h3, e3, _) = protocol::unpack_state(&sample).expect("parse reference bytes");
    assert_eq!(h3.tick, 42);
    assert_eq!(e3.len(), 5);
}

#[test]
fn events_are_applied_once() {
    let a = Event {
        kind: protocol::EV_EXPLOSION,
        t_ms: 1234,
        x: 10.0,
        y: 20.0,
        ..Event::default()
    };
    let mut b = a;
    assert_eq!(a.once_key(), b.once_key());
    b.t_ms += 1;
    assert_ne!(a.once_key(), b.once_key());
}

#[test]
fn frame_roundtrip_and_limits() {
    let pixels = vec![7u8; 16 * 8 * 4];
    let header = FrameHeader::with_geometry(16, 8, 500, 123_456);
    let packed = protocol::pack_frame(&header, &pixels).expect("pack");
    let (h2, p2) = protocol::unpack_frame(&packed).expect("parse");
    assert_eq!((h2.width, h2.height, h2.stride), (16, 8, 64));
    assert_eq!(h2.publish_ts_us, 123_456);
    assert_eq!(p2, &pixels[..]);
    // an oversized frame is an error, not a silent truncation
    let big = FrameHeader::with_geometry(4000, 8, 1000, 0);
    assert!(protocol::pack_frame(&big, &vec![0u8; 8]).is_err());
}

#[test]
fn mapping_positions_match_the_python_reference() {
    let t = TerrainHeader {
        origin_x: 1000.0,
        origin_y: 2000.0,
        cell_m: 8.0,
        nx: 64,
        ny: 64,
        tile_m: 8.0,
        ..TerrainHeader::default()
    };
    assert!(approx(t.origin_y_max(), 2512.0, 1e-3));
    assert!(approx(mapping::U2M, vectors::CONSTANTS.2, 1e-6));
    for (gx, gy, hx, hy, ci, cj) in vectors::POSITIONS.iter().copied() {
        let (x, y) = mapping::guest_to_host(gx, gy, &t);
        assert!(approx(x, hx, 1e-3), "guest_to_host x({gx},{gy}) = {x}, want {hx}");
        assert!(approx(y, hy, 1e-3), "guest_to_host y({gx},{gy}) = {y}, want {hy}");
        let (gx2, gy2) = mapping::host_to_guest(x, y, &t);
        assert!(approx(gx2, gx, 1e-2) && approx(gy2, gy, 1e-2), "round trip");
        assert_eq!(t.cell_at(x, y), (ci, cj), "cell_at({gx},{gy})");
    }
}

#[test]
fn mapping_angles_match_the_python_reference() {
    for (theta, yaw, dx, dy) in vectors::ANGLES.iter().copied() {
        let got = mapping::guest_angle_to_yaw(theta);
        assert!(
            approx(got, yaw, 1e-5),
            "guest_angle_to_yaw({theta}) = {got}, want {yaw}"
        );
        let (ax, ay) = mapping::yaw_to_dir(got);
        assert!(
            approx(ax, dx, 1e-5) && approx(ay, dy, 1e-5),
            "yaw_to_dir({got}) = ({ax},{ay}), want ({dx},{dy})"
        );
        assert!(
            approx(mapping::yaw_to_guest_angle(got), theta, 1e-5),
            "guest angle round trip"
        );
        // the guest's angle points along (cos, sin) in its y-down frame, and that is the same
        // direction on the host: this is the check that catches a mirrored or rotated wiring
        let guest_dir = (theta.cos(), theta.sin());
        let host_dir = (dx, -dy); // flip y: guest y grows down, host y grows up
        assert!(
            approx(guest_dir.0, host_dir.0, 1e-5) && approx(guest_dir.1, host_dir.1, 1e-5),
            "guest dir {guest_dir:?} vs host dir {host_dir:?} for theta {theta}"
        );
    }
}

#[test]
fn classification_matches_the_python_reference() {
    let nx = vectors::CLASSIFICATION_NX;
    let ny = vectors::CLASSIFICATION_NY;
    let mut cells = vec![Cell::default(); nx * ny];
    for (i, j, height, _kind) in vectors::CLASSIFICATION.iter().copied() {
        cells[j as usize * nx + i as usize].height_m = height;
    }
    for (i, j, _height, kind) in vectors::CLASSIFICATION.iter().copied() {
        let got = mapping::classify_cell(&cells, i as usize, j as usize, nx, ny, 0.0, 8.0);
        assert_eq!(got, kind, "cell ({i},{j})");
    }
    assert_eq!(mapping::guest_surface_for(protocol::CELL_CLIFF), "wall");
    assert_eq!(mapping::guest_surface_for(protocol::CELL_UNKNOWN), "wall");
    assert_eq!(mapping::guest_surface_for(protocol::CELL_WATER), "water");
}

#[test]
fn terrain_roundtrip_and_limits() {
    let t = TerrainHeader {
        origin_x: -100.0,
        origin_y: 50.0,
        nx: 8,
        ny: 8,
        revision: 3,
        ..TerrainHeader::default()
    };
    let cells: Vec<Cell> = (0..64)
        .map(|i| Cell::new(i as f32, protocol::CELL_GRASS))
        .collect();
    let packed = mapping::pack_terrain(&t, &cells).expect("pack");
    assert_eq!(packed.len(), mapping::TERRAIN_HEADER_SIZE + 64 * mapping::CELL_SIZE);
    let (t2, c2) = mapping::unpack_terrain(&packed).expect("parse");
    assert_eq!((t2.nx, t2.ny, t2.revision), (8, 8, 3));
    assert_eq!(c2.len(), 64);
    assert_eq!(c2[10].height_m, 10.0);
    // a wrong cell count is an error
    assert!(mapping::pack_terrain(&t, &cells[..32]).is_err());
}

#[test]
fn regions_commit_read_and_reject_corruption() {
    let path = temp_path("state.region");
    let mut writer = Region::create(&path, protocol::REGION_STATE, 4096).expect("create");
    let mut reader = Region::open(&path, protocol::REGION_STATE).expect("open");

    assert!(reader.writer_alive());
    assert!(reader.read().expect("read").is_none(), "nothing committed yet");

    let packed = protocol::pack_state(
        StateHeader {
            tick: 1,
            ..StateHeader::default()
        },
        &[Entity {
            id: 1,
            kind: protocol::ENTITY_TANK,
            flags: 1,
            x: 5.0,
            ..Entity::default()
        }],
        &[],
    );
    let seq1 = writer.write(&packed).expect("write");
    assert_eq!(seq1, 1);
    let res = reader.read().expect("read").expect("committed");
    assert_eq!(res.seq, 1);
    let (h, ents, _) = protocol::unpack_state(&res.payload).expect("parse");
    assert_eq!(h.tick, 1);
    assert_eq!(ents[0].x, 5.0);

    // second commit lands in the other slot and is readable
    let packed2 = protocol::pack_state(
        StateHeader {
            tick: 2,
            ..StateHeader::default()
        },
        &[],
        &[],
    );
    assert_eq!(writer.write(&packed2).expect("write"), 2);
    let res2 = reader.read().expect("read").expect("committed");
    assert_eq!(res2.seq, 2);
    assert_eq!(protocol::unpack_state(&res2.payload).expect("parse").0.tick, 2);

    // corrupting the committed payload must be caught by the CRC, not returned as data
    {
        use std::io::{Seek, SeekFrom, Write};
        let mut f = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .expect("reopen");
        f.seek(SeekFrom::Start(
            um_bridge::region::HEADER_SIZE as u64 + 3,
        ))
        .expect("seek");
        f.write_all(&[0xFF]).expect("corrupt");
        f.flush().expect("flush");
    }
    assert!(reader.read().is_err(), "corruption must be reported");

    // a clean close clears the writer bit, so the peer can say "offline" instead of "lost"
    writer.close_clean();
    assert!(!reader.writer_alive());
}

#[test]
fn region_rejects_a_wrong_kind() {
    let path = temp_path("frame.region");
    let _writer = Region::create(&path, protocol::REGION_FRAME, 256).expect("create");
    assert!(Region::open(&path, protocol::REGION_TERRAIN).is_err());
}

#[test]
fn control_messages_roundtrip() {
    let m = Msg::new("CAM")
        .with("x", "1032.500")
        .with("yaw", "-1.57080")
        .with("sx", "1600");
    let line = m.encode();
    assert!(line.ends_with('\n'));
    let parsed = Msg::parse(&line).expect("parse");
    assert_eq!(parsed.kind, "CAM");
    assert!(approx(parsed.get_f32("x"), 1032.5, 1e-3));
    assert!(approx(parsed.get_f32("yaw"), -1.5708, 1e-5));
    assert_eq!(parsed.get_i64("sx"), 1600);
    // unknown fields and unknown message types are kept, never fatal
    let odd = Msg::parse("WHAT v=1 who=someone").expect("parse");
    assert_eq!(odd.kind, "WHAT");
    assert_eq!(odd.get("who"), "someone");
    assert!(Msg::parse("   ").is_none());
}

#[test]
fn watchdog_matches_the_reference_timings() {
    let (stale, dead) = vectors::WATCHDOG;
    assert!(approx(stale as f32, 600.0, 0.1) && approx(dead as f32, 2000.0, 0.1));
    let mut w = Watchdog::new(stale, dead);
    assert_eq!(w.state(), WatchdogState::Ok);
    w.backdate_ms(stale as u64 + 100);
    assert_eq!(w.state(), WatchdogState::Stale);
    w.backdate_ms(dead as u64 + 100);
    assert_eq!(w.state(), WatchdogState::Dead);
    w.beat();
    assert_eq!(w.state(), WatchdogState::Ok);
    assert!(!w.is_dead());
}

#[test]
fn bridge_config_defaults_are_sane() {
    let c = BridgeConfig::default();
    assert_eq!(c.port, um_bridge::DEFAULT_PORT);
    assert!(c.state_region_size >= protocol::STATE_HEADER_SIZE);
    assert!(c.terrain_region_size >= mapping::TERRAIN_HEADER_SIZE);
    assert!(c.run_dir.to_string_lossy().contains("um-bridge"));
    // the two roles exist and are distinct
    assert_ne!(Role::Host, Role::Guest);
    let _ = FLAG_WRITER_ALIVE;
}
