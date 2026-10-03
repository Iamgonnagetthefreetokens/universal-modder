//! `worldforge` command line: generate, step, inspect, render and script worlds.
//!
//! ```text
//! worldforge demo --seed 7                      # make a world and watch it run
//! worldforge gen --size medium --seed 42 --out world.wfz
//! worldforge step world.wfz 200
//! worldforge show world.wfz --png world.png --scale 6
//! worldforge script examples/rise-and-fall.wf
//! worldforge powers
//! ```

use std::process::ExitCode;

use worldforge::powers::{Power, PowerCategory};
use worldforge::render::{color_map, render_ascii, render_panel, RenderOpts};
use worldforge::save::{load_from_file, save_to_file};
use worldforge::script;
use worldforge::world::{World, TICKS_PER_YEAR};
use worldforge::worldgen::{GenParams, WorldType};

const HELP: &str = "\
worldforge - a WorldBox-style god simulator in Rust

usage: worldforge <command> [options]

commands
  demo [--seed N] [--size tiny|small|medium|large] [--ticks N] [--color]
       generate a world, run it and print the map and a report
  gen [--size S] [--seed N] [--type T] [--land 5..95] [--out FILE]
       create a world and optionally save it
  both take [--civs N] (default 4) [--animals N] (default 30) [--monsters N]
  [--empty] to start from a lifeless rock instead
  step <save> [ticks] [--out FILE]
       advance a saved world (default 100 ticks) and save it back
  show <save> [--color|--mono] [--panel] [--scale N] [--png FILE]
       print a saved world; --png writes an image
  script <file>
       run a worldforge script
  powers
       list every god power by category
  info <save>
       print a saved world's report without drawing the map

common options: --help, --version";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("worldforge: {message}");
            eprintln!("try `worldforge --help`");
            ExitCode::FAILURE
        }
    }
}

fn run(args: &[String]) -> Result<(), String> {
    let Some(cmd) = args.first().map(|s| s.as_str()) else {
        return Err("no command given".into());
    };
    let rest = &args[1..];
    match cmd {
        "--help" | "-h" | "help" => {
            println!("{HELP}");
            Ok(())
        }
        "--version" | "-V" | "version" => {
            println!("worldforge {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        "demo" => demo(rest),
        "gen" => gen(rest),
        "step" => step(rest),
        "show" => show(rest),
        "script" => script_cmd(rest),
        "powers" => powers(),
        "info" => info(rest),
        other => Err(format!("unknown command `{other}`")),
    }
}

/// `--flag value` or `--flag` (as a boolean). Returns the value if given.
fn flag(args: &[String], name: &str) -> Option<String> {
    let at = args.iter().position(|a| a == name)?;
    args.get(at + 1)
        .filter(|v| !v.starts_with("--"))
        .cloned()
}

fn has_flag(args: &[String], name: &str) -> bool {
    args.iter().any(|a| a == name)
}

/// The first positional argument, ignoring flags and their values.
fn positional(args: &[String], skip: &[&str]) -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        if a.starts_with("--") {
            let takes_value = skip.contains(&a.as_str())
                || !matches!(
                    a.as_str(),
                    "--color" | "--mono" | "--panel" | "--plain"
                );
            i += if takes_value && args.get(i + 1).map(|v| !v.starts_with("--")) == Some(true) {
                2
            } else {
                1
            };
            continue;
        }
        out.push(a.clone());
        i += 1;
    }
    out
}

fn size_of(token: &str) -> Option<(u16, u16)> {
    match token.to_ascii_lowercase().as_str() {
        "tiny" => Some((32, 24)),
        "small" => Some((48, 32)),
        "medium" => Some((64, 48)),
        "large" => Some((96, 64)),
        "huge" => Some((128, 96)),
        _ => None,
    }
}

fn make_world(args: &[String]) -> Result<World, String> {
    let size = flag(args, "--size").unwrap_or_else(|| "medium".into());
    let (w, h) = size_of(&size)
        .ok_or_else(|| format!("unknown size `{size}` (tiny, small, medium, large, huge)"))?;
    let seed = flag(args, "--seed")
        .map(|s| s.parse::<u64>())
        .transpose()
        .map_err(|_| "--seed must be a number".to_string())?
        .unwrap_or(0x5eed);
    let type_name = flag(args, "--type").unwrap_or_else(|| "continents".into());
    let world_type = WorldType::parse(&type_name)
        .ok_or_else(|| format!("unknown world type `{type_name}` (try `worldforge --help`)"))?;
    let land = flag(args, "--land")
        .map(|s| s.parse::<u8>())
        .transpose()
        .map_err(|_| "--land must be a number 5..=95".to_string())?
        .unwrap_or(55)
        .clamp(5, 95);
    let params = GenParams::new(w, h, seed, world_type).with_land_ratio(land);
    Ok(World::generate(params))
}

/// Place starting civilizations and wildlife unless `--empty` was given.
fn seed_world(world: &mut World, args: &[String]) -> Result<usize, String> {
    if has_flag(args, "--empty") {
        return Ok(0);
    }
    let n = |name: &str, default: u32| -> Result<u32, String> {
        flag(args, name)
            .map(|s| s.parse::<u32>())
            .transpose()
            .map_err(|_| format!("{name} must be a number"))
            .map(|v| v.unwrap_or(default))
    };
    let civs = n("--civs", 4)?;
    let animals = n("--animals", 30)?;
    let monsters = n("--monsters", 0)?;
    Ok(world.seed_life(civs, animals, monsters).len())
}

