//! Units, axes and the guest's tile map (docs/MAPPING.md).
//!
//! Two games, two spaces. Everything that converts between them lives here so there is exactly one
//! place to be wrong, and `tests/conformance.rs` checks this file against the Python reference's
//! vectors.

use crate::protocol::{
    CELL_CLIFF, CELL_GRASS, CELL_ROCK, CELL_SAND, CELL_SHALLOW, CELL_SNOW, CELL_UNKNOWN, CELL_WATER,
};
use crate::{BridgeError, Result};
use std::f32::consts::FRAC_PI_2;

/// Guest units per tile (RecWars' `TILE_SIZE`, `src/map.rs:9`).
pub const TILE_UNITS: f32 = 64.0;
/// Metres per guest tile.
pub const TILE_M: f32 = 8.0;
/// Guest units to metres. One conversion factor for lengths, positions and speeds.
pub const U2M: f32 = TILE_M / TILE_UNITS;
/// Veloren's default camera field of view, used to match zoom (`camera.rs:332`).
pub const FOV_REF: f32 = 1.1;
/// Size of the guest's terrain region, per side.
pub const MAX_TERRAIN: usize = 128;
/// Size of one terrain cell record.
pub const CELL_SIZE: usize = 8;
/// Size of the terrain payload header.
pub const TERRAIN_HEADER_SIZE: usize = 32;
/// Height difference between neighbouring cells that counts as a cliff (with `cell_m = 8`, 45°).
pub const CLIFF_DELTA_M: f32 = 8.0;
/// Slope that counts as rock.
pub const ROCK_SLOPE: f32 = 0.5;
/// Cell flags: the host considers this cell a valid spawn point.
pub const CELL_FLAG_SPAWNABLE: u8 = 1 << 0;
/// Cell flags: the cell holds liquid.
pub const CELL_FLAG_LIQUID: u8 = 1 << 1;

/// Terrain payload header (32 B): where the heightfield is and what a cell means.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TerrainHeader {
    /// Minimum corner in Veloren metres.
    pub origin_x: f32,
    /// Minimum corner in Veloren metres.
    pub origin_y: f32,
    /// Metres per cell.
    pub cell_m: f32,
    /// Cells along x.
    pub nx: u16,
    /// Cells along y.
    pub ny: u16,
    /// Metres per guest tile; equals `cell_m` in v1.
    pub tile_m: f32,
    /// The host's sea level in metres.
    pub sea_level: f32,
    /// Bumped by the host on every export; the guest rebuilds its map when it changes.
    pub revision: u32,
    /// Bit 0 fresh, bit 1 partial (edge of the host's loaded chunks), bit 2 has water.
    pub flags: u32,
}

impl Default for TerrainHeader {
    fn default() -> Self {
        Self {
            origin_x: 0.0,
            origin_y: 0.0,
            cell_m: TILE_M,
            nx: 64,
            ny: 64,
            tile_m: TILE_M,
            sea_level: 0.0,
            revision: 0,
            flags: 0,
        }
    }
}

impl TerrainHeader {
    /// Host `y` of the top row (guest row 0 is the top, and guest `y` grows downward).
    pub fn origin_y_max(&self) -> f32 {
        self.origin_y + self.ny as f32 * self.cell_m
    }

    /// Width of the covered area in metres.
    pub fn width_m(&self) -> f32 {
        self.nx as f32 * self.cell_m
    }

    /// Depth of the covered area in metres.
    pub fn depth_m(&self) -> f32 {
        self.ny as f32 * self.cell_m
    }

    /// Total cells.
    pub fn cell_count(&self) -> usize {
        self.nx as usize * self.ny as usize
    }

    /// Cell index for a host position, clamped into range: `(i, j)`.
    pub fn cell_at(&self, host_x: f32, host_y: f32) -> (i32, i32) {
        let i = ((host_x - self.origin_x) / self.cell_m).floor() as i32;
        let j = ((host_y - self.origin_y) / self.cell_m).floor() as i32;
        (
            i.clamp(0, self.nx as i32 - 1),
            j.clamp(0, self.ny as i32 - 1),
        )
    }

