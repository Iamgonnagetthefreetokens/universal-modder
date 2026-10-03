//! A tiny scripting language, so a whole scenario can live in a text file.
//!
//! ```text
//! gen 48 32 seed 42 type continents
//! step 10y
//! cast rain @random
//! spawn human soldier 12,10
//! summary
//! hash
//! ```
//!
//! Scripts are the reproducibility story: the same script on the same build
//! always produces the same `hash` line.

use std::fmt;

use crate::ages::Age;
use crate::hex::Hex;
use crate::png::write_png;
use crate::powers::Power;
use crate::races::Race;
use crate::render::{color_map, render_ascii, render_panel, RenderOpts};
use crate::save::{load_from_file, save_to_file};
use crate::units::UnitKind;
use crate::world::{World, TICKS_PER_YEAR};
use crate::worldgen::{GenParams, WorldType};

/// A failing script line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScriptError {
    /// 1-based line number, or 0 for errors before the first line.
    pub line: usize,
    pub message: String,
}

impl fmt::Display for ScriptError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.line == 0 {
            write!(f, "{}", self.message)
        } else {
            write!(f, "line {}: {}", self.line, self.message)
        }
    }
}

impl std::error::Error for ScriptError {}

fn err(line: usize, message: impl Into<String>) -> ScriptError {
    ScriptError {
        line,
        message: message.into(),
    }
}

/// What a script run produced.
#[derive(Clone, Debug)]
pub struct Session {
    pub world: World,
    /// Everything the script printed, as text.
    pub output: String,
    /// Commands executed, `quit` excluded.
    pub commands_run: usize,
    /// True if the script asked to stop early.
    pub quit: bool,
}

/// Split a line into tokens, honouring double quotes for `echo`.
fn tokenize(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    let mut has_token = false;
    for ch in line.chars() {
        match ch {
            '"' => {
                quoted = !quoted;
                has_token = true;
            }
            c if c.is_whitespace() && !quoted => {
                if has_token {
                    out.push(std::mem::take(&mut cur));
                    has_token = false;
                }
            }
            c => {
                cur.push(c);
                has_token = true;
            }
        }
    }
    if has_token {
        out.push(cur);
    }
    out
}

/// Parse `12,7` / `12 7` into a hex.
fn parse_hex(tokens: &[String], line: usize) -> Result<Hex, ScriptError> {
    let joined = tokens.join(",");
    let parts: Vec<&str> = joined.split(',').filter(|s| !s.is_empty()).collect();
    if parts.len() != 2 {
        return Err(err(line, "expected a tile like `12,7`"));
    }
    let q = parts[0]
        .parse::<i32>()
        .map_err(|_| err(line, format!("`{}` is not a number", parts[0])))?;
    let r = parts[1]
        .parse::<i32>()
        .map_err(|_| err(line, format!("`{}` is not a number", parts[1])))?;
    Ok(Hex { q, r })
}

/// Parse a count with an optional `y` suffix: `40` ticks, `3y` years.
fn parse_duration(token: &str, line: usize) -> Result<u64, ScriptError> {
    let (num, years) = match token.strip_suffix('y') {
        Some(rest) => (rest, true),
        None => (token, false),
    };
    let n = num
        .parse::<u64>()
        .map_err(|_| err(line, format!("`{token}` is not a duration")))?;
    Ok(if years { n * TICKS_PER_YEAR } else { n })
}

fn parse_kind(token: &str) -> Option<UnitKind> {
    match token.to_ascii_lowercase().as_str() {
        "civilian" | "villager" | "civ" => Some(UnitKind::Civilian),
        "leader" => Some(UnitKind::Leader),
        "king" => Some(UnitKind::King),
        "soldier" | "warrior" => Some(UnitKind::Soldier),
        "animal" => Some(UnitKind::Animal),
        "monster" => Some(UnitKind::Monster),
        _ => None,
    }
}

/// A tile from `@random`, `@center`, `@first-land` or explicit `q,r`.
fn resolve_target(w: &mut World, tokens: &[String], line: usize) -> Result<Hex, ScriptError> {
    match tokens.first().map(|s| s.as_str()) {
        Some("@random") => w
            .random_land_tile()
            .ok_or_else(|| err(line, "the world has no land")),
        Some("@center") => Ok(Hex {
            q: w.width as i32 / 2,
            r: w.height as i32 / 2,
        }),
        Some("@first-land") => w
            .iter_hexes()
            .into_iter()
            .find(|h| w.tile(*h).map(|t| t.is_land()).unwrap_or(false))
            .ok_or_else(|| err(line, "the world has no land")),
        Some(_) => parse_hex(tokens, line),
        None => Err(err(line, "expected a target like `12,7` or `@random`")),
    }
}

