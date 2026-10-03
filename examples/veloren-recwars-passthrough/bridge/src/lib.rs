//! um-bridge — the Veloren × RecWars passthrough bridge.
//!
//! One crate, linked into both games. The host (Veloren's `voxygen`) and the guest (RecWars) each
//! hold a [`Bridge`] and call it from the loop they already run; nothing else changes about how either
//! game works. See `docs/PROTOCOL.md` (wire format), `docs/MAPPING.md` (units and axes) and
//! `docs/ARCHITECTURE.md` (threading, budgets, failure modes) in this example.
//!
//! ```
//! use um_bridge::{Bridge, Role, BridgeConfig};
//!
//! // Host side (Veloren): connect to the guest and keep the regions up to date.
//! # fn host_example() -> Result<(), um_bridge::BridgeError> {
//! let mut host = Bridge::new(Role::Host, BridgeConfig::default())?;
//! host.publish_camera(1032.5, 2487.5, 12.0, -1.5708, 0.0, 1.1, (1600, 900))?;
//! for entity in host.guest_entities()? { let _ = entity.host_pos; }
//! # Ok(()) }
//! ```
//!
//! Both sides use the same types on purpose: a Rust host and a Rust guest means the protocol is
//! defined once, and the Python reference (`fakes/bridge.py`) plus the shared vectors
//! (`testdata/vectors.rs`) keep it honest from outside.
//!
//! # What each side does per frame
//!
//! | Host (Veloren) | Guest (RecWars) |
//! |---|---|
//! | `pump()` — read control lines | `pump()` — read control lines |
//! | `publish_camera(..)` 20 Hz | `publish_state(..)` 30 Hz |
//! | `export_terrain(cells)` every 2 s | `publish_frame(..)` ≤ 20 Hz |
//! | `guest_entities()` / `guest_events()` | `terrain()` (rebuild the map when `revision` changes) |
//! | `read_guest_frame()` for the holotable texture | `input()` — host-routed actions when focused |
//!
//! Nothing here blocks: every call is non-blocking or bounded, and a dead peer degrades to
//! "no guest"/"no host" instead of stalling a frame (see [`Watchdog`]).

#![deny(unsafe_code)]
#![warn(missing_docs)]

pub mod control;
pub mod crc32;
pub mod mapping;
pub mod protocol;
pub mod region;
pub mod watchdog;

pub use control::{Msg, Peer};
pub use mapping::{Cell, TerrainHeader, U2M, TILE_M, TILE_UNITS};
pub use protocol::{
    Entity, Event, FrameHeader, StateHeader, CELL_*, ENTITY_*, EV_*, REGION_FRAME, REGION_STATE,
    REGION_TERRAIN,
};
pub use region::{ReadResult, Region};
pub use watchdog::{Watchdog, WatchdogState};

use std::path::{Path, PathBuf};

/// Wire protocol version. Both sides must agree or the handshake fails loudly.
pub const PROTOCOL_VERSION: u16 = 1;

/// Region header magic: `"UMBR"`.
pub const MAGIC: [u8; 4] = *b"UMBR";

/// Default control-channel port on loopback.
pub const DEFAULT_PORT: u16 = 47811;

/// Which side of the bridge this process is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// The game with the world and the camera (Veloren). Connects out, reads state, writes terrain.
    Host,
    /// The game with the arena (RecWars). Listens, writes state, reads terrain and input.
    Guest,
}

/// Everything the bridge needs to know to start.
#[derive(Debug, Clone)]
pub struct BridgeConfig {
    /// Directory holding the region files. Created if missing.
    pub run_dir: PathBuf,
    /// Control-channel TCP port on `127.0.0.1`.
    pub port: u16,
    /// Time without any traffic at all before the peer counts as dead.
    pub dead_after_ms: f64,
    /// Time without traffic before the peer counts as stale (freeze the last frame).
    pub stale_after_ms: f64,
    /// State region slot size (docs/PROTOCOL.md §2.3).
    pub state_region_size: usize,
    /// Terrain region slot size.
    pub terrain_region_size: usize,
    /// Frame region slot size; the guest picks this from the publish geometry.
    pub frame_region_size: usize,
}

