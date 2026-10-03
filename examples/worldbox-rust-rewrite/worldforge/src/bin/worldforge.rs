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

use worldforge::mcworld::{parse_message, BlockWorld, IsoOpts, Message};
use worldforge::powers::{Power, PowerCategory};
use worldforge::render::{color_map, render_ascii, render_panel, RenderOpts};
use worldforge::save::{load_from_file, save_to_file};
use worldforge::script;
use worldforge::serve::Server;
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
  serve [--port N] [--tps N] [--seed N] [--size S] [--type T] [--civs N]
        run the simulation and publish it for Minecraft on 127.0.0.1
  mcview --connect HOST:PORT [--png PREFIX] [--every N] [--scale N]
        be the Minecraft side without Minecraft: build the block world and
        write isometric PNGs of it (needs a `serve` to connect to)
  wbox <file.wbox> [--out world.wfz] [--png preview.png] [--civs N] [--width W]
        [--height H] [--flat] [--dump]
        read a WorldBox map and rebuild its terrain here; --dump prints what the
        reader found inside the file without importing anything
  watch [--port N] [--tps N] [--host ADDR] [--size S] [--seed N] [--civs N]
        [--animals N] [--monsters N] [--ticks N] [--wbox FILE [--width W --height H]]
        run the world and serve a live page: the map, the civilizations, the wars
        and the chronicle, updating as they happen (open the printed address)
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
        "serve" => serve(args, rest),
        "wbox" => wbox_cmd(rest),
        "watch" => watch_cmd(args, rest),
        "mcview" => mcview(rest),
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

/// `worldforge wbox`: read a WorldBox map, and optionally make a world of it.
fn wbox_cmd(args: &[String]) -> Result<(), String> {
    use worldforge::wbox;
    let files = positional(args, &["--out", "--png", "--civs", "--animals", "--monsters", "--width", "--height"]);
    let path = files
        .first()
        .ok_or("usage: worldforge wbox <file.wbox> [--out world.wfz] [--png preview.png] [--dump]")?;
    let bytes = std::fs::read(path).map_err(|e| format!("read {path}: {e}"))?;
    if has_flag(args, "--dump") {
        print!("{}", wbox::inspect(&bytes));
        return Ok(());
    }

    let width = flag(args, "--width").map(|v| v.parse::<u16>()).transpose();
    let height = flag(args, "--height").map(|v| v.parse::<u16>()).transpose();
    let map = match (width, height) {
        (Ok(Some(w)), Ok(Some(h))) => wbox::parse_with_size(&bytes, w, h),
        _ => wbox::parse(&bytes),
    }
    .map_err(|e| format!("{path}: {e}"))?;

    let civs = flag(args, "--civs")
        .map(|s| s.parse::<u32>())
        .transpose()
        .map_err(|_| "--civs must be a number".to_string())?
        .unwrap_or(4);
    let animals = flag(args, "--animals")
        .map(|s| s.parse::<u32>())
        .transpose()
        .map_err(|_| "--animals must be a number".to_string())?
        .unwrap_or(30);
    let monsters = flag(args, "--monsters")
        .map(|s| s.parse::<u32>())
        .transpose()
        .map_err(|_| "--monsters must be a number".to_string())?
        .unwrap_or(0);
    let options = wbox::ImportOptions {
        civs,
        animals,
        monsters,
        terrain_height: !has_flag(args, "--flat"),
    };

    println!("{path}: {}", map.note);
    println!("{}", map.summary());
    let world = wbox::to_world(&map, options);
    println!(
        "{} villages settled, {} units, {}x{} world",
        world.villages.iter().filter(|v| v.alive).count(),
        world.units.iter().filter(|u| u.alive).count(),
        world.width,
        world.height
    );

    if let Some(path) = flag(args, "--png") {
        let scale = flag(args, "--scale")
            .map(|s| s.parse::<u32>())
            .transpose()
            .map_err(|_| "--scale must be a number".to_string())?
            .unwrap_or(4)
            .clamp(1, 32);
        let img = wbox::render(&map, scale);
        worldforge::png::write_png(&path, &img).map_err(|e| format!("png: {e}"))?;
        eprintln!("wrote {path} ({}x{}) - the terrain as the reader saw it", img.width, img.height);
    }
    if let Some(path) = flag(args, "--out") {
        let n = save_to_file(&path, &world).map_err(|e| format!("save: {e}"))?;
        println!("saved {path} ({n} bytes) - now run it:");
        println!("  worldforge step {path} 2000 --out {path}");
        println!("  worldforge show {path} --color");
        println!("  worldforge serve --port 25607   # and start a Minecraft client on it");
    }
    Ok(())
}

