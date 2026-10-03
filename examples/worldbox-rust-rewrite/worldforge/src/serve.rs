//! The bridge server: run the simulation, publish it, take commands back.
//!
//! One thread owns the [`World`] and does all the talking: every tick it diffs the
//! tile field, sends the changes and a frame to every client. Reader threads only
//! push parsed commands into a channel, so a slow or hostile client can never
//! stall the simulation -- a client that stops reading is simply dropped.
//!
//! Everything is on `127.0.0.1`. There is no authentication: like the GTA V
//! passthrough's link, any program on the machine can talk to it, so the server
//! binds the loopback interface only and says so in its startup line.

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, TryRecvError};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::bridge::{self, Command, TileSpec};
use crate::world::World;

/// How long a write may block before the client is dropped.
const WRITE_TIMEOUT: Duration = Duration::from_millis(500);
/// How often structures are recomputed (they change with buildings, not ticks).
const STRUCTURE_EVERY: u64 = 20;

enum Event {
    Connected(u64, TcpStream),
    /// (client id, command). The id is kept for logging and replies.
    Command(u64, Command),
    Gone(u64),
}

/// A bound bridge, ready to start.
pub struct Server {
    listener: TcpListener,
    tps: f32,
}

impl Server {
    /// Bind the loopback interface. Port `0` picks a free one (tests do this).
    pub fn bind(addr: SocketAddr, tps: f32) -> std::io::Result<Server> {
        let listener = TcpListener::bind(addr)?;
        Ok(Server {
            listener,
            tps: tps.clamp(0.1, 60.0),
        })
    }

    /// Convenience: `127.0.0.1:port`.
    pub fn bind_local(port: u16, tps: f32) -> std::io::Result<Server> {
        Server::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, port)), tps)
    }

    pub fn port(&self) -> u16 {
        self.listener.local_addr().map(|a| a.port()).unwrap_or(0)
    }

    /// Start serving. The returned handle runs the simulation loop; set `stop` to
    /// wind it down. The accept loop is a second, detached thread.
    pub fn start(self, world: World, stop: Arc<AtomicBool>) -> JoinHandle<()> {
        let (tx, rx) = channel::<Event>();
        let port = self.port();
        let tps = self.tps;

        let accept_tx = tx.clone();
        let accept_stop = Arc::clone(&stop);
        std::thread::Builder::new()
            .name("bridge-accept".into())
            .spawn(move || {
                let stop = accept_stop;
                let mut next_id = 1u64;
                for stream in self.listener.incoming() {
                    let Ok(stream) = stream else { continue };
                    if stop.load(Ordering::Relaxed) {
                        break;
                    }
                    let id = next_id;
                    next_id += 1;
                    let _ = stream.set_nodelay(true);
                    if accept_tx.send(Event::Connected(id, stream.try_clone().expect("clone"))).is_err() {
                        break;
                    }
                    let reader_tx = accept_tx.clone();
                    std::thread::Builder::new()
                        .name(format!("bridge-client-{id}"))
                        .spawn(move || {
                            let mut lines = BufReader::new(stream);
                            let mut line = String::new();
                            loop {
                                line.clear();
                                match lines.read_line(&mut line) {
                                    Ok(0) | Err(_) => break,
                                    Ok(_) => {
                                        let text = line.trim();
                                        if text.is_empty() {
                                            continue;
                                        }
                                        match bridge::parse_command(text) {
                                            Ok(command) => {
                                                if reader_tx
                                                    .send(Event::Command(id, command))
                                                    .is_err()
                                                {
                                                    break;
                                                }
                                            }
                                            Err(message) => {
                                                eprintln!("bridge: client {id}: {message}");
                                            }
                                        }
                                    }
                                }
                            }
                            let _ = reader_tx.send(Event::Gone(id));
                        })
                        .expect("spawn client reader");
                }
            })
            .expect("spawn accept thread");

        std::thread::Builder::new()
            .name("bridge-sim".into())
            .spawn(move || sim_loop(world, rx, stop, tps, port))
            .expect("spawn sim thread")
    }
}

/// A one-line reply to a client, for the things it asked for.
fn notice_json(text: &str) -> String {
    format!(
        "{{\"t\":\"notice\",\"text\":\"{}\"}}",
        bridge::json_escape(text)
    )
}

struct Client {
    id: u64,
    stream: TcpStream,
}

impl Client {
    fn send(&mut self, line: &str) -> bool {
        self.stream
            .write_all(line.as_bytes())
            .and_then(|_| self.stream.write_all(b"\n"))
            .and_then(|_| self.stream.flush())
            .is_ok()
    }
}