/// Run a script against a fresh world (the `gen` line is required).
pub fn run(src: &str) -> Result<Session, ScriptError> {
    let world = World::new(16, 16, 0, WorldType::Continents);
    run_on(src, world)
}

/// Run a script, starting from an existing world. `gen` replaces it.
pub fn run_on(src: &str, world: World) -> Result<Session, ScriptError> {
    let mut session = Session {
        world,
        output: String::new(),
        commands_run: 0,
        quit: false,
    };
    let mut generated = false;
    for (idx, raw) in src.lines().enumerate() {
        let line_no = idx + 1;
        let line = match raw.find('#') {
            Some(at) => &raw[..at],
            None => raw,
        };
        let tokens = tokenize(line);
        if tokens.is_empty() {
            continue;
        }
        let cmd = tokens[0].to_ascii_lowercase();
        let args = &tokens[1..];
        if cmd == "quit" || cmd == "exit" {
            session.quit = true;
            break;
        }
        if cmd == "help" {
            session.output.push_str(help());
            continue;
        }
        if cmd == "gen" {
            if args.len() < 3 {
                return Err(err(line_no, "usage: gen <width> <height> <seed> [keywords]"));
            }
            let width = args[0]
                .parse::<u16>()
                .map_err(|_| err(line_no, "width must be a number"))?;
            let height = args[1]
                .parse::<u16>()
                .map_err(|_| err(line_no, "height must be a number"))?;
            let seed = args[2]
                .parse::<u64>()
                .map_err(|_| err(line_no, "seed must be a number"))?;
            let mut world_type = WorldType::Continents;
            let mut land = 55u8;
            let mut i = 3;
            while i < args.len() {
                match args[i].to_ascii_lowercase().as_str() {
                    "type" => {
                        let name = args
                            .get(i + 1)
                            .ok_or_else(|| err(line_no, "type needs a value"))?;
                        world_type = WorldType::parse(name)
                            .ok_or_else(|| err(line_no, format!("unknown world type `{name}`")))?;
                        i += 2;
                    }
                    "land" => {
                        let value = args
                            .get(i + 1)
                            .ok_or_else(|| err(line_no, "land needs a percentage"))?;
                        land = value
                            .parse::<u8>()
                            .map_err(|_| err(line_no, "land must be a number 5..=95"))?
                            .clamp(5, 95);
                        i += 2;
                    }
                    other => {
                        // A bare type name is allowed for brevity.
                        world_type = WorldType::parse(other).ok_or_else(|| {
                            err(line_no, format!("unknown keyword `{other}`"))
                        })?;
                        i += 1;
                    }
                }
            }
            session.world = World::generate(
                GenParams::new(width, height, seed, world_type).with_land_ratio(land),
            );
            generated = true;
        } else if !generated {
            return Err(err(
                line_no,
                format!("`{cmd}` needs a world: put a `gen` line first"),
            ));
        } else if cmd == "step" {
            let n = args
                .first()
                .ok_or_else(|| err(line_no, "usage: step <ticks|Ny>"))?;
            let ticks = parse_duration(n, line_no)?;
            session.world.step_n(ticks);
        } else if cmd == "cast" {
            let name = args
                .first()
                .ok_or_else(|| err(line_no, "usage: cast <power> [q,r|@random]"))?;
            let power = Power::parse(name).ok_or_else(|| {
                err(
                    line_no,
                    format!(
                        "unknown power `{name}` (two-word powers take an underscore: acid_rain)"
                    ),
                )
            })?;
            let target = if args.len() > 1 {
                resolve_target(&mut session.world, &args[1..], line_no)?
            } else {
                session
                    .world
                    .random_land_tile()
                    .ok_or_else(|| err(line_no, "the world has no land"))?
            };
            let outcome = session.world.cast(power, target);
            session.output.push_str(&format!(
                "cast {} at {}: {} ({})\n",
                power.name(),
                target,
                outcome.message,
                if outcome.ok { "ok" } else { "no effect" }
            ));
        } else if cmd == "spawn" {
            let race_name = args
                .first()
                .ok_or_else(|| err(line_no, "usage: spawn <race> [kind] [q,r|@random]"))?;
            let race = Race::parse(race_name)
                .ok_or_else(|| err(line_no, format!("unknown race `{race_name}`")))?;
            let mut rest: Vec<String> = args[1..].to_vec();
            let mut kind = None;
            if let Some(first) = rest.first() {
                if let Some(k) = parse_kind(first) {
                    kind = Some(k);
                    rest.remove(0);
                }
            }
            let pos = if rest.is_empty() {
                session
                    .world
                    .random_land_tile()
                    .ok_or_else(|| err(line_no, "the world has no land"))?
            } else {
                resolve_target(&mut session.world, &rest, line_no)?
            };
            let kind = kind.unwrap_or(UnitKind::Civilian);
            match session.world.spawn_unit(race, pos, kind) {
                Some(id) => session
                    .output
                    .push_str(&format!("spawned {} {} as #{}\n", race.name(), kind.name(), id)),
                None => return Err(err(line_no, "could not spawn there")),
            }
        } else if cmd == "life" {
            let civs = args.first().and_then(|s| s.parse::<u32>().ok()).unwrap_or(4);
            let animals = args.get(1).and_then(|s| s.parse::<u32>().ok()).unwrap_or(40);
            let monsters = args.get(2).and_then(|s| s.parse::<u32>().ok()).unwrap_or(0);
            let founded = session.world.seed_life(civs, animals, monsters);
            session.output.push_str(&format!(
                "seeded {} villages, {} animals, {} monsters\n",
                founded.len(),
                animals,
                monsters
            ));
        } else if cmd == "age" {
            let name = args
                .first()
                .ok_or_else(|| err(line_no, "usage: age <age name>"))?;
            let age = Age::parse(name)
                .ok_or_else(|| err(line_no, format!("unknown age `{name}`")))?;
            session.world.age.age = age;
            session.output.push_str(&format!("age: {}\n", age.name()));
        } else if cmd == "hash" {
            session
                .output
                .push_str(&format!("hash {}\n", crate::render::hash_line(&session.world)));
        } else if cmd == "summary" {
            session.output.push_str(&session.world.summary());
            session.output.push('\n');
        } else if cmd == "villages" {
            let rows = session.world.village_table();
            for row in rows {
                session.output.push_str(&row);
                session.output.push('\n');
            }
        } else if cmd == "census" {
            for line in session.world.census() {
                session.output.push_str(&line);
                session.output.push('\n');
            }
        } else if cmd == "panel" {
            let width = args
                .first()
                .and_then(|s| s.parse::<usize>().ok())
                .unwrap_or(60);
            session
                .output
                .push_str(&render_panel(&session.world, width));
        } else if cmd == "render" {
            let opts = match args.first().map(|s| s.to_ascii_lowercase()).as_deref() {
                Some("color") | Some("colour") => RenderOpts::default(),
                Some("mono") => RenderOpts::mono(),
                _ => RenderOpts::plain(),
            };
            session
                .output
                .push_str(&render_ascii(&session.world, &opts));
        } else if cmd == "legend" {
            session.output.push_str(&crate::render::legend());
        } else if cmd == "png" {
            let path = args
                .first()
                .ok_or_else(|| err(line_no, "usage: png <path> [scale]"))?;
            let scale = args
                .get(1)
                .and_then(|s| s.parse::<u32>().ok())
                .unwrap_or(4)
                .clamp(1, 32);
            let img = color_map(&session.world, scale, &RenderOpts::default());
            write_png(path, &img).map_err(|e| err(line_no, format!("png: {e}")))?;
            session.output.push_str(&format!(
                "png {} ({}x{})\n",
                path, img.width, img.height
            ));
        } else if cmd == "save" {
            let path = args
                .first()
                .ok_or_else(|| err(line_no, "usage: save <path>"))?;
            let n = save_to_file(path, &session.world)
                .map_err(|e| err(line_no, format!("save: {e}")))?;
            session
                .output
                .push_str(&format!("saved {path} ({n} bytes)\n"));
        } else if cmd == "load" {
            let path = args
                .first()
                .ok_or_else(|| err(line_no, "usage: load <path>"))?;
            session.world = load_from_file(path)
                .map_err(|e| err(line_no, format!("load: {e}")))?;
            generated = true;
            session.output.push_str(&format!("loaded {path}\n"));
        } else if cmd == "echo" {
            session.output.push_str(&args.join(" "));
            session.output.push('\n');
        } else {
            return Err(err(line_no, format!("unknown command `{cmd}` (try `help`)")));
        }
        session.commands_run += 1;
    }
    Ok(session)
}

