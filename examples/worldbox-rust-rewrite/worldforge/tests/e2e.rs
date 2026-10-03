//! End-to-end tests: the library, the save codec, the renderer, the PNG writer
//! and the actual `worldforge` binary, exercised the way a user would use them.

use std::process::Command;

use worldforge::png::probe_png;
use worldforge::powers::Power;
use worldforge::render::{color_map, render_ascii, RenderOpts};
use worldforge::save::{decode, encode, load_from_file, save_to_file};
use worldforge::script;
use worldforge::world::World;
use worldforge::worldgen::{GenParams, WorldType};

const SCENARIO: &str = "\
gen 64 40 2024 type pangaea land 62
step 20y
cast rain @random
cast volcano @random
spawn elf leader @random
spawn dragon monster @random
step 5y
hash
summary
";

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_worldforge"))
}

#[test]
fn a_scenario_is_reproducible_from_the_script_alone() {
    let a = script::run(SCENARIO).expect("scenario runs");
    let b = script::run(SCENARIO).expect("scenario runs again");
    assert_eq!(a.world.state_hash(), b.world.state_hash());
    assert_eq!(a.output, b.output);
    assert!(a.world.year >= 25, "25 years should have passed");
    assert!(a.output.contains("hash 0x"));
}

#[test]
fn the_world_keeps_evolving_after_a_save_and_load() {
    let mut w = World::generate(GenParams::new(48, 32, 7, WorldType::Continents));
    w.step_n(300);
    let mut loaded = decode(&encode(&w)).expect("save decodes");
    for _ in 0..500 {
        w.step();
        loaded.step();
    }
    assert_eq!(loaded.state_hash(), w.state_hash());
    assert_eq!(loaded.year, w.year);
}

#[test]
fn long_runs_stay_inside_their_invariants() {
    let mut w = World::generate(GenParams::new(72, 48, 1234, WorldType::Continents));
    w.step_n(1000);
    for i in 0..12 {
        let at = w.random_land_tile().expect("there is land");
        let power = Power::ALL[i * 4 % Power::ALL.len()];
        w.cast(power, at);
        w.step_n(250);
        check_invariants(&w);
    }
    assert!(w.tick >= 1000 + 12 * 250);
    assert!(w.stats.powers_cast >= 12);
    // The world is still worth playing: something is alive or at least it is
    // still ticking without panicking, and every tile is still legal.
    assert!(w.summary().contains("year"));
}

fn check_invariants(w: &World) {
    assert_eq!(w.tiles.len(), w.width as usize * w.height as usize);
    for (i, t) in w.tiles.iter().enumerate() {
        let h = w.hex_at_index(i);
        assert_eq!(w.idx(h), Some(i), "index and hex disagree at {i}");
        assert!((-320..=900).contains(&t.elevation), "elevation drift at {h}");
        if let Some(owner) = t.owner {
            assert!(
                w.village(owner).is_some(),
                "tile {h} is owned by a village that does not exist"
            );
        }
    }
    for u in w.units.iter().filter(|u| u.alive) {
        assert!(
            w.tile(u.pos).is_some(),
            "unit {} is off the map at {}",
            u.id,
            u.pos
        );
        if let Some(vid) = u.village {
            assert!(w.village(vid).is_some(), "unit {} lost its village", u.id);
        }
        if let Some(kid) = u.kingdom {
            assert!(w.kingdom(kid).is_some(), "unit {} lost its kingdom", u.id);
        }
    }
    for v in w.villages.iter().filter(|v| v.alive) {
        assert!(w.tile(v.center).is_some(), "village {} floated away", v.id);
        if let Some(kid) = v.kingdom {
            assert!(
                w.kingdom(kid).is_some(),
                "village {} is in a kingdom that does not exist",
                v.id
            );
        }
        if let Some(leader) = v.leader {
            assert!(w.unit(leader).is_some(), "village {} has a dead leader", v.id);
        }
    }
    for k in w.kingdoms.iter().filter(|k| k.alive) {
        assert!(!k.cities.is_empty(), "kingdom {} has no cities", k.id);
        if let Some(king) = k.king {
            assert!(w.unit(king).is_some(), "kingdom {} has a dead king", k.id);
        }
        for city in &k.cities {
            assert!(w.village(*city).is_some(), "kingdom {} lost city {}", k.id, city);
        }
    }
}

#[test]
fn rendering_and_png_output_are_deterministic() {
    let w = World::generate(GenParams::new(32, 24, 99, WorldType::Lakes));
    let a = render_ascii(&w, &RenderOpts::mono());
    let b = render_ascii(&w, &RenderOpts::mono());
    assert_eq!(a, b);
    assert_eq!(a.lines().count(), 24);
    let img = color_map(&w, 3, &RenderOpts::default());
    let png_a = worldforge::png::encode_png(&img);
    let png_b = worldforge::png::encode_png(&color_map(&w, 3, &RenderOpts::default()));
    assert_eq!(png_a, png_b);
    assert_eq!(probe_png(&png_a), Some((96, 72)));
}