fn report(world: &World, args: &[String]) {
    let color = has_flag(args, "--color");
    let panel_only = has_flag(args, "--panel");
    let mono = has_flag(args, "--mono") || has_flag(args, "--plain");
    let opts = if color {
        RenderOpts::default()
    } else if mono {
        RenderOpts::plain()
    } else {
        RenderOpts::mono()
    };
    if !panel_only {
        print!("{}", render_ascii(world, &opts));
    }
    print!("{}", render_panel(world, 78));
}

fn write_png_if_asked(world: &World, args: &[String]) -> Result<(), String> {
    if let Some(path) = flag(args, "--png") {
        let scale = flag(args, "--scale")
            .map(|s| s.parse::<u32>())
            .transpose()
            .map_err(|_| "--scale must be a number".to_string())?
            .unwrap_or(4)
            .clamp(1, 32);
        let img = color_map(world, scale, &RenderOpts::default());
        worldforge::png::write_png(&path, &img).map_err(|e| format!("png: {e}"))?;
        eprintln!("wrote {path} ({}x{})", img.width, img.height);
    }
    Ok(())
}

fn demo(args: &[String]) -> Result<(), String> {
    let mut world = make_world(args)?;
    let villages = seed_world(&mut world, args)?;
    if villages > 0 {
        eprintln!("seeded {villages} villages to start with");
    }
    let ticks = flag(args, "--ticks")
        .map(|s| s.parse::<u64>())
        .transpose()
        .map_err(|_| "--ticks must be a number".to_string())?
        .unwrap_or(TICKS_PER_YEAR * 60);
    world.step_n(ticks);
    eprintln!(
        "ran {ticks} ticks ({} years) of a {}x{} {} world, seed {}",
        world.year,
        world.width,
        world.height,
        world.world_type.name(),
        world.seed
    );
    report(&world, args);
    write_png_if_asked(&world, args)
}

fn gen(args: &[String]) -> Result<(), String> {
    let mut world = make_world(args)?;
    seed_world(&mut world, args)?;
    match flag(args, "--out") {
        Some(path) => {
            let n = save_to_file(&path, &world).map_err(|e| format!("save: {e}"))?;
            println!(
                "{}x{} {} world, seed {} -> {path} ({n} bytes)",
                world.width,
                world.height,
                world.world_type.name(),
                world.seed
            );
        }
        None => {
            print!("{}", render_panel(&world, 78));
        }
    }
    Ok(())
}

fn step(args: &[String]) -> Result<(), String> {
    let files = positional(args, &["--out", "--ticks"]);
    let path = files
        .first()
        .ok_or("usage: worldforge step <save> [ticks] [--out FILE]")?;
    let mut world = load_from_file(path).map_err(|e| format!("load {path}: {e}"))?;
    let ticks = flag(args, "--ticks")
        .or_else(|| files.get(1).cloned())
        .map(|s| s.parse::<u64>())
        .transpose()
        .map_err(|_| "ticks must be a number".to_string())?
        .unwrap_or(100);
    world.step_n(ticks);
    let out = flag(args, "--out").unwrap_or_else(|| path.clone());
    let n = save_to_file(&out, &world).map_err(|e| format!("save: {e}"))?;
    println!(
        "stepped {ticks} ticks -> year {}, tick {}, {} saved ({n} bytes)",
        world.year, world.tick, out
    );
    print!("{}", render_panel(&world, 78));
    Ok(())
}

fn show(args: &[String]) -> Result<(), String> {
    let files = positional(args, &["--png", "--scale"]);
    let path = files
        .first()
        .ok_or("usage: worldforge show <save> [--color] [--png FILE]")?;
    let world = load_from_file(path).map_err(|e| format!("load {path}: {e}"))?;
    report(&world, args);
    write_png_if_asked(&world, args)
}

fn info(args: &[String]) -> Result<(), String> {
    let files = positional(args, &[]);
    let path = files.first().ok_or("usage: worldforge info <save>")?;
    let world = load_from_file(path).map_err(|e| format!("load {path}: {e}"))?;
    println!("{}", world.summary());
    print!("{}", render_panel(&world, 78));
    Ok(())
}

fn script_cmd(args: &[String]) -> Result<(), String> {
    let files = positional(args, &[]);
    let path = files.first().ok_or("usage: worldforge script <file>")?;
    let session = script::run_file(path).map_err(|e| format!("{path}: {e}"))?;
    print!("{}", session.output);
    if session.quit {
        eprintln!("(script stopped early)");
    }
    Ok(())
}

fn powers() -> Result<(), String> {
    let categories = [
        PowerCategory::Destruction,
        PowerCategory::LifeAndDeath,
        PowerCategory::Divine,
        PowerCategory::WorldShaping,
        PowerCategory::Civilization,
        PowerCategory::Summon,
    ];
    for category in categories {
        println!("{}", category.name());
        for power in Power::ALL.iter().filter(|p| p.category() == category) {
            println!("  {:<18} {}", power.name(), power.description());
        }
    }
    println!("\n{} powers in total", Power::ALL.len());
    Ok(())
}
