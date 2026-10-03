//! Payload layouts (docs/PROTOCOL.md §2.3–§2.5).
//!
//! Every struct is written and read byte by byte, little-endian, so the wire format cannot change by
//! accident when a field is added or reordered in Rust. Sizes and field offsets are asserted against
//! the Python reference in `tests/conformance.rs`.

use crate::{BridgeError, Result};

/// Region kind: the guest's simulation state.
pub const REGION_STATE: u16 = 1;
/// Region kind: the host's heightfield.
pub const REGION_TERRAIN: u16 = 2;
/// Region kind: the guest's framebuffer.
pub const REGION_FRAME: u16 = 3;

/// Maximum entities in one state payload.
pub const MAX_ENTITIES: usize = 512;
/// Maximum events in one state payload.
pub const MAX_EVENTS: usize = 64;
/// Maximum framebuffer width.
pub const MAX_FRAME_W: usize = 1920;
/// Maximum framebuffer height.
pub const MAX_FRAME_H: usize = 1200;

/// Size of the state payload header.
pub const STATE_HEADER_SIZE: usize = 32;
/// Size of one entity record.
pub const ENTITY_SIZE: usize = 40;
/// Size of one event record.
pub const EVENT_SIZE: usize = 24;
/// Size of the frame payload header.
pub const FRAME_HEADER_SIZE: usize = 32;

// Entity kinds.
/// No entity / empty slot.
pub const ENTITY_NONE: u8 = 0;
/// A player's character on foot.
pub const ENTITY_PLAYER: u8 = 1;
/// A tank.
pub const ENTITY_TANK: u8 = 2;
/// A hovercraft.
pub const ENTITY_HOVER: u8 = 3;
/// A hummer.
pub const ENTITY_HUMMER: u8 = 4;
/// A bot vehicle.
pub const ENTITY_BOT: u8 = 5;
/// A projectile.
pub const ENTITY_PROJECTILE: u8 = 6;
/// An explosion effect.
pub const ENTITY_EXPLOSION: u8 = 7;
/// A pickup.
pub const ENTITY_PICKUP: u8 = 8;

/// Entity flag: alive.
pub const ENTITY_FLAG_ALIVE: u8 = 1 << 0;
/// Entity flag: firing this tick.
pub const ENTITY_FLAG_FIRING: u8 = 1 << 1;
/// Entity flag: the guest's local player.
pub const ENTITY_FLAG_LOCAL_PLAYER: u8 = 1 << 2;
/// Entity flag: selected/followed by the host.
pub const ENTITY_FLAG_SELECTED: u8 = 1 << 3;

// Event kinds.
/// A weapon fired.
pub const EV_FIRE: u8 = 1;
/// Something was hit.
pub const EV_HIT: u8 = 2;
/// An explosion.
pub const EV_EXPLOSION: u8 = 3;
/// A vehicle was destroyed.
pub const EV_KILL: u8 = 4;
/// A vehicle spawned or respawned.
pub const EV_SPAWN: u8 = 5;
/// A pickup was taken.
pub const EV_PICKUP: u8 = 6;
/// A self-destruct.
pub const EV_SELF_DESTRUCT: u8 = 7;

// Cell kinds (also used by `mapping`).
/// Land whose kind the host does not know (unloaded chunks). The guest treats it as a wall.
pub const CELL_UNKNOWN: u8 = 0;
/// Water.
pub const CELL_WATER: u8 = 1;
/// Shallow water.
pub const CELL_SHALLOW: u8 = 2;
/// Sand.
pub const CELL_SAND: u8 = 3;
/// Grass.
pub const CELL_GRASS: u8 = 4;
/// Rock.
pub const CELL_ROCK: u8 = 5;
/// A cliff: a wall to the guest.
pub const CELL_CLIFF: u8 = 6;
/// Snow.
pub const CELL_SNOW: u8 = 7;

/// State payload header (32 B).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct StateHeader {
    /// Guest simulation tick.
    pub tick: u32,
    /// Guest game clock in milliseconds.
    pub game_time_ms: u32,
    /// Entity count in the payload.
    pub ent_count: u16,
    /// Event count in the payload.
    pub event_count: u16,
    /// Entity id the host should follow, `0xFFFF` for none.
    pub focus_ent: u16,
    /// Bit 0 local player alive, bit 1 paused, bit 2 match over.
    pub flags: u16,
    /// Local player position, guest units.
    pub player_x: f32,
    /// Local player position, guest units.
    pub player_y: f32,
    /// Guest view centre, guest units.
    pub cam_x: f32,
    /// Guest view centre, guest units.
    pub cam_y: f32,
}