#[test]
fn the_cli_generates_steps_inspects_and_draws() {
    let dir = std::env::temp_dir();
    let save = dir.join("worldforge_e2e.wfz");
    let png = dir.join("worldforge_e2e.png");
    let save_s = save.to_string_lossy().to_string();
    let png_s = png.to_string_lossy().to_string();

    let out = bin()
        .args(["gen", "--size", "small", "--seed", "31337", "--out", &save_s])
        .output()
        .expect("binary runs");
    assert!(out.status.success(), "gen failed: {out:?}");
    assert!(std::fs::metadata(&save).unwrap().len() > 500);

    let out = bin().args(["step", &save_s, "250"]).output().unwrap();
    assert!(out.status.success(), "step failed: {out:?}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("stepped 250 ticks"), "{stdout}");

    let out = bin().args(["show", &save_s, "--png", &png_s, "--scale", "3"]).output().unwrap();
    assert!(out.status.success(), "show failed: {out:?}");
    let png_bytes = std::fs::read(&png).unwrap();
    assert_eq!(&png_bytes[..8], b"\x89PNG\r\n\x1a\n");
    assert!(probe_png(&png_bytes).unwrap().0 > 0);

    let out = bin().args(["info", &save_s]).output().unwrap();
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("hash 0x"));

    let loaded = load_from_file(&save_s).unwrap();
    assert!(loaded.tick >= 250);

    let _ = std::fs::remove_file(&save);
    let _ = std::fs::remove_file(&png);
}

#[test]
fn the_cli_lists_every_power_and_prints_help() {
    let out = bin().args(["powers"]).output().unwrap();
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    for power in Power::ALL {
        assert!(text.contains(power.name()), "missing power {}", power.name());
    }
    assert!(text.contains("49 powers in total"));

    let out = bin().args(["--help"]).output().unwrap();
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("worldforge <command>"));
    assert!(text.contains("script <file>"));

    let out = bin().args(["--version"]).output().unwrap();
    assert!(String::from_utf8_lossy(&out.stdout).contains("worldforge 0.1.0"));
}

#[test]
fn the_shipped_example_script_runs() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/examples/rise-and-fall.wf");
    let session = script::run_file(path).expect("example script runs");
    assert!(session.world.year > 0);
    assert!(session.world.stats.powers_cast > 0);
    assert!(session.output.contains("hash 0x"));
}

#[test]
fn demo_runs_a_world_and_writes_a_picture() {
    let png = std::env::temp_dir().join("worldforge_demo.png");
    let png_s = png.to_string_lossy().to_string();
    let out = bin()
        .args(["demo", "--size", "tiny", "--seed", "5", "--ticks", "300", "--png", &png_s])
        .output()
        .unwrap();
    assert!(out.status.success(), "demo failed: {out:?}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.lines().count() > 20, "demo should draw a map and a report");
    assert!(std::fs::metadata(&png).unwrap().len() > 1000);
    let _ = std::fs::remove_file(&png);
}

#[test]
fn saves_are_portable_between_processes() {
    let dir = std::env::temp_dir();
    let path = dir.join("worldforge_portable.wfz");
    let path_s = path.to_string_lossy().to_string();
    let src = format!(
        "gen 40 28 77 type archipelago\nstep 8y\nsave {path_s}\nhash\n"
    );
    let session = script::run(&src).unwrap();
    let hash_line = session
        .output
        .lines()
        .find(|l| l.starts_with("hash "))
        .unwrap()
        .to_string();
    let out = bin().args(["info", &path_s]).output().unwrap();
    assert!(out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stdout).contains(&hash_line),
        "reloaded world has a different hash"
    );
    let _ = std::fs::remove_file(&path);
}

#[test]
fn an_empty_world_renders_and_saves_without_panicking() {
    let w = World::empty(20, 14, 3);
    let text = render_ascii(&w, &RenderOpts::default());
    assert_eq!(text.lines().count(), 14);
    let bytes = encode(&w);
    let back = decode(&bytes).unwrap();
    assert_eq!(back.state_hash(), w.state_hash());
    let panel = worldforge::render::render_panel(&w, 40);
    assert!(!panel.is_empty());
    let dir = std::env::temp_dir().join("worldforge_empty.wfz");
    let path = dir.to_string_lossy().to_string();
    save_to_file(&path, &w).unwrap();
    assert!(load_from_file(&path).is_ok());
    let _ = std::fs::remove_file(&path);
}