fn sim_loop(world: World, rx: Receiver<Event>, stop: Arc<AtomicBool>, tps0: f32, port: u16) {
    let mut world = world;
    let mut clients: Vec<Client> = Vec::new();
    let mut pending: VecDeque<(u64, Command)> = VecDeque::new();
    // `(client id or 0 for everyone, line)` -- replies and error notices.
    let mut messages: VecDeque<(u64, String)> = VecDeque::new();

    let mut specs: Vec<TileSpec> = bridge::tile_specs(&world);
    let mut ops = bridge::world_ops(&world);
    let mut tps = tps0;
    let mut paused = false;
    let mut step_budget: u32 = 0;
    let mut last_news_tick = world.tick;
    let mut resend_tiles = true;
    let mut tick_started = Instant::now();

    println!(
        "worldforge serve: 127.0.0.1:{port} | {}x{} seed {} | {tps} ticks/s | {} units",
        world.width,
        world.height,
        world.seed,
        world.units.iter().filter(|u| u.alive).count()
    );
    println!("worldforge serve: waiting for a Minecraft client (or `worldforge mcview`)");

    while !stop.load(Ordering::Relaxed) {
        // 1. everything the clients said since the last tick
        loop {
            match rx.try_recv() {
                Ok(Event::Connected(id, stream)) => {
                    let _ = stream.set_write_timeout(Some(WRITE_TIMEOUT));
                    println!("bridge: client {id} connected");
                    clients.push(Client { id, stream });
                    resend_tiles = true;
                }
                Ok(Event::Command(id, command)) => pending.push_back((id, command)),
                Ok(Event::Gone(id)) => {
                    clients.retain(|c| c.id != id);
                    println!("bridge: client {id} disconnected");
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => break,
            }
        }

        while let Some((client, command)) = pending.pop_front() {
            match command {
                Command::Ping => {
                    messages.push_back((client, format!("{{\"t\":\"pong\",\"tick\":{}}}", world.tick)));
                }
                Command::Speed { tps: wanted } => {
                    tps = wanted.clamp(0.1, 60.0);
                    println!("bridge: speed set to {tps} ticks/s");
                }
                Command::Pause { on } => {
                    paused = on;
                    println!("bridge: {}", if on { "paused" } else { "running" });
                }
                Command::Step { ticks } => {
                    paused = true;
                    step_budget = step_budget.saturating_add(ticks);
                    println!("bridge: stepping {ticks} ticks");
                }
                Command::Tiles => resend_tiles = true,
                Command::Power { power, col, row } => {
                    let at = crate::hex::Hex::from_offset(col, row);
                    let outcome = world.cast(power, at);
                    let line = format!(
                        "{} at {col},{row}: {}{}",
                        power.name(),
                        if outcome.ok { "" } else { "failed - " },
                        outcome.message
                    );
                    println!("bridge: {line}");
                    if !outcome.ok {
                        messages.push_back((client, notice_json(&line)));
                    }
                }
                Command::Spawn {
                    race,
                    soldier,
                    col,
                    row,
                } => {
                    let kind = if soldier {
                        crate::units::UnitKind::Soldier
                    } else {
                        crate::units::UnitKind::Civilian
                    };
                    let at = crate::hex::Hex::from_offset(col, row);
                    match world.spawn_unit(race, at, kind) {
                        Some(id) => {
                            println!("bridge: spawned {} #{id} at {col},{row}", race.def().name);
                        }
                        None => {
                            let line = format!(
                                "cannot put a {} there (tile {col},{row} is not walkable)",
                                race.def().name
                            );
                            println!("bridge: {line}");
                            messages.push_back((client, notice_json(&line)));
                        }
                    }
                }
            }
        }

        // 2. advance the world
        let stepping = !paused || step_budget > 0;
        if stepping {
            world.step();
            step_budget = step_budget.saturating_sub(1);
        }

        // 3. publish
        let news = bridge::news_since(&world, last_news_tick);
        if !news.is_empty() {
            last_news_tick = world.tick;
        }
        let units = bridge::unit_specs(&world);
        let frame = bridge::frame_json(&world, &units, &news);

        let next_specs = if stepping {
            bridge::tile_specs(&world)
        } else {
            Vec::new()
        };
        let edits = if next_specs.is_empty() {
            Vec::new()
        } else {
            bridge::diff_specs(&specs, &next_specs)
        };
        if !next_specs.is_empty() {
            specs = next_specs;
        }

        let new_ops = if stepping && world.tick % STRUCTURE_EVERY == 0 {
            bridge::world_ops(&world)
        } else {
            Vec::new()
        };
        let structures_changed = !new_ops.is_empty() && new_ops != ops;
        if structures_changed {
            ops = new_ops;
        }

        let hello = bridge::hello_json(&world, port, tps);
        let tiles = if resend_tiles {
            Some(bridge::tiles_json(&specs))
        } else {
            None
        };
        let edits_line = if !edits.is_empty() {
            Some(bridge::edits_json(&edits))
        } else {
            None
        };
        let structures_line = if structures_changed || resend_tiles {
            Some(bridge::structures_json(&ops))
        } else {
            None
        };

        let mut dead: Vec<u64> = Vec::new();
        for client in clients.iter_mut() {
            let mut ok = true;
            // hello first: a client that joins mid-stream needs the shape of the
            // world before it is told about any of it.
            if ok && tiles.is_some() {
                ok &= client.send(&hello);
            }
            if ok {
                if let Some(line) = &tiles {
                    ok &= client.send(line);
                }
            }
            if ok {
                if let Some(line) = &edits_line {
                    ok &= client.send(line);
                }
            }
            if ok {
                if let Some(line) = &structures_line {
                    ok &= client.send(line);
                }
            }
            if ok {
                ok &= client.send(&frame);
            }
            while let Some((to, line)) = messages.pop_front() {
                if to == 0 || to == client.id {
                    ok &= client.send(&line);
                } else {
                    messages.push_front((to, line));
                    break;
                }
            }
            if !ok {
                dead.push(client.id);
            }
        }
        if !dead.is_empty() {
            for id in &dead {
                println!("bridge: client {id} dropped (write failed)");
            }
            clients.retain(|c| !dead.contains(&c.id));
        }
        resend_tiles = false;

        // 4. pace
        let target = Duration::from_secs_f32(1.0 / tps.max(0.1));
        let elapsed = tick_started.elapsed();
        if elapsed < target {
            std::thread::sleep(target - elapsed);
        }
        tick_started = Instant::now();
    }

    println!("worldforge serve: stopping at {}", world.summary());
}