/// `worldforge serve`: publish the running world on the loopback bridge.
fn serve(args: &[String], rest: &[String]) -> Result<(), String> {
    if has_flag(rest, "--help") || has_flag(rest, "-h") {
        println!(
            "usage: worldforge serve [--port N] [--tps N] [--seed N] [--size tiny|small|medium|large|huge]\n\
             \x20                      [--type T] [--land 5..95] [--civs N] [--animals N] [--monsters N]\n\
             \x20                      [--ticks N]   (pre-run the world before you look at it)\n\
             \x20                      [--wbox FILE.wbox [--width W --height H]]\n\
             \n\
             Runs the simulation and publishes it for a Minecraft client on 127.0.0.1.\n\
             Gen options are the same as `worldforge demo`; see `worldforge --help`."
        );
        return Ok(());
    }
    let mut world = world_from_args(rest)?;
    if let Some(n) = flag(rest, "--ticks").and_then(|v| v.parse::<u64>().ok()) {
        let n = n.min(200_000);
        world.step_n(n);
        println!("watch: pre-ran {n} ticks → year {}", world.year);
    }
    let port = flag(rest, "--port")
        .map(|s| s.parse::<u16>())
        .transpose()
        .map_err(|_| "--port must be a number".to_string())?
        .unwrap_or(worldforge::bridge::DEFAULT_PORT);
    let tps = flag(rest, "--tps")
        .map(|s| s.parse::<f32>())
        .transpose()
        .map_err(|_| "--tps must be a number".to_string())?
        .unwrap_or(10.0)
        .clamp(0.1, 60.0);
    let _ = args;
    let server = Server::bind_local(port, tps).map_err(|e| format!("bind 127.0.0.1:{port}: {e}"))?;
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let handle = server.start(world, stop.clone());
    println!("worldforge: press Ctrl-C to stop");
    loop {
        if stop.load(std::sync::atomic::Ordering::Relaxed) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
        if handle.is_finished() {
            break;
        }
    }
    handle.join().ok();
    Ok(())
}

/// `worldforge watch`: the world, live, in a browser tab.
fn watch_cmd(args: &[String], rest: &[String]) -> Result<(), String> {
    if has_flag(rest, "--help") || has_flag(rest, "-h") {
        println!(
            "usage: worldforge watch [--host 0.0.0.0] [--port N] [--tps N]\n\
             \x20                      [--seed N] [--size tiny|small|medium|large|huge]\n\
             \x20                      [--type T] [--land 5..95] [--civs N] [--animals N] [--monsters N]\n\
             \x20                      [--ticks N]   (pre-run the world before you look at it)\n\
             \x20                      [--wbox FILE.wbox [--width W --height H]]\n\
             \n\
             Serves a live page: the hex map, territory, units, wars, sieges and\n\
             the chronicle, all updating as the simulation runs. It binds every\n\
             interface on purpose, so a browser that reaches this machine through\n\
             a preview proxy can open it. Press Ctrl-C to stop."
        );
        return Ok(());
    }
    let mut world = world_from_args(rest)?;
    if let Some(n) = flag(rest, "--ticks").and_then(|v| v.parse::<u64>().ok()) {
        let n = n.min(200_000);
        world.step_n(n);
        println!("watch: pre-ran {n} ticks → year {}", world.year);
    }
    let port = flag(rest, "--port")
        .map(|s| s.parse::<u16>())
        .transpose()
        .map_err(|_| "--port must be a number".to_string())?
        .unwrap_or(worldforge::live::DEFAULT_PORT);
    let tps = flag(rest, "--tps")
        .map(|s| s.parse::<f32>())
        .transpose()
        .map_err(|_| "--tps must be a number".to_string())?
        .unwrap_or(20.0)
        .clamp(0.5, 120.0);
    let host = flag(rest, "--host").unwrap_or_else(|| "0.0.0.0".to_string());
    let _ = args;
    worldforge::live::watch(
        world,
        &worldforge::live::WatchOptions { host, port, tps },
    )
}

