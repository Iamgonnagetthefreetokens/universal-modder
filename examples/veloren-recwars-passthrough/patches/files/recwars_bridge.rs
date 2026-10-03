//! The Veloren passthrough bridge, guest side (RecWars).
//!
//! RecWars keeps running its own match exactly as before; this module adds a window onto it and a way
//! for the other game to take the wheel:
//!
//! * publishes the simulation (vehicles, projectiles, explosions) as a region the host reads;
//! * publishes the framebuffer, scaled down, so the host can show the real match on a screen in its
//!   world — the same idea as the Minecraft x GTA V reference, with a file region instead of a shared
//!   memory handle so it works identically on Linux and Windows;
//! * reads the host's heightfield and builds the arena from it, so the tanks drive on the ground the
//!   host is standing on (a `Map::from_heightfield` on the guest side);
//! * takes the host's input while it holds focus, and gives it back the moment it stops talking.
//!
//! Inert unless `RCW_BRIDGE=1` is set, and compiled out entirely on wasm (no files, no sockets, and
//! the browser build has no business opening either). Hooks are marked `// recwars:` with the file and
//! line they belong to.
//!
//! ```text
//! RCW_BRIDGE=1              enable the guest side (or `--bridge` on the command line)
//! RCW_BRIDGE_PORT=47811     control-channel port
//! RCW_BRIDGE_RUN_DIR=…      run directory (must match the host's)
//! RCW_BRIDGE_SCALE=2        publish at 1/2 resolution (frame cost is dominated by the readback)
//! RCW_BRIDGE_EVERY=3        publish every 3rd frame (~20 Hz at 60 fps)
//! ```

#![cfg(not(target_arch = "wasm32"))]

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Instant;

use macroquad::prelude::*;
use tracing::{debug, info, warn};

use crate::client::Client;
use crate::entities::{ClientType, VehicleType, Weapon};
use crate::input::ClientInput;
use crate::map::{Map, Surface, SurfaceKind, Tile, TILE_SIZE};
use crate::prelude::*;
use um_bridge::mapping::{self, Cell, TerrainHeader};
use um_bridge::{Bridge, BridgeConfig, Entity as GuestEntity, Event as GuestEvent, FrameHeader,
                 Role, StateHeader, WatchdogState};

/// Set by `main()` from the environment or `--bridge`.
pub static ENABLED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// True when the guest side should run at all.
pub fn enabled() -> bool {
    ENABLED.load(std::sync::atomic::Ordering::Relaxed)
}

/// The guest-side bridge.
pub struct GuestBridge {
    bridge: Bridge,
    started: Instant,
    last_state: f64,
    last_frame: f64,
    last_report: f64,
    frame_index: u64,
    /// Host-routed input for the current frame, while the host holds focus.
    focus: bool,
    held: HashMap<String, bool>,
    /// The map the host's terrain produced, waiting to be installed on the next tick.
    pending_map: Option<Map>,
    /// Stats, logged every 10 s and shown in the console (`bridge` command, phase 2).
    updates: u64,
    events: u64,
    frames: u64,
    input_events: u64,
    warned_offline: bool,
    /// Entities and events seen last frame, so events are delivered exactly once.
    alive_vehicles: std::collections::HashSet<Index>,
    seen_projectiles: std::collections::HashSet<Index>,
    seen_deaths: std::collections::HashSet<Index>,
    last_explosion_time: f64,
    pending_events: Vec<GuestEvent>,
}