/// One guest entity (40 B).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Entity {
    /// Stable id.
    pub id: u32,
    /// Kind (`ENTITY_*`).
    pub kind: u8,
    /// Team.
    pub team: u8,
    /// Flags (`ENTITY_FLAG_*`).
    pub flags: u8,
    /// Degrees of freedom on foot only on the host; unused by the guest.
    pub pad: u8,
    /// Position in guest units (`x`, `y` in the arena plane; `z` is the guest's own height guess).
    pub x: f32,
    /// See `x`.
    pub y: f32,
    /// See `x`.
    pub z: f32,
    /// Hull angle, radians, guest convention.
    pub angle: f32,
    /// Turret angle, radians, relative to the hull.
    pub turret_angle: f32,
    /// Velocity in guest units per second.
    pub vx: f32,
    /// Velocity in guest units per second.
    pub vy: f32,
    /// Health fraction 0..1.
    pub hp_frac: f32,
}

impl Entity {
    /// Write into a 40-byte buffer at `offset`.
    pub fn write_to(&self, buf: &mut [u8], offset: usize) {
        let mut w = Cursor::at(buf, offset);
        w.u32(self.id);
        w.u8(self.kind);
        w.u8(self.team);
        w.u8(self.flags);
        w.u8(self.pad);
        w.f32(self.x);
        w.f32(self.y);
        w.f32(self.z);
        w.f32(self.angle);
        w.f32(self.turret_angle);
        w.f32(self.vx);
        w.f32(self.vy);
        w.f32(self.hp_frac);
    }

    /// Read from a 40-byte record at `offset`.
    pub fn read_from(buf: &[u8], offset: usize) -> Self {
        let mut r = Cursor::at(buf, offset);
        Self {
            id: r.u32(),
            kind: r.u8(),
            team: r.u8(),
            flags: r.u8(),
            pad: r.u8(),
            x: r.f32(),
            y: r.f32(),
            z: r.f32(),
            angle: r.f32(),
            turret_angle: r.f32(),
            vx: r.f32(),
            vy: r.f32(),
            hp_frac: r.f32(),
        }
    }

    /// True when the entity is alive.
    pub fn alive(&self) -> bool {
        self.flags & ENTITY_FLAG_ALIVE != 0
    }

    /// True when this is the guest's own player.
    pub fn is_local_player(&self) -> bool {
        self.flags & ENTITY_FLAG_LOCAL_PLAYER != 0
    }
}

/// One guest event (24 B).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Event {
    /// Kind (`EV_*`).
    pub kind: u8,
    /// Flags, meaning depends on the kind.
    pub flags: u8,
    /// Radius in guest units (explosions) or weapon id (fire).
    pub arg16: u16,
    /// Guest game time in milliseconds.
    pub t_ms: u32,
    /// Position in guest units.
    pub x: f32,
    /// Position in guest units.
    pub y: f32,
    /// Position in guest units.
    pub z: f32,
    /// Power or damage.
    pub arg: f32,
}

impl Event {
    /// Write into a 24-byte buffer at `offset`.
    pub fn write_to(&self, buf: &mut [u8], offset: usize) {
        let mut w = Cursor::at(buf, offset);
        w.u8(self.kind);
        w.u8(self.flags);
        w.u16(self.arg16);
        w.u32(self.t_ms);
        w.f32(self.x);
        w.f32(self.y);
        w.f32(self.z);
        w.f32(self.arg);
    }

    /// Read from a 24-byte record at `offset`.
    pub fn read_from(buf: &[u8], offset: usize) -> Self {
        let mut r = Cursor::at(buf, offset);
        Self {
            kind: r.u8(),
            flags: r.u8(),
            arg16: r.u16(),
            t_ms: r.u32(),
            x: r.f32(),
            y: r.f32(),
            z: r.f32(),
            arg: r.f32(),
        }
    }

    /// A key that identifies this event across republishes of the ring buffer, so the host applies
    /// each event exactly once (docs/PROTOCOL.md §2.3).
    pub fn once_key(&self) -> (u32, u8, i32, i32) {
        (
            self.t_ms,
            self.kind,
            (self.x * 10.0).round() as i32,
            (self.y * 10.0).round() as i32,
        )
    }
}