    /// Cell index for a guest tile `(c, r)` (exact because `cell_m == tile_m` in v1).
    pub fn cell_of_tile(&self, c: u32, r: u32) -> (usize, usize) {
        (c as usize, self.ny as usize - 1 - r as usize)
    }
}

/// One terrain cell (8 B).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Cell {
    /// Ground height in metres.
    pub height_m: f32,
    /// Kind (`CELL_*`).
    pub kind: u8,
    /// Flags (`CELL_FLAG_*`).
    pub flags: u8,
}

impl Cell {
    /// A cell with a height and a kind.
    pub fn new(height_m: f32, kind: u8) -> Self {
        Self {
            height_m,
            kind,
            flags: 0,
        }
    }
}

/// Guest coordinates to host metres (docs/MAPPING.md §3).
pub fn guest_to_host(gx: f32, gy: f32, t: &TerrainHeader) -> (f32, f32) {
    (
        t.origin_x + gx * U2M,
        t.origin_y_max() - gy * U2M,
    )
}

/// Host metres to guest coordinates (docs/MAPPING.md §3).
pub fn host_to_guest(hx: f32, hy: f32, t: &TerrainHeader) -> (f32, f32) {
    ((hx - t.origin_x) / U2M, (t.origin_y_max() - hy) / U2M)
}

/// Clamp guest coordinates into the exported region.
pub fn clamp_to_region(gx: f32, gy: f32, t: &TerrainHeader) -> (f32, f32) {
    let w = t.nx as f32 * TILE_UNITS;
    let h = t.ny as f32 * TILE_UNITS;
    (gx.clamp(0.0, w), gy.clamp(0.0, h))
}

/// Wrap an angle into `(-π, π]`, the range the protocol uses.
pub fn normalize_angle(a: f32) -> f32 {
    let mut a = (a + std::f32::consts::PI) % (2.0 * std::f32::consts::PI);
    if a <= 0.0 {
        a += 2.0 * std::f32::consts::PI;
    }
    a - std::f32::consts::PI
}

/// Guest hull angle to Veloren yaw (docs/MAPPING.md §4): `yaw = -theta - π/2`.
pub fn guest_angle_to_yaw(theta: f32) -> f32 {
    normalize_angle(-theta - FRAC_PI_2)
}

/// Veloren yaw to a guest hull angle: `theta = -yaw - π/2`.
pub fn yaw_to_guest_angle(yaw: f32) -> f32 {
    normalize_angle(-yaw - FRAC_PI_2)
}

/// Veloren's direction vector for a yaw: `(-sin φ, cos φ)`, because `Dir::forward()` is `+y`
/// (`common/src/util/dir.rs:122`) and `yawed_left` is `rotation_z(+θ)` (`common/src/comp/ori.rs:227`).
pub fn yaw_to_dir(yaw: f32) -> (f32, f32) {
    (-yaw.sin(), yaw.cos())
}

/// The yaw that faces a point, in Veloren's convention.
pub fn yaw_towards(dx: f32, dy: f32) -> f32 {
    (-dx).atan2(dy)
}

/// Classify one cell of a grid, exactly as `fakes/bridge.py` does (docs/MAPPING.md §5).
///
/// `cells` is row-major with `j` increasing with host `y`; `sea` is the host's sea level.
pub fn classify_cell(
    cells: &[Cell],
    i: usize,
    j: usize,
    nx: usize,
    ny: usize,
    sea: f32,
    cell_m: f32,
) -> u8 {
    let idx = j * nx + i;
    if idx >= cells.len() {
        return CELL_UNKNOWN;
    }
    let h = cells[idx].height_m;
    if h < sea + 0.5 {
        return CELL_WATER;
    }
    if h < sea + 1.2 {
        return CELL_SHALLOW;
    }
    let mut worst = 0.0f32;
    for (di, dj) in [(1i32, 0i32), (-1, 0), (0, 1), (0, -1)] {
        let ii = i as i32 + di;
        let jj = j as i32 + dj;
        if ii < 0 || jj < 0 || ii >= nx as i32 || jj >= ny as i32 {
            continue;
        }
        let other = cells[jj as usize * nx + ii as usize].height_m;
        worst = worst.max((other - h).abs());
    }
    if worst >= CLIFF_DELTA_M {
        return CELL_CLIFF;
    }
    if h < sea + 2.2 {
        return CELL_SAND;
    }
    if h > sea + 120.0 {
        return CELL_SNOW;
    }
    if worst / cell_m >= ROCK_SLOPE || h > sea + 90.0 {
        return CELL_ROCK;
    }
    CELL_GRASS
}