impl Default for BridgeConfig {
    fn default() -> Self {
        Self {
            run_dir: default_run_dir(),
            port: DEFAULT_PORT,
            dead_after_ms: 2000.0,
            stale_after_ms: 600.0,
            state_region_size: protocol::STATE_HEADER_SIZE
                + protocol::MAX_ENTITIES * protocol::ENTITY_SIZE
                + protocol::MAX_EVENTS * protocol::EVENT_SIZE,
            terrain_region_size: protocol::TERRAIN_HEADER_SIZE
                + mapping::MAX_TERRAIN * mapping::MAX_TERRAIN * mapping::CELL_SIZE,
            frame_region_size: protocol::FRAME_HEADER_SIZE + 1280 * 720 * 4,
        }
    }
}

/// The run directory: `$XDG_RUNTIME_DIR/um-bridge/veloren-recwars` on Linux,
/// `%LOCALAPPDATA%\um-bridge\veloren-recwars` on Windows.
pub fn default_run_dir() -> PathBuf {
    #[cfg(windows)]
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    #[cfg(not(windows))]
    let base = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    base.join("um-bridge").join("veloren-recwars")
}

/// Anything that can go wrong. All of it is recoverable by ignoring the bridge for a frame.
#[derive(Debug)]
pub enum BridgeError {
    /// Underlying file or socket error.
    Io(std::io::Error),
    /// A region file header did not start with `UMBR`.
    BadMagic,
    /// A region file was written by another protocol version.
    BadVersion(u16),
    /// A region file holds the wrong kind of payload.
    BadKind(u16),
    /// A payload exceeded the size the protocol allows (counts, dimensions, or the slot size).
    TooLarge(&'static str),
    /// The control peer is not reachable yet (the guest may not have started).
    NoPeer,
    /// A region read could not get a consistent snapshot within the retry budget.
    Torn,
    /// Nothing has been written to the region yet.
    Empty,
    /// The payload did not parse (truncated, or counts out of range).
    Malformed(&'static str),
}

impl std::fmt::Display for BridgeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "io: {e}"),
            Self::BadMagic => write!(f, "region does not start with UMBR"),
            Self::BadVersion(v) => write!(f, "protocol version {v} != {PROTOCOL_VERSION}"),
            Self::BadKind(k) => write!(f, "region kind {k} does not match"),
            Self::TooLarge(what) => write!(f, "{what} is too large for the protocol"),
            Self::NoPeer => write!(f, "no control peer yet"),
            Self::Torn => write!(f, "region read was torn"),
            Self::Empty => write!(f, "region has no committed payload"),
            Self::Malformed(what) => write!(f, "malformed payload: {what}"),
        }
    }
}

impl std::error::Error for BridgeError {}

impl From<std::io::Error> for BridgeError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

/// Convenience result type used across the crate.
pub type Result<T> = std::result::Result<T, BridgeError>;

/// One side's bridge: socket, regions and watchdog.
///
/// The host and guest use the same struct; [`Role`] decides which regions are written and which are
/// read. Create it once (at startup, or when the user enables the bridge) and call the small set of
/// non-blocking methods from the frame loop.
pub struct Bridge {
    /// Which side this is.
    pub role: Role,
    config: BridgeConfig,
    peer: Option<Peer>,
    listener: Option<control::Listener>,
    state: Option<Region>,
    terrain: Option<Region>,
    frame: Option<Region>,
    watchdog: Watchdog,
    last_terrain: TerrainHeader,
    /// Counters for the host's HUD and for tests.
    pub stats: BridgeStats,
}