impl GuestBridge {
    /// Start the guest side. Returns `None` (with a log line) if anything is wrong; the game then
    /// runs exactly as it always did.
    pub fn start() -> Option<Self> {
        let mut config = BridgeConfig::default();
        if let Ok(dir) = std::env::var("RCW_BRIDGE_RUN_DIR") {
            config.run_dir = PathBuf::from(dir);
        }
        if let Ok(port) = std::env::var("RCW_BRIDGE_PORT") {
            if let Ok(port) = port.parse() {
                config.port = port;
            }
        }
        match Bridge::new(Role::Guest, config) {
            Ok(bridge) => {
                info!(run_dir = %bridge.run_dir().display(), "bridge: guest side started, waiting for the host");
                Some(Self {
                    bridge,
                    started: Instant::now(),
                    last_state: 0.0,
                    last_frame: 0.0,
                    last_report: 0.0,
                    frame_index: 0,
                    focus: false,
                    held: HashMap::new(),
                    pending_map: None,
                    updates: 0,
                    events: 0,
                    frames: 0,
                    input_events: 0,
                    warned_offline: false,
                    alive_vehicles: std::collections::HashSet::new(),
                    seen_projectiles: std::collections::HashSet::new(),
                    seen_deaths: std::collections::HashSet::new(),
                    last_explosion_time: 0.0,
                    pending_events: Vec::new(),
                })
            }
            Err(err) => {
                warn!(?err, "bridge: could not start; RecWars runs unmodified");
                None
            }
        }
    }

    /// How often to publish. Both are env-tunable because the frame readback is the expensive part:
    /// the game's own comment (`src/client.rs:314`) measures `get_screen_data()` at 10-20 ms at
    /// 1600x900, which is why we publish a scaled-down frame and only every few frames.
    fn frame_every() -> u64 {
        std::env::var("RCW_BRIDGE_EVERY")
            .ok()
            .and_then(|v| v.parse().ok())
            .filter(|v| *v > 0)
            .unwrap_or(3)
    }

    fn scale() -> u32 {
        std::env::var("RCW_BRIDGE_SCALE")
            .ok()
            .and_then(|v| v.parse().ok())
            .filter(|v| *v > 1)
            .unwrap_or(2)
    }