/// Frame payload header (32 B).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct FrameHeader {
    /// Published width in pixels.
    pub width: u16,
    /// Published height in pixels.
    pub height: u16,
    /// Bytes per row (`width * 4`).
    pub stride: u32,
    /// Pixel format, `1` = BGRA8.
    pub format: u8,
    /// Bit 0 alpha present, bit 1 letterboxed.
    pub flags: u8,
    /// Published size ÷ window size × 1000.
    pub scale_permille: u16,
    /// Guest monotonic microseconds at publish time.
    pub publish_ts_us: u64,
    /// Guest game tick.
    pub game_tick: u32,
}

impl FrameHeader {
    /// Pack the state payload (header + entities + events). Counts in the header are overwritten.
    pub fn with_geometry(width: u16, height: u16, scale_permille: u16, publish_ts_us: u64) -> Self {
        Self {
            width,
            height,
            stride: width as u32 * 4,
            format: 1,
            flags: 1,
            scale_permille,
            publish_ts_us,
            game_tick: 0,
        }
    }

    /// Bytes of one frame.
    pub fn payload_size(&self) -> usize {
        FRAME_HEADER_SIZE + self.height as usize * self.stride as usize
    }
}

/// Pack a full state payload.
pub fn pack_state(mut header: StateHeader, entities: &[Entity], events: &[Event]) -> Vec<u8> {
    let ents = &entities[..entities.len().min(MAX_ENTITIES)];
    let evs = &events[events.len().saturating_sub(MAX_EVENTS)..];
    header.ent_count = ents.len() as u16;
    header.event_count = evs.len() as u16;
    let mut buf = vec![0u8; STATE_HEADER_SIZE + ents.len() * ENTITY_SIZE + evs.len() * EVENT_SIZE];
    {
        let mut w = Cursor::at(&mut buf, 0);
        w.u32(header.tick);
        w.u32(header.game_time_ms);
        w.u16(header.ent_count);
        w.u16(header.event_count);
        w.u16(header.focus_ent);
        w.u16(header.flags);
        w.f32(header.player_x);
        w.f32(header.player_y);
        w.f32(header.cam_x);
        w.f32(header.cam_y);
    }
    let mut offset = STATE_HEADER_SIZE;
    for e in ents {
        e.write_to(&mut buf, offset);
        offset += ENTITY_SIZE;
    }
    for e in evs {
        e.write_to(&mut buf, offset);
        offset += EVENT_SIZE;
    }
    buf
}

/// Parse a state payload.
pub fn unpack_state(payload: &[u8]) -> Result<(StateHeader, Vec<Entity>, Vec<Event>)> {
    if payload.len() < STATE_HEADER_SIZE {
        return Err(BridgeError::Malformed("state payload shorter than its header"));
    }
    let header = {
        let mut r = Cursor::at(payload, 0);
        StateHeader {
            tick: r.u32(),
            game_time_ms: r.u32(),
            ent_count: r.u16(),
            event_count: r.u16(),
            focus_ent: r.u16(),
            flags: r.u16(),
            player_x: r.f32(),
            player_y: r.f32(),
            cam_x: r.f32(),
            cam_y: r.f32(),
        }
    };
    if header.ent_count as usize > MAX_ENTITIES || header.event_count as usize > MAX_EVENTS {
        return Err(BridgeError::TooLarge("state counts"));
    }
    let need = STATE_HEADER_SIZE
        + header.ent_count as usize * ENTITY_SIZE
        + header.event_count as usize * EVENT_SIZE;
    if payload.len() < need {
        return Err(BridgeError::Malformed("state payload truncated"));
    }
    let mut entities = Vec::with_capacity(header.ent_count as usize);
    let mut offset = STATE_HEADER_SIZE;
    for _ in 0..header.ent_count {
        entities.push(Entity::read_from(payload, offset));
        offset += ENTITY_SIZE;
    }
    let mut events = Vec::with_capacity(header.event_count as usize);
    for _ in 0..header.event_count {
        events.push(Event::read_from(payload, offset));
        offset += EVENT_SIZE;
    }
    Ok((header, entities, events))
}