/// Counters, so a mod does not have to guess whether the bridge is doing anything.
#[derive(Debug, Default, Clone)]
pub struct BridgeStats {
    /// Control messages parsed.
    pub messages_in: u64,
    /// Control messages written.
    pub messages_out: u64,
    /// State payloads committed (guest) or read (host).
    pub state_commits: u64,
    /// Frame payloads committed (guest) or read (host).
    pub frame_commits: u64,
    /// Terrain payloads committed (host) or read (guest).
    pub terrain_commits: u64,
    /// Region reads that failed a consistency check and were retried.
    pub torn_reads: u64,
    /// Guest events handed to the host since the bridge was created.
    pub events_delivered: u64,
    /// Control lines dropped because the socket was not writable.
    pub dropped_messages: u64,
}

impl Bridge {
    /// Start a bridge for `role`. Creates the run directory and the regions this role writes.
    pub fn new(role: Role, config: BridgeConfig) -> Result<Self> {
        std::fs::create_dir_all(&config.run_dir)?;
        let mut bridge = Self {
            role,
            listener: None,
            peer: None,
            state: None,
            terrain: None,
            frame: None,
            watchdog: Watchdog::new(config.stale_after_ms, config.dead_after_ms),
            last_terrain: TerrainHeader::default(),
            stats: BridgeStats::default(),
            config,
        };
        match role {
            Role::Guest => {
                bridge.listener = Some(control::Listener::bind(bridge.config.port)?);
                bridge.state = Some(Region::create(
                    &bridge.region_path("state"),
                    REGION_STATE,
                    bridge.config.state_region_size,
                )?);
                bridge.frame = Some(Region::create(
                    &bridge.region_path("frame"),
                    REGION_FRAME,
                    bridge.config.frame_region_size,
                )?);
            }
            Role::Host => {
                bridge.terrain = Some(Region::create(
                    &bridge.region_path("terrain"),
                    REGION_TERRAIN,
                    bridge.config.terrain_region_size,
                )?);
            }
        }
        Ok(bridge)
    }

    /// Path of a region file inside the run directory.
    pub fn region_path(&self, which: &str) -> PathBuf {
        self.config.run_dir.join(format!("{which}.region"))
    }

    /// The run directory in use.
    pub fn run_dir(&self) -> &Path {
        &self.config.run_dir
    }

    /// True when a control peer is connected and not known to be dead.
    pub fn connected(&self) -> bool {
        self.peer.is_some() && self.watchdog.state() != WatchdogState::Dead
    }

    /// How the peer looks right now.
    pub fn peer_state(&self) -> WatchdogState {
        if self.peer.is_none() {
            WatchdogState::Dead
        } else {
            self.watchdog.state()
        }
    }

    /// Read whatever the peer sent since the last call. Never blocks.
    pub fn pump(&mut self) -> Vec<Msg> {
        if self.role == Role::Guest && self.peer.is_none() {
            if let Some(listener) = &mut self.listener {
                if let Ok(Some(peer)) = listener.try_accept() {
                    self.peer = Some(peer);
                }
                // a refused non-loopback connection is counted in `listener.refused`, never fatal
            }
        }
        let mut out = Vec::new();
        if let Some(peer) = &mut self.peer {
            out = peer.pump();
            if !out.is_empty() {
                self.watchdog.beat();
            }
        }
        if self.peer.as_ref().map(|p| p.is_closed()).unwrap_or(false) {
            self.peer = None;
        }
        let mut pongs: Vec<String> = Vec::new();
        for msg in &out {
            self.stats.messages_in += 1;
            if msg.kind == "PING" {
                pongs.push(msg.get_i64("t_us").to_string());
            }
        }
        for t in pongs {
            self.send("PONG", &[("t_us", t.as_str())]);
        }
        out
    }

    /// Send a control message. Ignored (and counted) if there is no writable peer.
    pub fn send(&mut self, kind: &str, fields: &[(&str, &str)]) {
        let ok = match &self.peer {
            Some(peer) => peer.send(kind, fields),
            None => return,
        };
        if ok {
            self.stats.messages_out += 1;
        } else {
            self.stats.dropped_messages += 1;
        }
    }