/// A world for the bridge commands: an imported `.wbox` if one was given (the save
/// *is* the terrain, the flags only decide who lives on it), otherwise generated.
fn world_from_args(rest: &[String]) -> Result<World, String> {
    if let Some(path) = flag(rest, "--wbox") {
        let bytes = std::fs::read(&path).map_err(|e| format!("read {path}: {e}"))?;
        let size = match (flag(rest, "--width"), flag(rest, "--height")) {
            (Some(w), Some(h)) => Some((
                w.parse::<u16>().map_err(|_| "--width must be a number".to_string())?,
                h.parse::<u16>().map_err(|_| "--height must be a number".to_string())?,
            )),
            _ => None,
        };
        let map = match size {
            Some((w, h)) => worldforge::wbox::parse_with_size(&bytes, w, h),
            None => worldforge::wbox::parse(&bytes),
        }
        .map_err(|e| format!("{path}: {e}"))?;
        let options = worldforge::wbox::ImportOptions {
            civs: flag(rest, "--civs").and_then(|s| s.parse().ok()).unwrap_or(4),
            animals: flag(rest, "--animals").and_then(|s| s.parse().ok()).unwrap_or(30),
            monsters: flag(rest, "--monsters").and_then(|s| s.parse().ok()).unwrap_or(0),
            terrain_height: !has_flag(rest, "--flat"),
        };
        println!("{path}: {}", map.note);
        println!("{}", map.summary());
        return Ok(worldforge::wbox::to_world(&map, options));
    }
    let mut world = make_world(rest)?;
    seed_world(&mut world, rest)?;
    Ok(world)
}