/// Pack a frame payload (header + BGRA pixels).
pub fn pack_frame(header: &FrameHeader, pixels: &[u8]) -> Result<Vec<u8>> {
    if header.width as usize > MAX_FRAME_W || header.height as usize > MAX_FRAME_H {
        return Err(BridgeError::TooLarge("frame"));
    }
    if header.stride < header.width as u32 * 4 {
        return Err(BridgeError::Malformed("frame stride smaller than a row"));
    }
    let expected = header.height as usize * header.stride as usize;
    if pixels.len() != expected {
        return Err(BridgeError::Malformed("frame pixel buffer size mismatch"));
    }
    let mut buf = Vec::with_capacity(FRAME_HEADER_SIZE + expected);
    buf.resize(FRAME_HEADER_SIZE, 0);
    {
        let mut w = Cursor::at(&mut buf, 0);
        w.u16(header.width);
        w.u16(header.height);
        w.u32(header.stride);
        w.u8(header.format);
        w.u8(header.flags);
        w.u16(header.scale_permille);
        w.u32(0);
        w.u64(header.publish_ts_us);
        w.u32(header.game_tick);
        w.u32(0);
    }
    buf.extend_from_slice(pixels);
    Ok(buf)
}

/// Parse a frame payload.
pub fn unpack_frame(payload: &[u8]) -> Result<(FrameHeader, &[u8])> {
    if payload.len() < FRAME_HEADER_SIZE {
        return Err(BridgeError::Malformed("frame payload shorter than its header"));
    }
    let header = {
        let mut r = Cursor::at(payload, 0);
        let width = r.u16();
        let height = r.u16();
        let stride = r.u32();
        let format = r.u8();
        let flags = r.u8();
        let scale_permille = r.u16();
        let _reserved = r.u32();
        let publish_ts_us = r.u64();
        let game_tick = r.u32();
        let _reserved2 = r.u32();
        FrameHeader {
            width,
            height,
            stride,
            format,
            flags,
            scale_permille,
            publish_ts_us,
            game_tick,
        }
    };
    if header.width as usize > MAX_FRAME_W || header.height as usize > MAX_FRAME_H {
        return Err(BridgeError::TooLarge("frame"));
    }
    if header.stride < header.width as u32 * 4 {
        return Err(BridgeError::Malformed("frame stride smaller than a row"));
    }
    let need = FRAME_HEADER_SIZE + header.height as usize * header.stride as usize;
    if payload.len() < need {
        return Err(BridgeError::Malformed("frame payload truncated"));
    }
    Ok((header, &payload[FRAME_HEADER_SIZE..need]))
}

/// Little-endian byte reader/writer, bounds-checked by construction (callers size their buffers).
pub struct Cursor<'a> {
    buf: &'a mut [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    /// A cursor at `offset` of `buf`.
    pub fn at(buf: &'a mut [u8], offset: usize) -> Self {
        Self { buf, pos: offset }
    }

    /// Write `u8`.
    pub fn u8(&mut self, v: u8) {
        if self.pos < self.buf.len() {
            self.buf[self.pos] = v;
        }
        self.pos += 1;
    }

    /// Write `u16`.
    pub fn u16(&mut self, v: u16) {
        self.bytes(&v.to_le_bytes());
    }

    /// Write `u32`.
    pub fn u32(&mut self, v: u32) {
        self.bytes(&v.to_le_bytes());
    }

    /// Write `u64`.
    pub fn u64(&mut self, v: u64) {
        self.bytes(&v.to_le_bytes());
    }

    /// Write `f32`.
    pub fn f32(&mut self, v: f32) {
        self.bytes(&v.to_le_bytes());
    }

    fn bytes(&mut self, b: &[u8]) {
        for (i, v) in b.iter().enumerate() {
            if self.pos + i < self.buf.len() {
                self.buf[self.pos + i] = *v;
            }
        }
        self.pos += b.len();
    }

    /// Read `u8`.
    pub fn u8(&mut self) -> u8 {
        let v = self.buf.get(self.pos).copied().unwrap_or(0);
        self.pos += 1;
        v
    }

    /// Read `u16`.
    pub fn u16(&mut self) -> u16 {
        u16::from_le_bytes(self.arr::<2>())
    }

    /// Read `u32`.
    pub fn u32(&mut self) -> u32 {
        u32::from_le_bytes(self.arr::<4>())
    }

    /// Read `u64`.
    pub fn u64(&mut self) -> u64 {
        u64::from_le_bytes(self.arr::<8>())
    }

    /// Read `f32`.
    pub fn f32(&mut self) -> f32 {
        f32::from_le_bytes(self.arr::<4>())
    }

    fn arr<const N: usize>(&mut self) -> [u8; N] {
        let mut out = [0u8; N];
        for (i, slot) in out.iter_mut().enumerate() {
            *slot = self.buf.get(self.pos + i).copied().unwrap_or(0);
        }
        self.pos += N;
        out
    }
}