    /// Host: tell the guest where the player is looking (docs/PROTOCOL.md §3, `CAM`).
    #[allow(clippy::too_many_arguments)]
    pub fn publish_camera(
        &mut self,
        x: f32,
        y: f32,
        z: f32,
        yaw: f32,
        pitch: f32,
        fov: f32,
        screen: (u32, u32),
    ) {
        let (sx, sy) = screen;
        let (t, x, y, z) = (
            (now_us() / 1000).to_string(),
            format!("{x:.3}"),
            format!("{y:.3}"),
            format!("{z:.3}"),
        );
        let (yaw, pitch, fov) = (
            format!("{yaw:.5}"),
            format!("{pitch:.5}"),
            format!("{fov:.5}"),
        );
        let (sx, sy) = (sx.to_string(), sy.to_string());
        let fields: [(&str, &str); 9] = [
            ("t", t.as_str()),
            ("x", x.as_str()),
            ("y", y.as_str()),
            ("z", z.as_str()),
            ("yaw", yaw.as_str()),
            ("pitch", pitch.as_str()),
            ("fov", fov.as_str()),
            ("sx", sx.as_str()),
            ("sy", sy.as_str()),
        ];
        self.send("CAM", &fields);
    }

    /// Host: write the heightfield around the player and tell the guest it changed
    /// (docs/PROTOCOL.md §2.4). `cells` is row-major with `j` increasing with host `y`.
    pub fn export_terrain(&mut self, header: &TerrainHeader, cells: &[Cell]) -> Result<()> {
        if self.role != Role::Host {
            return Err(BridgeError::Malformed("only the host exports terrain"));
        }
        let terrain = self
            .terrain
            .as_mut()
            .ok_or(BridgeError::Malformed("this role does not write terrain"))?;
        let payload = mapping::pack_terrain(header, cells)?;
        let _ = terrain.write(&payload)?;
        self.last_terrain = *header;
        self.stats.terrain_commits += 1;
        let rev = header.revision.to_string();
        let cells_n = header.nx.to_string();
        let cell_m = format!("{:.3}", header.cell_m);
        let origin_x = format!("{:.3}", header.origin_x);
        let origin_y = format!("{:.3}", header.origin_y);
        let fields: [(&str, &str); 5] = [
            ("rev", rev.as_str()),
            ("cells", cells_n.as_str()),
            ("cell_m", cell_m.as_str()),
            ("origin_x", origin_x.as_str()),
            ("origin_y", origin_y.as_str()),
        ];
        self.send("MAP", &fields);
        Ok(())
    }

    /// Guest: read the terrain the host exported, if a newer revision is available.
    ///
    /// The host gets `Ok(None)`: it is the one writing terrain, and reading its own region would
    /// confuse "the terrain I published" with "the terrain I received".
    pub fn terrain(&mut self) -> Result<Option<(TerrainHeader, Vec<Cell>)>> {
        if self.role != Role::Guest {
            return Ok(None);
        }
        if self.terrain.is_none() {
            let path = self.region_path("terrain");
            if !path.exists() {
                return Ok(None);
            }
            self.terrain = Some(Region::open(&path, REGION_TERRAIN)?);
        }
        let region = self.terrain.as_mut().expect("just opened");
        match region.read() {
            Ok(Some(res)) => {
                let parsed = mapping::unpack_terrain(&res.payload)?;
                self.last_terrain = parsed.0;
                self.stats.terrain_commits += 1;
                Ok(Some(parsed))
            }
            Ok(None) => Ok(None),
            Err(BridgeError::Torn) => {
                self.stats.torn_reads += 1;
                Ok(None)
            }
            Err(e) => Err(e),
        }
    }

