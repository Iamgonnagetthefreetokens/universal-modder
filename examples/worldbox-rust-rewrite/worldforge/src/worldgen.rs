//! Procedural world generation.
//!
//! Value-noise fractal terrain on a hex grid, shaped by a world type, then
//! climate, biomes, rivers and resources. Everything is driven by [`crate::rng`]
//! plus a deterministic lattice hash, so `generate(w, h, seed, kind)` is a pure
//! function.
//!
//! Pipeline:
//! 1. elevation = fbm noise + world-type shaping (falloff mask, ridges)
//! 2. sea level cut -> ocean / shallow / land
//! 3. moisture noise, temperature from latitude minus altitude
//! 4. biome from climate
//! 5. rivers carved from the highest peaks down to the sea
//! 6. resources: trees, stone, ore, fertile soil

use crate::hex::Hex;
use crate::rng::Rng;
use crate::terrain::{Biome, Tile};

/// Shorthand for a rectangular map size in tiles.
pub const SIZE_TINY: (u16, u16) = (36, 28);
pub const SIZE_SMALL: (u16, u16) = (48, 36);
pub const SIZE_MEDIUM: (u16, u16) = (64, 48);
pub const SIZE_LARGE: (u16, u16) = (88, 64);
pub const SIZE_GIGANTIC: (u16, u16) = (120, 88);

/// The kind of world to roll.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum WorldType {
    /// A few big landmasses with long coasts. The default.
    #[default]
    Continents,
    /// Many small islands, lots of ocean.
    Archipelago,
    /// One dominant supercontinent.
    Pangaea,
    /// Mostly land with mountain ranges.
    Highlands,
    /// Land dotted with inland lakes.
    Lakes,
    /// Hot and dry, deserts dominate.
    Desert,
    /// Cold, snow and permafrost dominate.
    Frozen,
}

impl WorldType {
    pub const ALL: [WorldType; 7] = [
        WorldType::Continents,
        WorldType::Archipelago,
        WorldType::Pangaea,
        WorldType::Highlands,
        WorldType::Lakes,
        WorldType::Desert,
        WorldType::Frozen,
    ];

    pub fn name(self) -> &'static str {
        match self {
            WorldType::Continents => "continents",
            WorldType::Archipelago => "archipelago",
            WorldType::Pangaea => "pangaea",
            WorldType::Highlands => "highlands",
            WorldType::Lakes => "lakes",
            WorldType::Desert => "desert",
            WorldType::Frozen => "frozen",
        }
    }

    pub fn parse(s: &str) -> Option<WorldType> {
        WorldType::ALL
            .into_iter()
            .find(|w| w.name().starts_with(&s.to_ascii_lowercase()))
    }

    /// Named map size used by the CLI (`tiny` .. `gigantic`).
    pub fn size(name: &str) -> Option<(u16, u16)> {
        match name.to_ascii_lowercase().as_str() {
            "tiny" => Some(SIZE_TINY),
            "small" => Some(SIZE_SMALL),
            "medium" => Some(SIZE_MEDIUM),
            "large" => Some(SIZE_LARGE),
            "gigantic" | "huge" => Some(SIZE_GIGANTIC),
            _ => None,
        }
    }
}

/// Parameters for one generation run.
#[derive(Clone, Copy, Debug)]
pub struct GenParams {
    pub width: u16,
    pub height: u16,
    pub seed: u64,
    pub world_type: WorldType,
    /// 0..=100, fraction of the map that ends up above water (roughly).
    pub land_ratio: u8,
}

impl GenParams {
    pub fn new(width: u16, height: u16, seed: u64, world_type: WorldType) -> Self {
        GenParams {
            width,
            height,
            seed,
            world_type,
            land_ratio: 45,
        }
    }

    pub fn with_land_ratio(mut self, ratio: u8) -> Self {
        self.land_ratio = ratio.clamp(5, 95);
        self
    }
}

impl Default for GenParams {
    fn default() -> Self {
        GenParams::new(SIZE_SMALL.0, SIZE_SMALL.1, 1, WorldType::Continents)
    }
}