    /// One frame. Call after `Client::render` and before `next_frame().await` (`src/main.rs:409`).
    pub fn tick(&mut self, client: &mut Client, dt: f64) {
        let now = self.started.elapsed().as_secs_f64();
        self.frame_index += 1;

        // 1. the host's control lines
        for msg in self.bridge.pump() {
            match msg.kind.as_str() {
                "HELLO" => {
                    let pid = std::process::id().to_string();
                    let (w, h) = (client.viewport_size.x as u32, client.viewport_size.y as u32);
                    let geometry = format!("{w}x{h}");
                    self.bridge.send(
                        "WELCOME",
                        &[
                            ("v", "1"),
                            ("role", "guest"),
                            ("app", "recwars"),
                            ("pid", pid.as_str()),
                            ("tick_hz", "30"),
                            ("frame", geometry.as_str()),
                        ],
                    );
                    info!(app = msg.get("app"), "bridge: host connected");
                    self.warned_offline = false;
                }
                "FOCUS" => {
                    self.focus = msg.get_bool("on");
                    if !self.focus {
                        self.held.clear(); // never leave a tank driving by itself
                    }
                    info!(focus = self.focus, "bridge: input focus changed");
                }
                "IN" => {
                    self.held.insert(msg.get("a").to_string(), msg.get_bool("d"));
                    self.input_events += 1;
                }
                "MOUSE" => {
                    // The host sends deltas, so the guest aims by turning the turret towards them.
                    let dx = msg.get_f32("dx");
                    let dy = msg.get_f32("dy");
                    // The guest's turret follows the host's mouse, scaled to the two viewports.
                    let previous = self.held.get("turret_l").copied().unwrap_or(false)
                        || self.held.get("turret_r").copied().unwrap_or(false);
                    let turn = dx.abs() + dy.abs() > 0.5 || previous;
                    self.held.insert("turret_l".to_string(), turn && dx < 0.0);
                    self.held.insert("turret_r".to_string(), turn && dx > 0.0);
                }
                "MAP" => self.read_terrain(),
                "BYE" => warn!(reason = msg.get("reason"), "bridge: host left"),
                _ => debug!(kind = %msg.kind, "bridge: message"),
            }
        }

        // 2. install the map the host's terrain produced (never mid-render: this is safe, the map is
        //    swapped between frames, and vehicles are nudged out of walls by the builder)
        if let Some(map) = self.pending_map.take() {
            client.map = map;
            info!("bridge: arena rebuilt from the host's terrain");
        }

        // 3. the host's input, if it has focus. `client.cg.input1` is what `Client::cl_input` would
        //    have written from the keyboard (`src/client.rs:293`), so the rest of the game is
        //    unchanged and unaware.
        if self.focus {
            let mut input = ClientInput::default();
            input.left = self.held.get("left").copied().unwrap_or(false);
            input.right = self.held.get("right").copied().unwrap_or(false);
            input.up = self.held.get("forward").copied().unwrap_or(false);
            input.down = self.held.get("back").copied().unwrap_or(false);
            input.fire = self.held.get("fire").copied().unwrap_or(false);
            input.mine = self.held.get("mine").copied().unwrap_or(false);
            input.horn = self.held.get("horn").copied().unwrap_or(false);
            input.self_destruct = self.held.get("self_destruct").copied().unwrap_or(false);
            input.turret_left = self.held.get("turret_l").copied().unwrap_or(false);
            input.turret_right = self.held.get("turret_r").copied().unwrap_or(false);
            input.prev_weapon = self.held.get("prev_weapon").copied().unwrap_or(false);
            input.next_weapon = self.held.get("next_weapon").copied().unwrap_or(false);
            client.cg.input1 = input;
        }

        // 4. publish the simulation
        if now - self.last_state >= 1.0 / 30.0 {
            self.last_state = now;
            let (entities, events) = export(self, client);
            self.events += events.len() as u64;
            let local = client
                .gs
                .players
                .get(client.cg.tmp_local_player_handle)
                .and_then(|player| player.vehicle)
                .map(|index| entity_id(index))
                .unwrap_or(u32::MAX);
            let (px, py) = client
                .gs
                .players
                .get(client.cg.tmp_local_player_handle)
                .and_then(|player| player.vehicle)
                .and_then(|index| client.gs.vehicles.get(index))
                .map(|vehicle| (vehicle.pos.x as f32, vehicle.pos.y as f32))
                .unwrap_or((0.0, 0.0));
            let header = StateHeader {
                tick: client.gs.frame_num as u32,
                game_time_ms: (client.gs.game_time * 1000.0) as u32,
                focus_ent: if local == u32::MAX { 0xFFFF } else { local as u16 },
                flags: (if local != u32::MAX { 1 } else { 0 }) | if self.focus { 2 } else { 0 },
                player_x: px,
                player_y: py,
                cam_x: px,
                cam_y: py,
                ..StateHeader::default()
            };
            if let Err(err) = self.bridge.publish_state(&header, &entities, &events) {
                debug!(?err, "bridge: state publish failed");
            }
            self.updates += 1;
        }

        // 5. publish a frame of the real game
        if self.frame_index % Self::frame_every() == 0 && now - self.last_frame >= 1.0 / 25.0 {
            self.last_frame = now;
            self.publish_frame();
        }

        // 6. liveness
        if self.bridge.peer_state() == WatchdogState::Dead {
            if !self.warned_offline {
                self.warned_offline = true;
                info!("bridge: host is not responding; input released, playing on");
                self.focus = false;
                self.held.clear();
            }
        } else {
            self.warned_offline = false;
        }
        if now - self.last_report >= 10.0 {
            self.last_report = now;
            info!(
                updates = self.updates,
                frames = self.frames,
                events = self.events,
                input_events = self.input_events,
                focus = self.focus,
                "bridge: report"
            );
        }
        let _ = dt;
    }

    /// Read the host's heightfield and build a RecWars map out of it.
    fn read_terrain(&mut self) {
        match self.bridge.terrain() {
            Ok(Some((header, cells))) => match build_map(&header, &cells) {
                Ok(map) => self.pending_map = Some(map),
                Err(err) => warn!(?err, "bridge: terrain could not become a map"),
            },
            Ok(None) => {}
            Err(err) => debug!(?err, "bridge: terrain not readable yet"),
        }
    }