    /// Guest: publish the simulation (docs/PROTOCOL.md §2.3). Returns the region sequence.
    pub fn publish_state(
        &mut self,
        header: &StateHeader,
        entities: &[Entity],
        events: &[Event],
    ) -> Result<u64> {
        if self.role != Role::Guest {
            return Err(BridgeError::Malformed("only the guest publishes state"));
        }
        if entities.len() > protocol::MAX_ENTITIES || events.len() > protocol::MAX_EVENTS {
            return Err(BridgeError::TooLarge("state"));
        }
        let payload = protocol::pack_state(header, entities, events);
        let state = self
            .state
            .as_mut()
            .ok_or(BridgeError::Malformed("this role does not write state"))?;
        let seq = state.write(&payload)?;
        self.stats.state_commits += 1;
        let seq_s = seq.to_string();
        self.send("NOTIFY", &[("kind", "state"), ("seq", seq_s.as_str())]);
        Ok(seq)
    }

    /// Host: the guest's latest simulation, if it changed since the last call.
    pub fn guest_state(&mut self) -> Result<Option<(StateHeader, Vec<Entity>, Vec<Event>)>> {
        if self.role != Role::Host {
            return Ok(None);
        }
        if self.state.is_none() {
            let path = self.region_path("state");
            if !path.exists() {
                return Ok(None);
            }
            self.state = Some(Region::open(&path, REGION_STATE)?);
        }
        let region = self.state.as_mut().expect("just opened");
        match region.read() {
            Ok(Some(res)) => {
                let parsed = protocol::unpack_state(&res.payload)?;
                self.stats.state_commits += 1;
                self.watchdog.beat();
                Ok(Some(parsed))
            }
            Ok(None) => Ok(None),
            Err(BridgeError::Torn) => {
                self.stats.torn_reads += 1;
                Ok(None)
            }
            Err(e) => Err(e),
        }
    }

    /// Host: the guest's entities, each already placed in the host's world.
    ///
    /// This is the convenience the host actually wants: guest units converted to metres, angles to
    /// Veloren yaw, and the position clamped to the exported region (docs/MAPPING.md §3–§4).
    pub fn guest_entities(&mut self) -> Result<Vec<PlacedEntity>> {
        let Some((_header, entities, events)) = self.guest_state()? else {
            return Ok(Vec::new());
        };
        self.stats.events_delivered += events.len() as u64;
        // The host placed the terrain, so it is the authority on where the guest's arena sits in its
        // world: a guest that has not caught up yet cannot drag the sprites somewhere else.
        let terrain = self.last_terrain;
        Ok(entities.iter().map(|e| self.placed(e, &terrain)).collect())
    }

    /// Host: place one guest entity using the last terrain the host exported.
    pub fn placed(&self, entity: &Entity, terrain: &TerrainHeader) -> PlacedEntity {
        let (x, y) = mapping::guest_to_host(entity.x, entity.y, terrain);
        PlacedEntity {
            id: entity.id,
            kind: entity.kind,
            team: entity.team,
            flags: entity.flags,
            host_pos: [x, entity.z, y], // note the axis order: Veloren is (x, y, z) with z up
            yaw: mapping::guest_angle_to_yaw(entity.angle),
            turret_yaw: mapping::guest_angle_to_yaw(entity.angle + entity.turret_angle),
            velocity: [entity.vx * U2M, 0.0, entity.vy * U2M],
            speed: (entity.vx * entity.vx + entity.vy * entity.vy).sqrt(),
            hp_frac: entity.hp_frac,
        }
    }

    /// The last terrain header this bridge exported (host) or received (guest).
    pub fn last_terrain(&self) -> &TerrainHeader {
        &self.last_terrain
    }