/// `worldforge mcview`: connect to a bridge and draw what it is sending.
fn mcview(args: &[String]) -> Result<(), String> {
    if has_flag(args, "--help") || has_flag(args, "-h") {
        println!(
            "usage: worldforge mcview --connect HOST:PORT [--png PREFIX] [--every N] [--scale N]\n\
             \x20                      [--region col,row,w,h] [--timeout SECONDS]\n\
             \n\
             The Minecraft side without Minecraft: speaks the bridge protocol, builds the block\n\
             world it describes and writes isometric PNGs of it."
        );
        return Ok(());
    }
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpStream;
    use std::time::{Duration, Instant};

    let connect = flag(args, "--connect").unwrap_or_else(|| format!("127.0.0.1:{}", worldforge::bridge::DEFAULT_PORT));
    let prefix = flag(args, "--png");
    let every = flag(args, "--every")
        .map(|s| s.parse::<u64>())
        .transpose()
        .map_err(|_| "--every must be a number".to_string())?
        .unwrap_or(100)
        .max(1);
    let scale = flag(args, "--scale")
        .map(|s| s.parse::<i32>())
        .transpose()
        .map_err(|_| "--scale must be a number".to_string())?
        .unwrap_or(6)
        .clamp(2, 32);
    let timeout = flag(args, "--timeout")
        .map(|s| s.parse::<u64>())
        .transpose()
        .map_err(|_| "--timeout must be a number".to_string())?
        .map(Duration::from_secs);
    let region = match flag(args, "--region") {
        Some(text) => {
            let parts: Vec<i32> = text
                .split(',')
                .map(|p| p.trim().parse::<i32>().map_err(|_| "--region wants col,row,w,h".to_string()))
                .collect::<Result<_, _>>()?;
            if parts.len() != 4 {
                return Err("--region wants col,row,w,h".into());
            }
            Some((parts[0], parts[1], parts[2], parts[3]))
        }
        None => None,
    };

    let stream = TcpStream::connect(&connect).map_err(|e| format!("connect {connect}: {e}"))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(30)))
        .map_err(|e| e.to_string())?;
    println!("mcview: connected to {connect}");
    let mut reader = BufReader::new(stream.try_clone().map_err(|e| e.to_string())?);

    let mut blocks: Option<BlockWorld> = None;
    let mut frames = 0u64;
    let mut pictures = 0u64;
    let mut printed_villages = 0usize;
    let mut bad_lines = 0u64;
    let started = Instant::now();
    let mut line = String::new();

    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => {
                println!("mcview: the bridge closed the connection");
                break;
            }
            Ok(_) => {}
            Err(e) => {
                println!("mcview: {e}");
                break;
            }
        }
        let text = line.trim();
        if text.is_empty() {
            continue;
        }
        let message = match parse_message(text) {
            Ok(m) => m,
            Err(e) => {
                bad_lines += 1;
                if bad_lines <= 5 {
                    eprintln!("mcview: bad message: {e}");
                }
                continue;
            }
        };
        if let Message::Hello {
            size,
            base_y,
            sea_level,
            canvas_top,
            seed,
            world,
            palette,
            protocol,
            ..
        } = &message
        {
            println!(
                "mcview: protocol {protocol}, {size:?} world `{world}` seed {seed}, {} palette names",
                palette.len()
            );
            blocks = Some(BlockWorld::new(size.0, size.1, *base_y, *sea_level, *canvas_top));
        }
        let Some(world) = blocks.as_mut() else {
            // Joined mid-stream: ask for the tile field again.
            println!("mcview: world messages before hello; asking for the tile field");
            let mut out = stream.try_clone().map_err(|e| e.to_string())?;
            out.write_all(b"{\"t\":\"tiles\"}\n").map_err(|e| e.to_string())?;
            continue;
        };
        world.apply(&message);

        if let Message::Notice { text } = &message {
            println!("mcview: notice: {text}");
            continue;
        }
        if let Message::Frame { info, .. } = &message {
            frames += 1;
            let render = prefix.is_some() && frames % every == 0;
            if frames == 1 || render {
                let (animals, monsters) = (info.animals, info.monsters);
                println!(
                    "mcview: year {:>3} tick {:>6} | pop {:>3} | villages {:>2} | kingdoms {:>2} | animals {animals} monsters {monsters} | {} | {}",
                    info.year, info.tick, info.pop, info.villages, info.kingdoms, info.age, info.hash
                );
                for (year, text) in info.news.iter().take(3) {
                    println!("           {year}: {text}");
                }
                if frames == 1 || printed_villages != world.villages.len() {
                    printed_villages = world.villages.len();
                    for v in world.villages.iter().take(8) {
                        println!(
                            "           village {:<12} {:<6} pop {:>3} at tile {:>3},{:<3} kingdom {}",
                            v.name, v.race, v.pop, v.col, v.row, v.kingdom
                        );
                    }
                }
            }
            if let Some(prefix) = &prefix {
                if render {
                    let img = world.render_iso(&IsoOpts {
                        scale,
                        region,
                        units: true,
                        grid: false,
                    });
                    // Sparse renders get one file per year; a render-every-frame run
                    // gets the tick too, so it does not write the same file 20 times.
                    let path = if every <= 20 {
                        format!("{prefix}-y{:03}-t{:06}.png", info.year, info.tick)
                    } else {
                        format!("{prefix}-y{:03}.png", info.year)
                    };
                    worldforge::png::write_png(&path, &img).map_err(|e| format!("png {path}: {e}"))?;
                    pictures += 1;
                    println!("           wrote {path} ({}x{})", img.width, img.height);
                }
            }
        }
        if let Some(limit) = timeout {
            if started.elapsed() > limit {
                println!("mcview: stopping after {frames} frames");
                break;
            }
        }
    }
    if let Some(world) = &blocks {
        println!(
            "mcview: applied {} messages, {} blocks set, {} cleared, {} units tracked, {pictures} pictures",
            world.messages, world.blocks_set, world.blocks_cleared, world.units.len()
        );
    }
    if bad_lines > 0 {
        println!("mcview: {bad_lines} unparseable lines (the bridge has a bug)");
        return Err(format!("{bad_lines} unparseable lines on the wire"));
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