/// The guest-side name of the surface a cell kind becomes (RecWars `SurfaceKind`, `src/map.rs:305`).
pub fn guest_surface_for(kind: u8) -> &'static str {
    match kind {
        CELL_WATER | CELL_SHALLOW => "water",
        CELL_SAND => "sand",
        CELL_GRASS => "grass",
        CELL_ROCK => "rock",
        CELL_SNOW => "snow",
        // unknown (unloaded chunks) and cliffs are walls to the guest
        _ => "wall",
    }
}

/// Pack a terrain payload: header plus cells.
pub fn pack_terrain(header: &TerrainHeader, cells: &[Cell]) -> Result<Vec<u8>> {
    if header.nx as usize > MAX_TERRAIN || header.ny as usize > MAX_TERRAIN {
        return Err(BridgeError::TooLarge("terrain"));
    }
    if cells.len() != header.cell_count() {
        return Err(BridgeError::Malformed("terrain cell count"));
    }
    let mut buf = vec![0u8; TERRAIN_HEADER_SIZE + cells.len() * CELL_SIZE];
    {
        let mut w = crate::protocol::Cursor::at(&mut buf, 0);
        w.f32(header.origin_x);
        w.f32(header.origin_y);
        w.f32(header.cell_m);
        w.u16(header.nx);
        w.u16(header.ny);
        w.f32(header.tile_m);
        w.f32(header.sea_level);
        w.u32(header.revision);
        w.u32(header.flags);
    }
    let mut offset = TERRAIN_HEADER_SIZE;
    for cell in cells {
        let mut w = crate::protocol::Cursor::at(&mut buf, offset);
        w.f32(cell.height_m);
        w.u8(cell.kind);
        w.u8(cell.flags);
        w.u16(0);
        offset += CELL_SIZE;
    }
    Ok(buf)
}

/// Parse a terrain payload.
pub fn unpack_terrain(payload: &[u8]) -> Result<(TerrainHeader, Vec<Cell>)> {
    if payload.len() < TERRAIN_HEADER_SIZE {
        return Err(BridgeError::Malformed("terrain payload shorter than its header"));
    }
    let header = {
        let mut head = [0u8; TERRAIN_HEADER_SIZE];
        head.copy_from_slice(&payload[..TERRAIN_HEADER_SIZE]);
        let mut r = crate::protocol::Cursor::at(&mut head, 0);
        TerrainHeader {
            origin_x: r.f32(),
            origin_y: r.f32(),
            cell_m: r.f32(),
            nx: r.u16(),
            ny: r.u16(),
            tile_m: r.f32(),
            sea_level: r.f32(),
            revision: r.u32(),
            flags: r.u32(),
        }
    };
    if header.nx as usize > MAX_TERRAIN || header.ny as usize > MAX_TERRAIN {
        return Err(BridgeError::TooLarge("terrain"));
    }
    let need = TERRAIN_HEADER_SIZE + header.cell_count() * CELL_SIZE;
    if payload.len() < need {
        return Err(BridgeError::Malformed("terrain payload truncated"));
    }
    let mut cells = Vec::with_capacity(header.cell_count());
    for n in 0..header.cell_count() {
        let offset = TERRAIN_HEADER_SIZE + n * CELL_SIZE;
        let b = &payload[offset..offset + CELL_SIZE];
        cells.push(Cell {
            height_m: f32::from_le_bytes([b[0], b[1], b[2], b[3]]),
            kind: b[4],
            flags: b[5],
        });
    }
    Ok((header, cells))
}