    /// Guest: publish a frame (docs/PROTOCOL.md §2.5). `pixels` is BGRA8, top row first.
    pub fn publish_frame(&mut self, header: &FrameHeader, pixels: &[u8]) -> Result<u64> {
        if self.role != Role::Guest {
            return Err(BridgeError::Malformed("only the guest publishes frames"));
        }
        let payload = protocol::pack_frame(header, pixels)?;
        let frame = self
            .frame
            .as_mut()
            .ok_or(BridgeError::Malformed("this role does not write frames"))?;
        let seq = frame.write(&payload)?;
        self.stats.frame_commits += 1;
        Ok(seq)
    }

    /// Host: the guest's latest frame, if it changed. Returns the header and BGRA pixels.
    pub fn guest_frame(&mut self) -> Result<Option<(FrameHeader, Vec<u8>)>> {
        if self.role != Role::Host {
            return Ok(None);
        }
        if self.frame.is_none() {
            let path = self.region_path("frame");
            if !path.exists() {
                return Ok(None);
            }
            self.frame = Some(Region::open(&path, REGION_FRAME)?);
        }
        let region = self.frame.as_mut().expect("just opened");
        match region.read() {
            Ok(Some(res)) => {
                let parsed = protocol::unpack_frame(&res.payload)?;
                self.stats.frame_commits += 1;
                self.watchdog.beat();
                Ok(Some(parsed))
            }
            Ok(None) => Ok(None),
            Err(BridgeError::Torn) => {
                self.stats.torn_reads += 1;
                Ok(None)
            }
            Err(e) => Err(e),
        }
    }

    /// Host: hand input focus to the guest (`FOCUS`).
    pub fn set_focus(&mut self, on: bool) {
        self.send("FOCUS", &[("on", if on { "1" } else { "0" })]);
    }

    /// Host: forward one action press/release (`IN`). Names are the guest's own actions.
    pub fn send_action(&mut self, action: &str, down: bool) {
        let t = (now_us() / 1000).to_string();
        let d = if down { "1" } else { "0" };
        self.send("IN", &[("a", action), ("d", d), ("t", t.as_str())]);
    }

    /// Host: forward a mouse delta (`MOUSE`) — deltas, never absolute pixels.
    pub fn send_mouse(&mut self, dx: f32, dy: f32) {
        let t = (now_us() / 1000).to_string();
        let dx = format!("{dx:.2}");
        let dy = format!("{dy:.2}");
        self.send(
            "MOUSE",
            &[("t", t.as_str()), ("dx", dx.as_str()), ("dy", dy.as_str())],
        );
    }

    /// Tell the peer this side is shutting down cleanly.
    pub fn bye(&mut self, reason: &str) {
        self.send("BYE", &[("reason", reason)]);
        if let Some(region) = self.state.as_mut() {
            region.close_clean();
        }
        if let Some(region) = self.frame.as_mut() {
            region.close_clean();
        }
        if let Some(region) = self.terrain.as_mut() {
            region.close_clean();
        }
    }
}

/// A guest entity placed in the host's world.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlacedEntity {
    /// Guest entity id.
    pub id: u32,
    /// Guest kind (see `ENTITY_*`).
    pub kind: u8,
    /// Guest team.
    pub team: u8,
    /// Guest flags (bit 2 = the guest's local player).
    pub flags: u8,
    /// Host position in metres, in Veloren's axis order `[x, y, z]` with `z` up. `y` is the guest's
    /// own height guess: the host replaces it with its own terrain sample unless the ground is
    /// unloaded (docs/MAPPING.md §7).
    pub host_pos: [f32; 3],
    /// Hull yaw in Veloren's convention (radians, `Dir::forward() = +y`).
    pub yaw: f32,
    /// Turret yaw in Veloren's convention (already includes the hull).
    pub turret_yaw: f32,
    /// Velocity in metres per second, `[x, y, z]` (always 0 in `z`).
    pub velocity: [f32; 3],
    /// Speed in metres per second.
    pub speed: f32,
    /// Health fraction 0..1.
    pub hp_frac: f32,
}

/// Milliseconds since the Unix epoch, used for control-message timestamps.
pub fn now_us() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_micros() as u64)
        .unwrap_or(0)
}