    /// Grab the framebuffer, scale it down and publish it.
    ///
    /// `get_screen_data()` is the expensive call (10-20 ms at 1600x900, per the game's own comment);
    /// the documented improvement is to render into a `RenderTarget` and read that back instead, which
    /// is why this is one function: swap the source, keep the wire format.
    fn publish_frame(&mut self) {
        let scale = Self::scale();
        let image = get_screen_data();
        let (sw, sh) = (image.width, image.height);
        let (w, h) = ((sw / scale).max(1), (sh / scale).max(1));
        let src = image.bytes.as_slice(); // RGBA8
        let mut pixels = vec![0u8; w * h * 4];
        for y in 0..h {
            for x in 0..w {
                let mut acc = [0u32; 4];
                let mut n = 0u32;
                for dy in 0..scale {
                    for dx in 0..scale {
                        let (sx, sy) = (x * scale + dx, y * scale + dy);
                        if sx >= sw || sy >= sh {
                            continue;
                        }
                        let offset = ((sy * sw + sx) * 4) as usize;
                        for c in 0..4 {
                            acc[c] += u32::from(src[offset + c]);
                        }
                        n += 1;
                    }
                }
                let n = n.max(1);
                let out = ((y * w + x) * 4) as usize;
                // RGBA in, BGRA out (docs/PROTOCOL.md §2.5), straight alpha
                pixels[out] = (acc[2] / n) as u8;
                pixels[out + 1] = (acc[1] / n) as u8;
                pixels[out + 2] = (acc[0] / n) as u8;
                pixels[out + 3] = 255;
            }
        }
        let header = FrameHeader::with_geometry(w as u16, h as u16, (1000 / scale) as u16,
                                                self.started.elapsed().as_micros() as u64);
        match self.bridge.publish_frame(&header, &pixels) {
            Ok(_) => self.frames += 1,
            Err(err) => debug!(?err, "bridge: frame publish failed"),
        }
    }
}

/// Turn the host's grid into a RecWars `Map` (`src/map.rs:13`), keeping every existing system:
/// collision (`Map::is_wall`), AI, spawns and the tile grid all work on it unchanged.
///
/// Spawn points are `SurfaceKind::Spawn` tiles, which is how the game's own maps do it (`Map::new`
/// collects them), so the host's "spawnable" cells become real spawns for the guest.
pub fn build_map(header: &TerrainHeader, cells: &[Cell]) -> Result<Map, String> {
    let nx = header.nx as usize;
    let ny = header.ny as usize;
    if cells.len() != nx * ny {
        return Err(format!("{} cells for a {nx}x{ny} grid", cells.len()));
    }
    // One surface per guest-facing kind, so the game's own surface behaviour (friction, speed,
    // particles) applies to the host's terrain.
    let mut surfaces: Vec<Surface> = Vec::new();
    let mut index_of = |surfaces: &mut Vec<Surface>, name: &'static str| -> usize {
        if let Some(i) = surfaces.iter().position(|s| s.name == name) {
            return i;
        }
        let kind = match name {
            "water" => SurfaceKind::Water,
            "wall" => SurfaceKind::Wall,
            "spawn" => SurfaceKind::Spawn,
            _ => SurfaceKind::Normal,
        };
        let (friction, speed) = match name {
            "water" => (1.6, 0.6),
            "sand" => (0.8, 0.95),
            "snow" => (0.6, 0.9),
            "rock" => (1.0, 1.0),
            "wall" => (1.0, 1.0),
            _ => (0.9, 1.0),
        };
        surfaces.push(Surface {
            name: name.to_string(),
            kind,
            friction,
            speed,
        });
        surfaces.len() - 1
    };

    let mut tiles: Vec<Vec<Tile>> = Vec::with_capacity(ny);
    let mut spawn_tiles = 0usize;
    for r in 0..ny {
        let mut row = Vec::with_capacity(nx);
        for c in 0..nx {
            // Row 0 of the guest is the top, which is the *highest* host y: the grid arrives the
            // other way round (docs/MAPPING.md §3).
            let j = ny - 1 - r;
            let cell = cells[j * nx + c];
            let mut name = mapping::guest_surface_for(cell.kind);
            let walkable = name != "wall" && name != "water";
            // Roughly one spawn per 32 walkable cells, and only where the host said a spawn is
            // reasonable; if the host marked nothing, the same rule still produces a playable map.
            let want_spawn = walkable
                && (cell.flags & mapping::CELL_FLAG_SPAWNABLE != 0
                    || (spawn_tiles == 0 && ((c * 7 + r * 13) % 23 == 0)));
            if want_spawn {
                name = "spawn";
                spawn_tiles += 1;
            }
            let index = index_of(&mut surfaces, name);
            row.push(Tile {
                surface_index: index,
                // The host does not export orientations yet (a v2 idea: flow direction for water,
                // cliff normals).
                angle: 0.0,
            });
        }
        tiles.push(row);
    }
    Ok(Map::from_bridge(
        tiles,
        surfaces,
        format!("bridge-rev{}", header.revision),
    ))
}