/// Run a script file.
pub fn run_file(path: &str) -> Result<Session, ScriptError> {
    let src = std::fs::read_to_string(path)
        .map_err(|e| err(0, format!("{path}: {e}")))?;
    run(&src)
}

/// The built-in help text.
pub fn help() -> &'static str {
    "\
worldforge script commands
  gen <w> <h> <seed> [type <name>] [land <5..95>]   create a world
  step <ticks|Ny>          advance N ticks, or N years (20 ticks each)
  cast <power> [q,r|@random|@center|@first-land]    use a god power
       two-word powers take an underscore or quotes: acid_rain, \"blood rain\"
  spawn <race> [kind] [q,r|@random]                 add a unit
  life [civs] [animals] [monsters]                 populate the world
  age <name>               force the current age
  hash | summary | census | villages | panel [width]    report on the world
  render [plain|mono|color] | legend                draw the map
  png <path> [scale] | save <path> | load <path>    files
  echo <text>              print text
  quit                     stop (lines after this are ignored)
Comments start with `#`. World types: continents, archipelago, pangaea,
highlands, lakes, desert, frozen."
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEMO: &str = "\
# a small scenario
gen 48 32 4242 type continents land 60
step 5y
cast rain @random
spawn human soldier @random
spawn orc king @random
summary
hash
";

    #[test]
    fn a_script_runs_and_reports() {
        let s = run(DEMO).expect("script runs");
        assert!(s.commands_run >= 7);
        assert!(!s.quit);
        assert!(s.output.contains("hash 0x"));
        assert!(s.output.contains("spawned"));
        assert!(s.world.year >= 5);
    }

    #[test]
    fn the_same_script_gives_the_same_hash_every_time() {
        let a = run(DEMO).unwrap();
        let b = run(DEMO).unwrap();
        assert_eq!(a.world.state_hash(), b.world.state_hash());
        assert_eq!(a.output, b.output);
    }

    #[test]
    fn a_different_seed_changes_the_world() {
        let a = run(DEMO).unwrap();
        let b = run(&DEMO.replace("4242", "4243")).unwrap();
        assert_ne!(a.world.state_hash(), b.world.state_hash());
    }

    #[test]
    fn targets_and_durations_are_understood() {
        let s = run("gen 32 24 1\nstep 2y\ncast nuke @center\nspawn dragon monster 1,1\n")
            .unwrap();
        assert_eq!(s.world.tick, 40, "2y is 40 ticks");
        assert!(s.output.contains("cast nuke"));
        assert_eq!(s.world.units.len(), 1);
    }

    #[test]
    fn errors_point_at_the_offending_line() {
        let e = run("gen 16 16 1\nstep 10\nfrobnicate the world\n").unwrap_err();
        assert_eq!(e.line, 3);
        assert!(e.message.contains("unknown command"));
        assert!(format!("{e}").starts_with("line 3:"));

        let e = run("step 10\n").unwrap_err();
        assert_eq!(e.line, 1);
        assert!(e.message.contains("needs a world"));

        let e = run("gen 16 16 1\ncast lazerbeam @random\n").unwrap_err();
        assert!(e.message.contains("unknown power"));

        let e = run("gen 16 16 1\ncast nuke nowhere\n").unwrap_err();
        assert!(e.message.contains("tile like"), "unexpected: {e:?}");
        // `cast nuke` with no target picks a random land tile and succeeds.
        assert!(run("gen 16 16 1\ncast nuke\n").is_ok());
    }

    #[test]
    fn files_and_pngs_come_out_of_a_script() {
        let dir = std::env::temp_dir();
        let save = dir.join("worldforge_script_test.wfz");
        let png = dir.join("worldforge_script_test.png");
        let src = format!(
            "gen 24 16 5\nstep 3\nsave {}\npng {} 2\nhash\n",
            save.display(),
            png.display()
        );
        let s = run(&src).unwrap();
        assert!(s.output.contains("saved"));
        assert!(s.output.contains("png"));
        assert!(std::fs::metadata(&save).unwrap().len() > 100);
        let bytes = std::fs::read(&png).unwrap();
        assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
        assert_eq!(crate::png::probe_png(&bytes), Some((48, 32)));
        let _ = std::fs::remove_file(&save);
        let _ = std::fs::remove_file(&png);
    }

    #[test]
    fn a_script_can_quit_and_leave_the_rest_alone() {
        let s = run("gen 16 16 1\nstep 10\nquit\nfrobnicate\n").unwrap();
        assert!(s.quit);
        assert_eq!(s.commands_run, 2);
    }

    #[test]
    fn quoting_and_comments_behave() {
        let s = run("gen 16 16 1\necho \"hello  world\"\n# nothing here\necho done\n").unwrap();
        assert!(s.output.contains("hello  world"));
        assert!(s.output.contains("done\n"));
    }
}