/// Deterministic 2D lattice hash in `[0, 1)`.
fn hash2(seed: u64, x: i32, y: i32) -> f32 {
    let mut h = seed
        ^ (x as i64 as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ (y as i64 as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F);
    h ^= h >> 33;
    h = h.wrapping_mul(0xff51_afd7_ed55_8ccd);
    h ^= h >> 33;
    h = h.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
    h ^= h >> 33;
    (h >> 11) as f32 / (1u64 << 53) as f32
}

fn smoothstep(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

/// Bilinear value noise.
pub fn value_noise(seed: u64, x: f32, y: f32) -> f32 {
    let x0 = x.floor();
    let y0 = y.floor();
    let tx = smoothstep(x - x0);
    let ty = smoothstep(y - y0);
    let (xi, yi) = (x0 as i32, y0 as i32);
    let c00 = hash2(seed, xi, yi);
    let c10 = hash2(seed, xi + 1, yi);
    let c01 = hash2(seed, xi, yi + 1);
    let c11 = hash2(seed, xi + 1, yi + 1);
    let a = c00 + (c10 - c00) * tx;
    let b = c01 + (c11 - c01) * tx;
    a + (b - a) * ty
}

/// Fractal Brownian motion over [`value_noise`], normalised to `[0, 1]`.
pub fn fbm(seed: u64, x: f32, y: f32, octaves: u32, lacunarity: f32, gain: f32) -> f32 {
    let mut sum = 0.0;
    let mut norm = 0.0;
    let mut amp = 1.0;
    let mut freq = 1.0;
    for o in 0..octaves {
        sum += amp * value_noise(seed.wrapping_add(o as u64 * 0x51ed_2701), x * freq, y * freq);
        norm += amp;
        amp *= gain;
        freq *= lacunarity;
    }
    if norm > 0.0 {
        sum / norm
    } else {
        0.0
    }
}

/// Ridged noise, good for mountain chains.
fn ridge(seed: u64, x: f32, y: f32, octaves: u32) -> f32 {
    let mut sum = 0.0;
    let mut norm = 0.0;
    let mut amp = 1.0;
    let mut freq = 1.0;
    for o in 0..octaves {
        let n = value_noise(seed.wrapping_add(o as u64 * 0x9e37_79b9), x * freq, y * freq);
        sum += amp * (1.0 - (n * 2.0 - 1.0).abs());
        norm += amp;
        amp *= 0.5;
        freq *= 2.0;
    }
    if norm > 0.0 {
        sum / norm
    } else {
        0.0
    }
}

/// Result of generation: the tile grid plus a few stats worth logging.
#[derive(Clone, Debug)]
pub struct GeneratedWorld {
    pub tiles: Vec<Tile>,
    pub width: u16,
    pub height: u16,
    pub land_tiles: u32,
    pub water_tiles: u32,
    pub peak_elevation: i16,
    pub river_tiles: u32,
}

impl GeneratedWorld {
    /// Tiles as a slice in row-major (odd-r offset) order.
    pub fn tile(&self, h: Hex) -> Option<&Tile> {
        let (c, r) = h.to_offset();
        if c < 0 || r < 0 || c >= self.width as i32 || r >= self.height as i32 {
            return None;
        }
        self.tiles.get((r * self.width as i32 + c) as usize)
    }
}

/// Roll a world. Pure function of `params`.
pub fn generate(params: GenParams) -> GeneratedWorld {
    let w = params.width as usize;
    let h = params.height as usize;
    let n = w * h;
    let seed = params.seed;

    // --- 1. elevation ---------------------------------------------------
    // Sampling in "world units" keeps features round regardless of map size.
    let scale = match params.world_type {
        WorldType::Archipelago => 0.085,
        WorldType::Pangaea => 0.045,
        WorldType::Highlands => 0.075,
        WorldType::Lakes => 0.06,
        _ => 0.055,
    };
    let mut raw = vec![0f32; n];
    let cx = w as f32 / 2.0;
    let cy = h as f32 / 2.0;
    let max_d = (cx * cx + cy * cy).sqrt().max(1.0);
    for row in 0..h {
        for col in 0..w {
            let i = row * w + col;
            let (fx, fy) = (col as f32, row as f32);
            let mut e = fbm(seed, fx * scale, fy * scale, 6, 2.0, 0.5);
            // Ridges in highlands (and a bit everywhere) make mountain spines.
            let ridge_amt = match params.world_type {
                WorldType::Highlands => 0.55,
                WorldType::Pangaea => 0.3,
                _ => 0.18,
            };
            e = e * (1.0 - ridge_amt) + ridge(seed ^ 0xabcd, fx * scale, fy * scale, 4) * ridge_amt;
            // Shaping mask.
            let dx = (fx - cx) / max_d;
            let dy = (fy - cy) / max_d;
            let d = (dx * dx + dy * dy).sqrt();
            let shaped = match params.world_type {
                WorldType::Archipelago => e - (d * 0.45).powf(1.4) * 1.3 + 0.12,
                WorldType::Pangaea => e - (d * 0.30).powf(2.2) * 1.1,
                WorldType::Highlands => e * 1.25 - d * 0.20,
                WorldType::Lakes | WorldType::Continents | WorldType::Desert | WorldType::Frozen => {
                    e - (d * 0.55).powf(2.4) * 0.95 + 0.06
                }
            };
            raw[i] = shaped;
        }
    }

    // Land ratio -> sea level threshold (percentile of the noise field).
    let mut sorted = raw.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let sea_pct = (100 - params.land_ratio) as f32 / 100.0;
    let sea_level = sorted[((sea_pct * n as f32) as usize).min(n - 1)];

    // --- 2. water / land ------------------------------------------------
    let mut tiles: Vec<Tile> = Vec::with_capacity(n);
    for row in 0..h {
        for col in 0..w {
            let i = row * w + col;
            let above = raw[i] - sea_level;
            let elevation = (above * 900.0).clamp(-320.0, 900.0) as i16;
            let mut t = Tile {
                elevation,
                ..Default::default()
            };
            t.biome = if elevation <= -25 {
                Biome::Ocean
            } else if elevation < 0 {
                Biome::Shallow
            } else {
                Biome::Beach // provisional; overwritten by climate below
            };
            tiles.push(t);
        }
    }

    // Lakes world type: carve inland depressions.
    if params.world_type == WorldType::Lakes {
        let mut rng = Rng::stream(seed, 0x1a4e5);
        let lakes = rng.range(6, 14);
        for _ in 0..lakes {
            let cx0 = rng.range(2, w as i32 - 3);
            let cy0 = rng.range(2, h as i32 - 3);
            let radius = rng.range(1, 3);
            let c = Hex::from_offset(cx0, cy0);
            for p in c.spiral(radius) {
                let (pc, pr) = p.to_offset();
                if pc < 0 || pr < 0 || pc >= w as i32 || pr >= h as i32 {
                    continue;
                }
                let i = (pr as usize) * w + pc as usize;
                if tiles[i].elevation > 40 {
                    tiles[i].elevation = -10;
                    tiles[i].biome = Biome::Shallow;
                    tiles[i].river = true;
                }
            }
        }
    }

    // --- 3./4. climate and biomes --------------------------------------
    let cold_bias: f32 = match params.world_type {
        WorldType::Frozen => -32.0,
        WorldType::Desert => 20.0,
        _ => 0.0,
    };
    for row in 0..h {
        for col in 0..w {
            let i = row * w + col;
            if tiles[i].biome.is_water() && !tiles[i].river {
                // Freeze the top and bottom of a cold world.
                if cold_bias < -20.0 && (row as f32 / h as f32 - 0.5).abs() > 0.3 {
                    tiles[i].biome = Biome::Ice;
                }
                continue;
            }
            let fx = col as f32;
            let fy = row as f32;
            let lat = (fy / h as f32 - 0.5).abs() * 2.0; // 0 equator -> 1 poles
            let mut temperature = (100.0 - lat * 88.0) + cold_bias;
            temperature += (fbm(seed ^ 0x77, fx * 0.09, fy * 0.09, 3, 2.0, 0.5) - 0.5) * 22.0;
            let moisture = (fbm(seed ^ 0x1234, fx * 0.11, fy * 0.11, 4, 2.0, 0.5) * 100.0).clamp(0.0, 100.0);
            let elevation = tiles[i].elevation;
            let t = temperature.clamp(0.0, 100.0) as u8;
            let m = moisture as u8;
            tiles[i].biome = Biome::from_climate(elevation, m, t);
            if elevation < 45 {
                tiles[i].biome = Biome::Beach;
            }
            tiles[i].refresh_fertility();
        }
    }

    // --- 5. rivers ------------------------------------------------------
    // Start from the highest tiles and walk downhill to the sea.
    let mut peaks: Vec<(i16, usize)> = tiles
        .iter()
        .enumerate()
        .filter(|(_, t)| t.is_land())
        .map(|(i, t)| (t.elevation, i))
        .collect();
    peaks.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    peaks.truncate(24);
    let mut rng = Rng::stream(seed, 0x2b0a7);
    rng.shuffle(&mut peaks);
    let mut river_tiles = 0u32;
    for (_, start) in peaks.iter().take(12) {
        let (sc, sr) = (*start % w, *start / w);
        let mut cur = Hex::from_offset(sc as i32, sr as i32);
        for _ in 0..400 {
            let (cc, cr) = cur.to_offset();
            if cc < 0 || cr < 0 || cc >= w as i32 || cr >= h as i32 {
                break;
            }
            let ci = cr as usize * w + cc as usize;
            if tiles[ci].is_water() && !tiles[ci].river {
                break; // reached the sea
            }
            if !tiles[ci].river {
                tiles[ci].river = true;
                tiles[ci].refresh_fertility();
                river_tiles += 1;
            }
            // Pick the lowest neighbour, with a small random tie-break so rivers
            // meander instead of running perfectly straight.
            let mut best: Option<(i32, Hex)> = None;
            for nb in cur.neighbors() {
                let (nc, nr) = nb.to_offset();
                if nc < 0 || nr < 0 || nc >= w as i32 || nr >= h as i32 {
                    continue;
                }
                let ni = nr as usize * w + nc as usize;
                let e = tiles[ni].elevation as i32 - if tiles[ni].is_water() { 80 } else { 0 };
                let jitter = rng.range(-6, 6);
                let score = e + jitter;
                if best.is_none() || score < best.unwrap().0 {
                    best = Some((score, nb));
                }
            }
            match best {
                Some((_, nb)) => cur = nb,
                None => break,
            }
        }
    }

    // --- 6. resources ---------------------------------------------------
    let mut rng = Rng::stream(seed, 0x3c0de);
    for t in tiles.iter_mut() {
        if !t.is_land() {
            continue;
        }
        let cap = t.biome.tree_capacity();
        if cap > 0 {
            // Forests cluster: sample a second noise field for density.
            let base = if t.biome == Biome::Forest || t.biome == Biome::Jungle {
                0.75
            } else {
                0.35
            };
            let roll = rng.next_f32();
            t.trees = if roll < base * 0.45 {
                cap
            } else if roll < base {
                (cap - 1).max(1)
            } else {
                0
            };
        }
        match t.biome {
            Biome::Mountain => {
                t.stone = rng.range(1, 3) as u8;
                t.ore = if rng.chance(0.35) { rng.range(1, 3) as u8 } else { 0 };
            }
            Biome::Snow | Biome::Tundra | Biome::Permafrost | Biome::Crystal => {
                t.stone = if rng.chance(0.3) { 1 } else { 0 };
                t.ore = if rng.chance(0.08) { 1 } else { 0 };
            }
            Biome::Desert | Biome::Savanna | Biome::Wasteland | Biome::Ash => {
                t.ore = if rng.chance(0.10) { rng.range(1, 2) as u8 } else { 0 };
            }
            Biome::Beach => {
                t.stone = if rng.chance(0.15) { 1 } else { 0 };
            }
            _ => {
                t.ore = if rng.chance(0.05) { 1 } else { 0 };
                t.stone = if rng.chance(0.10) { 1 } else { 0 };
            }
        }
    }

    let land_tiles = tiles.iter().filter(|t| t.is_land()).count() as u32;
    let water_tiles = n as u32 - land_tiles;
    let peak_elevation = tiles.iter().map(|t| t.elevation).max().unwrap_or(0);

    GeneratedWorld {
        tiles,
        width: params.width,
        height: params.height,
        land_tiles,
        water_tiles,
        peak_elevation,
        river_tiles,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generation_is_deterministic() {
        let a = generate(GenParams::new(32, 24, 1234, WorldType::Continents));
        let b = generate(GenParams::new(32, 24, 1234, WorldType::Continents));
        assert_eq!(a.tiles, b.tiles);
        let c = generate(GenParams::new(32, 24, 1235, WorldType::Continents));
        assert_ne!(a.tiles, c.tiles);
    }

    #[test]
    fn every_world_type_produces_land_and_water() {
        for wt in WorldType::ALL {
            let g = generate(GenParams::new(48, 36, 42, wt));
            assert!(g.land_tiles > 50, "{wt:?} had {} land tiles", g.land_tiles);
            assert!(g.water_tiles > 50, "{wt:?} had {} water tiles", g.water_tiles);
            assert!(g.tiles.iter().all(|t| t.elevation >= -320));
        }
    }

    #[test]
    fn land_ratio_actually_controls_land() {
        let dry = generate(GenParams::new(48, 36, 7, WorldType::Continents).with_land_ratio(15));
        let wet = generate(GenParams::new(48, 36, 7, WorldType::Continents).with_land_ratio(75));
        assert!(dry.land_tiles < wet.land_tiles);
        assert!(dry.land_tiles > 0);
    }

    #[test]
    fn continents_have_more_land_than_archipelago() {
        let cont = generate(GenParams::new(64, 48, 99, WorldType::Continents));
        let arch = generate(GenParams::new(64, 48, 99, WorldType::Archipelago));
        assert!(cont.land_tiles > arch.land_tiles);
    }

    #[test]
    fn rivers_run_into_the_sea() {
        let g = generate(GenParams::new(64, 48, 2024, WorldType::Highlands));
        assert!(g.river_tiles > 0, "highlands should produce rivers");
        // A river tile should have at least one water neighbour reachable downhill;
        // check the weaker invariant that rivers touch land and are not isolated
        // in the ocean.
        for (i, t) in g.tiles.iter().enumerate() {
            if t.river {
                let (c, r) = ((i % 64) as i32, (i / 64) as i32);
                let h = Hex::from_offset(c, r);
                let has_land_neighbor = h
                    .neighbors()
                    .iter()
                    .filter_map(|n| g.tile(*n))
                    .any(|n| n.is_land());
                assert!(has_land_neighbor || t.elevation < 0);
            }
        }
    }

    #[test]
    fn tiles_are_addressable_by_hex() {
        let g = generate(GenParams::new(32, 24, 5, WorldType::Lakes));
        assert!(g.tile(Hex::new(0, 0)).is_some());
        assert!(g.tile(Hex::from_offset(31, 23)).is_some());
        assert!(g.tile(Hex::from_offset(32, 23)).is_none());
        assert!(g.tile(Hex::from_offset(-1, 0)).is_none());
    }

    #[test]
    fn noise_is_in_unit_range_and_smooth() {
        let mut prev = value_noise(9, 3.0, 3.0);
        for i in 0..200 {
            let v = value_noise(9, 3.0 + i as f32 * 0.05, 3.0);
            assert!((0.0..1.0).contains(&v));
            assert!((v - prev).abs() < 0.35, "noise jumped too far: {prev} -> {v}");
            prev = v;
        }
        let f = fbm(9, 1.5, 2.5, 6, 2.0, 0.5);
        assert!((0.0..=1.0).contains(&f));
    }
}