/// Snapshot the guest's world as bridge entities and events, straight out of the arenas the game
/// already keeps (`src/game_state.rs:15`, `src/entities.rs`).
///
/// Nothing here mutates the game. Events are derived from state this module watches between frames
/// (new explosions in `cg.explosions`, new projectiles, a vehicle's hp reaching zero), so the guest
/// patch does not have to touch the game's own event handling at all — which is also why this is safe
/// to switch off: with `RCW_BRIDGE` unset, this function is never called.
pub fn export(bridge: &mut GuestBridge, client: &Client) -> (Vec<GuestEntity>, Vec<GuestEvent>) {
    let game_time = client.gs.game_time;
    let now_ms = (game_time * 1000.0) as u32;
    let local_player = client.cg.tmp_local_player_handle;
    let local_vehicle = client
        .gs
        .players
        .get(local_player)
        .and_then(|player| player.vehicle);

    let mut entities = Vec::with_capacity(client.gs.vehicles.len() + client.gs.projectiles.len());
    for (index, vehicle) in client.gs.vehicles.iter() {
        let owner = client.gs.players.get(vehicle.owner);
        let is_bot = matches!(owner.map(|p| p.client), Some(ClientType::Ai(_)));
        let kind = match vehicle.veh_type {
            VehicleType::Tank => um_bridge::protocol::ENTITY_TANK,
            VehicleType::Hovercraft => um_bridge::protocol::ENTITY_HOVER,
            VehicleType::Hummer => um_bridge::protocol::ENTITY_HUMMER,
        };
        let kind = if is_bot {
            um_bridge::protocol::ENTITY_BOT
        } else {
            kind
        };
        let alive = !vehicle.destroyed();
        let flags = (if alive { um_bridge::protocol::ENTITY_FLAG_ALIVE } else { 0 })
            | (if Some(index) == local_vehicle {
                um_bridge::protocol::ENTITY_FLAG_LOCAL_PLAYER
            } else {
                0
            });
        entities.push(GuestEntity {
            id: entity_id(index),
            kind,
            team: 0, // teams are per-player and only in some game modes (`GameMode::Tw`); phase 2
            flags,
            pad: 0,
            x: vehicle.pos.x as f32,
            y: vehicle.pos.y as f32,
            z: 0.0,
            angle: vehicle.angle as f32,
            turret_angle: (vehicle.turret_angle_current - vehicle.angle) as f32,
            vx: vehicle.vel.x as f32,
            vy: vehicle.vel.y as f32,
            hp_frac: vehicle.hp_fraction.clamp(0.0, 1.0) as f32,
        });
        // A vehicle that just died is worth telling the host about (a wreck, a burst, a score)
        if !alive && bridge.alive_vehicles.remove(&index) && !bridge.seen_deaths.contains(&index) {
            bridge.seen_deaths.insert(index);
            let mut event = GuestEvent::default();
            event.kind = um_bridge::protocol::EV_KILL;
            event.t_ms = now_ms;
            event.x = vehicle.pos.x as f32;
            event.y = vehicle.pos.y as f32;
            bridge.pending_events.push(event);
        }
        if alive {
            bridge.alive_vehicles.insert(index);
        }
    }

    for (index, projectile) in client.gs.projectiles.iter() {
        if bridge.seen_projectiles.insert(index) {
            // New projectile = somebody fired. The weapon id rides along in `arg16`.
            let mut event = GuestEvent::default();
            event.kind = um_bridge::protocol::EV_FIRE;
            event.arg16 = weapon_id(projectile.weapon);
            event.t_ms = now_ms;
            event.x = projectile.pos.x as f32;
            event.y = projectile.pos.y as f32;
            bridge.pending_events.push(event);
        }
        entities.push(GuestEntity {
            id: 0x8000_0000 | entity_id(index),
            kind: um_bridge::protocol::ENTITY_PROJECTILE,
            team: 0,
            flags: um_bridge::protocol::ENTITY_FLAG_ALIVE,
            pad: 0,
            x: projectile.pos.x as f32,
            y: projectile.pos.y as f32,
            z: 0.0,
            angle: projectile.angle as f32,
            turret_angle: 0.0,
            vx: projectile.vel.x as f32,
            vy: projectile.vel.y as f32,
            hp_frac: 1.0,
        });
    }
    // Projectiles that vanished (hit something) become explosion events, which is what the host
    // actually draws; the game's own explosion list below carries the size.
    let live: std::collections::HashSet<Index> = client
        .gs
        .projectiles
        .iter()
        .map(|(index, _)| index)
        .collect();
    bridge.seen_projectiles.retain(|index| live.contains(index));

    // The client's own explosion list (`src/game_state.rs:116`, filled at `src/client.rs:452`) is the
    // authoritative source for size and position, including ones the game spawned for a hit.
    for explosion in client.cg.explosions.iter() {
        if explosion.start_time <= bridge.last_explosion_time {
            continue;
        }
        let mut event = GuestEvent::default();
        event.kind = um_bridge::protocol::EV_EXPLOSION;
        event.arg16 = explosion.scale.round().clamp(0.0, 65535.0) as u16;
        event.arg = if explosion.bfg { 1.0 } else { 0.0 };
        event.t_ms = (explosion.start_time * 1000.0) as u32;
        event.x = explosion.pos.x as f32;
        event.y = explosion.pos.y as f32;
        bridge.pending_events.push(event);
        bridge.last_explosion_time = bridge.last_explosion_time.max(explosion.start_time);
    }
    if bridge.pending_events.len() > 64 {
        let drop = bridge.pending_events.len() - 64;
        bridge.pending_events.drain(..drop);
    }
    let events = std::mem::take(&mut bridge.pending_events);
    // Keep the "seen" sets from growing without bound in a long match.
    if bridge.seen_projectiles.len() > 4096 {
        bridge.seen_projectiles.clear();
    }
    if bridge.seen_deaths.len() > 4096 {
        bridge.seen_deaths.clear();
    }
    (entities, events)
}

/// A stable id for a thunderdome handle: slot plus generation, so a recycled slot is a new entity.
fn entity_id(index: Index) -> u32 {
    (index.slot() & 0x000F_FFFF) | ((index.generation().get() & 0x7FF) << 20)
}

/// The protocol's weapon ids for the guest's weapons (docs/PROTOCOL.md §2.3, `EV_FIRE.arg16`).
fn weapon_id(weapon: Weapon) -> u16 {
    match weapon {
        Weapon::Mg => 1,
        Weapon::Rail => 2,
        Weapon::Cb => 3,
        Weapon::Rockets => 4,
        Weapon::Hm => 5,
        Weapon::Gm => 6,
        Weapon::Bfg => 7,
    }
}

/// Guest units per tile, so `map.rs` and the bridge agree by construction.
pub const BRIDGE_TILE_SIZE: f64 = TILE_SIZE;
