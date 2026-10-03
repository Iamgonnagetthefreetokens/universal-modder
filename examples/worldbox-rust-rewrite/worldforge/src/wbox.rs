//! Read a WorldBox map.
//!
//! WorldBox saves maps as `.wbox` files (PC: `~/mkarpenko/WorldBox/saves/save#/`,
//! Android: `.../com.mkarpenko.worldbox/files/saves/save#/`). The files are not
//! documented and this crate does not ship, decompile or patch the game, so the
//! reader is built on the one thing the community does publish in abundance:
//! **the files themselves**, shared as maps on the game's Discord, plus the rule
//! that a save preview is a plain image of the very tiles inside the file.
//!
//! So the reader works in two steps:
//!
//! 1. **Colour-matching, always.** A `.wbox` stores one colour per tile, the same
//!    palette the game draws with. Step three bytes at a time through the file,
//!    keep the longest unbroken run of palette colours ([`MIN_RUN`] tiles or more)
//!    and that run is the map — no format knowledge, just the palette. A flat run
//!    carries no row markers, so its *shape* is genuinely undecidable from the
//!    bytes: every factor pair is scored by how well neighbouring tiles agree
//!    ([`candidate_layouts`]), the best wins, and the runners-up are reported in
//!    [`ParsedMap::note`] so a human can say otherwise. [`parse_with_size`] is the
//!    escape hatch — when the size is known, it wins.
//! 2. **Header detection, when present.** Some saves start with `WBOX`, a version, a
//!    width and a height as little-endian `i32`s, with the tile block somewhere
//!    after the header fields (which is why [`find_tiles_for_size`] scans for a
//!    block of exactly `width * height` colours instead of assuming it follows
//!    immediately). If nothing fits the header's size, the colour pass takes over
//!    rather than failing — a lie in the header must not cost you the map.
//!
//! **This header layout was inferred, not observed.** No real `.wbox` existed when
//! this reader was written. The colour pass needs no format knowledge and the tests
//! cover the layouts it can meet, but the header path is the one part a real file
//! can falsify; if it does, the fix is here and the workaround is
//! `parse_with_size`.
//!
//! What the colour pass cannot know is the *simulation*: which tiles are a
//! village, where the units are, who owns what. Those become a suggestion —
//! sites are picked where a village's biome would be founded — and the honest
//! framing is in `README`: this imports the world's *terrain*, not its history.
//!
//! Everything here is testable without a single real `.wbox`: the synthetic writer
//! in the tests produces files in the shape the reader expects, so colour-matching,
//! layout scoring, the header path, the truncated-header fallback, the PNG render
//! and the whole import-into-a-running-simulation path are covered. What those
//! tests **cannot** prove is that a file the game itself wrote looks like them —
//! hence `inspect`, which prints what the reader found so the first real file can be
//! judged instead of trusted.

use crate::bridge;
use crate::hex::Hex;
use crate::terrain::Biome;
use crate::world::World;
use crate::worldgen::{GenParams, WorldType};

/// Magic of the newer container, if it exists — never seen in a real file and
/// only ever a hint (`inspect` reports it, nothing depends on it).
pub const MAGIC: &[u8; 4] = b"WBOX";
/// A tile colour and its biome: the `(r, g, b)` a `.wbox` stores is the biome's
/// own colour from [`crate::render`], so the two agree by construction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Palette {
    pub rgb: (u8, u8, u8),
    pub biome: Biome,
}

/// Every biome colour, brightest to darkest -- the order does not matter for
/// matching, but a stable order keeps the tests readable.
pub fn palette() -> Vec<Palette> {
    Biome::ALL
        .iter()
        .map(|b| Palette {
            rgb: b.color(),
            biome: *b,
        })
        .collect()
}

/// How close a colour has to be to a palette entry to count as that biome.
/// Exact matches are the common case; the slack is for saves that stored
/// slightly-shaded tiles.
pub const COLOR_TOLERANCE: i32 = 8;

/// The biome whose colour is closest to `rgb`, if it is close enough.
pub fn nearest_biome(rgb: (u8, u8, u8), table: &[Palette], tolerance: i32) -> Option<Biome> {
    let distance = |c: (u8, u8, u8)| -> i32 {
        let dr = c.0 as i32 - rgb.0 as i32;
        let dg = c.1 as i32 - rgb.1 as i32;
        let db = c.2 as i32 - rgb.2 as i32;
        dr * dr + dg * dg + db * db
    };
    table
        .iter()
        .map(|entry| (distance(entry.rgb), entry.biome))
        .filter(|(d, _)| *d <= tolerance * tolerance)
        .min_by_key(|(d, _)| *d)
        .map(|(_, biome)| biome)
}

/// What a `.wbox` yielded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParsedMap {
    pub width: u16,
    pub height: u16,
    /// One biome per tile, row-major.
    pub biomes: Vec<Biome>,
    /// `true` when the `WBOX` header gave the size, `false` when the reader had
    /// to find the tile block by colour.
    pub sized_by_header: bool,
    /// How the size was found, for the human to read.
    pub note: String,
}

impl ParsedMap {
    pub fn biome_at(&self, col: i32, row: i32) -> Option<Biome> {
        if col < 0 || row < 0 || col >= self.width as i32 || row >= self.height as i32 {
            return None;
        }
        self.biomes
            .get((row as usize) * self.width as usize + col as usize)
            .copied()
    }

    /// A one-line summary of the mix, most common biome first.
    pub fn summary(&self) -> String {
        use std::collections::HashMap;
        let mut counts: HashMap<Biome, usize> = HashMap::new();
        for biome in &self.biomes {
            *counts.entry(*biome).or_default() += 1;
        }
        let mut sorted: Vec<(Biome, usize)> = counts.into_iter().collect();
        sorted.sort_by(|a, b| b.1.cmp(&a.1));
        let top: Vec<String> = sorted
            .iter()
            .take(4)
            .map(|(biome, count)| format!("{} {}%", biome.name(), count * 100 / self.biomes.len().max(1)))
            .collect();
        format!("{}x{} tiles: {}", self.width, self.height, top.join(", "))
    }
}

/// Read a `.wbox` file's tile block.
pub fn parse(bytes: &[u8]) -> Result<ParsedMap, String> {
    if bytes.len() < 16 {
        return Err(format!("file is {} bytes, far too small to be a map", bytes.len()));
    }

    // 1. the easy path: a WBOX header, if this save has one
    if bytes.starts_with(MAGIC) {
        let base = MAGIC.len();
        let read_i32 = |at: usize| -> i32 {
            if at + 4 > bytes.len() {
                return 0;
            }
            i32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
        };
        let width = read_i32(base);
        let height = read_i32(base + 4);
        if width > 0 && height > 0 && (width as i64) * (height as i64) < 4_000_000 {
            if let Some((offset, biomes)) =
                find_tiles_for_size(bytes, width as usize, height as usize)
            {
                return Ok(ParsedMap {
                    width: width as u16,
                    height: height as u16,
                    biomes,
                    sized_by_header: true,
                    note: format!("WBOX header: {width}x{height}, tiles from offset {offset}"),
                });
            }
            // The header gave a size but no tile block fits it; fall through to
            // the colour search rather than failing.
        }
    }

    // 2. the general path: find the tile run by colour, fold it, score the folds
    let found = find_tile_run(bytes)?;
    let Some(best) = found.layouts.first().copied() else {
        return Err(format!(
            "found a run of {} tiles at offset {} but no sensible width for them",
            found.biomes.len(),
            found.offset
        ));
    };
    let note = if found.layouts.len() > 1 {
        let alternatives: Vec<String> = found
            .layouts
            .iter()
            .take(4)
            .map(|l| format!("{}x{} ({:.0}%)", l.width, l.height, l.score * 100.0))
            .collect();
        format!(
            "{}x{} from {} tiles at offset {}, scored against {} other shapes: {}",
            best.width,
            best.height,
            found.biomes.len(),
            found.offset,
            found.layouts.len() - 1,
            alternatives.join(", ")
        )
    } else {
        format!(
            "{}x{} from {} tiles at offset {}",
            best.width,
            best.height,
            found.biomes.len(),
            found.offset
        )
    };
    Ok(ParsedMap {
        width: best.width as u16,
        height: best.height as u16,
        biomes: found.biomes,
        sized_by_header: false,
        note,
    })
}

/// Read a file with a size the caller already knows (WorldBox shows the map size
/// in game, so this is the most reliable path of all). The tiles are still found
/// by colour; `width` and `height` only decide how they are folded.
pub fn parse_with_size(bytes: &[u8], width: u16, height: u16) -> Result<ParsedMap, String> {
    let wanted = width as usize * height as usize;
    let found = find_tile_run(bytes)?;
    if found.biomes.len() < wanted {
        return Err(format!(
            "asked for {width}x{height} ({wanted} tiles) but the file holds {} in a row",
            found.biomes.len()
        ));
    }
    Ok(ParsedMap {
        width,
        height,
        biomes: found.biomes[..wanted].to_vec(),
        sized_by_header: false,
        note: format!("{width}x{height} as given, tiles from offset {}", found.offset),
    })
}

/// How many tiles in a row before a run counts as part of the map. Small maps
/// exist, but a run shorter than this is far more likely to be a coincidence of
/// three bytes that happen to match a palette colour.
const MIN_RUN: usize = 24;

/// How a flat run of tile colours might be folded into a map. A run of `n` tiles
/// can be `w * h` for any factor pair, and the file does not say which -- so the
/// reader scores every pair by how coherent the terrain looks at that width and
/// offers the best few, rather than guessing one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    pub width: usize,
    pub height: usize,
    /// 0..1: how often neighbouring tiles are the same class of ground.
    pub score: f32,
}

/// Fold `biomes` (a flat run of `width * height` tiles) at `width` and score it.
pub fn score_layout(biomes: &[Biome], width: usize) -> f32 {
    if width == 0 || biomes.len() < width * 3 {
        return 0.0;
    }
    let height = biomes.len() / width;
    let same_class = |a: Biome, b: Biome| -> bool {
        a == b || (a.is_water() == b.is_water() && !a.is_water())
    };
    let mut agree = 0usize;
    let mut pairs = 0usize;
    for row in 0..height {
        for col in 0..width {
            let here = biomes[row * width + col];
            if col + 1 < width {
                pairs += 1;
                if same_class(here, biomes[row * width + col + 1]) {
                    agree += 1;
                }
            }
            if row + 1 < height {
                pairs += 1;
                if same_class(here, biomes[(row + 1) * width + col]) {
                    agree += 1;
                }
            }
        }
    }
    if pairs == 0 {
        return 0.0;
    }
    agree as f32 / pairs as f32
}

/// Every plausible `(width, height)` for a run of this length, best first.
pub fn candidate_layouts(biomes: &[Biome]) -> Vec<Layout> {
    let mut out = Vec::new();
    let length = biomes.len();
    for width in MIN_RUN..=length / 3 {
        if length % width != 0 {
            continue;
        }
        let height = length / width;
        if height < 3 {
            continue;
        }
        // WorldBox maps are at most about 4:1 either way; ignore the silly shapes
        if width > height * 4 || height > width * 4 {
            continue;
        }
        out.push(Layout {
            width,
            height,
            score: score_layout(biomes, width),
        });
    }
    out.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| (a.width as i64 - a.height as i64).abs().cmp(&(b.width as i64 - b.height as i64).abs()))
    });
    out
}

/// A block of tiles found by colour, with where it was.
#[derive(Clone, Debug, PartialEq)]
pub struct FoundBlock {
    pub offset: usize,
    /// The flat run of tiles, not yet folded into rows.
    pub biomes: Vec<Biome>,
    /// Best foldings, best first (empty when the caller supplied a size).
    pub layouts: Vec<Layout>,
}

/// Walk the file looking for the longest run of palette colours.
pub fn find_tile_run(bytes: &[u8]) -> Result<FoundBlock, String> {
    let table = palette();
    let mut runs: Vec<(usize, Vec<Biome>)> = Vec::new();
    let mut at = 0usize;
    while at + 3 <= bytes.len() {
        // a run of 3-byte tile colours starting here
        let mut run = Vec::new();
        let mut cursor = at;
        while cursor + 3 <= bytes.len() {
            let rgb = (bytes[cursor], bytes[cursor + 1], bytes[cursor + 2]);
            match nearest_biome(rgb, &table, COLOR_TOLERANCE) {
                Some(biome) => {
                    run.push(biome);
                    cursor += 3;
                }
                None => break,
            }
        }
        if run.len() >= MIN_RUN {
            at = cursor;
            runs.push((at - run.len() * 3, run));
        } else {
            at += 1;
        }
    }
    let (offset, biomes) = runs
        .into_iter()
        .max_by_key(|(_, run)| run.len())
        .ok_or_else(|| {
            "no run of WorldBox tile colours in this file: is it really a .wbox save?".to_string()
        })?;
    let layouts = candidate_layouts(&biomes);
    Ok(FoundBlock {
        offset,
        biomes,
        layouts,
    })
}

/// Look for a tile block of exactly `width * height` tiles near the start of the
/// file. A real save has other fields between the header and the tiles, so the
/// block is searched for rather than assumed to follow immediately; the first
/// eight tiles are checked before the full read, which keeps the scan cheap.
pub fn find_tiles_for_size(bytes: &[u8], width: usize, height: usize) -> Option<(usize, Vec<Biome>)> {
    let wanted = width * height;
    if wanted == 0 || wanted * 3 > bytes.len() {
        return None;
    }
    let table = palette();
    let last_start = (bytes.len() - wanted * 3).min(MAGIC.len() + 8 + 65_536);
    let mut at = MAGIC.len() + 8;
    while at <= last_start {
        let mut plausible = true;
        for probe in 0..8.min(wanted) {
            let i = at + probe * 3;
            let rgb = (bytes[i], bytes[i + 1], bytes[i + 2]);
            if nearest_biome(rgb, &table, COLOR_TOLERANCE).is_none() {
                plausible = false;
                break;
            }
        }
        if plausible {
            if let Some(biomes) = read_tiles(bytes, at, width, height) {
                return Some((at, biomes));
            }
        }
        at += 1;
    }
    None
}

/// Read `width * height` tile colours and turn them into biomes.
fn read_tiles(bytes: &[u8], offset: usize, width: usize, height: usize) -> Option<Vec<Biome>> {
    let table = palette();
    let count = width * height;
    if offset + count * 3 > bytes.len() {
        return None;
    }
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let at = offset + i * 3;
        let rgb = (bytes[at], bytes[at + 1], bytes[at + 2]);
        out.push(nearest_biome(rgb, &table, COLOR_TOLERANCE)?);
    }
    Some(out)
}

/// Whether a `.wbox` has the readable header.
pub fn has_header(bytes: &[u8]) -> bool {
    bytes.starts_with(MAGIC)
}

// ---------------------------------------------------------------------------
// Terrain -> a world the simulation can run
// ---------------------------------------------------------------------------

/// How an imported map is turned into a running world.
#[derive(Clone, Copy, Debug)]
pub struct ImportOptions {
    /// Civs to settle: `0` imports the terrain only, exactly as drawn.
    pub civs: u32,
    pub animals: u32,
    pub monsters: u32,
    /// Import elevations inferred from biome type (mountains high, water low) as
    /// well as the biome itself. Off, every land tile is flat.
    pub terrain_height: bool,
}

impl Default for ImportOptions {
    fn default() -> Self {
        ImportOptions {
            civs: 4,
            animals: 30,
            monsters: 0,
            terrain_height: true,
        }
    }
}

/// The elevation a biome implies, so an imported map has hills instead of a
/// pancake. Rough on purpose: the save stores biome, not height.
pub fn implied_elevation(biome: Biome) -> i16 {
    match biome {
        Biome::Ocean => -140,
        Biome::Shallow => -40,
        Biome::Ice => -30,
        Biome::Beach => 6,
        Biome::Grass => 40,
        Biome::Savanna => 60,
        Biome::Desert => 50,
        Biome::Jungle => 70,
        Biome::Forest => 90,
        Biome::Taiga => 80,
        Biome::Swamp => 12,
        Biome::Mushroom => 100,
        Biome::Tundra => 70,
        Biome::Snow => 90,
        Biome::Permafrost => 60,
        Biome::Mountain => 520,
        Biome::Corrupted => 40,
        Biome::Infernal => 120,
        Biome::Crystal => 160,
        Biome::Enchanted => 80,
        Biome::Candy => 30,
        Biome::Wasteland => 40,
        Biome::Ash => 60,
        Biome::Lava => 220,
    }
}

/// Whether a race would settle on this biome at all.
fn settleable(biome: Biome) -> bool {
    !biome.is_water() && biome.is_land() && biome != Biome::Mountain && biome != Biome::Lava
}

/// Rebuild a parsed map as a [`World`]: biomes, elevations, and (unless `civs`
/// is 0) a few settlements where the layout allows them.
///
/// The imported world is *frozen into* the sim's tile layer; the sim then runs on
/// it like any other world. What is not imported is everything the colour pass
/// cannot see: villages, units, kingdoms and the map's history. Those start fresh.
pub fn to_world(map: &ParsedMap, options: ImportOptions) -> World {
    let params = GenParams::new(map.width, map.height, imported_seed(map), WorldType::Continents);
    let mut world = World::generate(params);

    for row in 0..map.height as i32 {
        for col in 0..map.width as i32 {
            let Some(biome) = map.biome_at(col, row) else {
                continue;
            };
            let hex = Hex::from_offset(col, row);
            world.set_biome(hex, biome);
            if let Some(tile) = world.tile_mut(hex) {
                if options.terrain_height {
                    tile.elevation = implied_elevation(biome);
                }
                tile.owner = None;
                tile.road = 0;
                tile.fire = 0;
                tile.lava = if biome == Biome::Lava { 1 } else { 0 };
                tile.river = false;
                tile.trees = match biome {
                    Biome::Forest | Biome::Jungle | Biome::Taiga => 2,
                    Biome::Grass | Biome::Savanna | Biome::Swamp | Biome::Mushroom => 1,
                    _ => 0,
                };
                tile.stone = if biome == Biome::Mountain { 3 } else { 0 };
                tile.ore = if biome == Biome::Mountain { 2 } else { 0 };
                tile.scorch = 0;
            }
        }
    }

    if options.civs > 0 {
        settle(&mut world, map, options);
    }
    world
}

/// Imported worlds are seeded off the map itself: two saves of the same size
/// settle the same way, and the sky is the limit for the rest.
fn imported_seed(map: &ParsedMap) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for byte in &map.biomes {
        h ^= (*byte as u64) & 0xff;
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    h ^ ((map.width as u64) << 32) ^ map.height as u64
}

/// Place starting villages on the map's biggest landmasses, using the same
/// site rules the generator does (walkable, spaced out, room to grow).
fn settle(world: &mut World, map: &ParsedMap, options: ImportOptions) {
    let mut sites: Vec<Hex> = Vec::new();
    let mut rng = crate::rng::Rng::new(0x517E_0001 ^ ((map.width as u64) << 16) ^ map.height as u64);

    // Try hard: a hand-drawn map can have very few legal corners.
    for _ in 0..(map.width as u32 * map.height as u32 * 8).min(200_000) {
        let col = rng.range(0, map.width as i32 - 1);
        let row = rng.range(0, map.height as i32 - 1);
        let Some(biome) = map.biome_at(col, row) else {
            continue;
        };
        if !settleable(biome) {
            continue;
        }
        let hex = Hex::from_offset(col, row);
        // a village needs a patch of land around it, not a one-tile rock
        let open = hex
            .neighbors()
            .iter()
            .filter(|n| {
                let (c, r) = n.to_offset();
                map.biome_at(c, r).map(settleable).unwrap_or(false)
            })
            .count();
        if open < 4 {
            continue;
        }
        if sites.iter().any(|s| s.distance(hex) < 12) {
            continue;
        }
        sites.push(hex);
        if sites.len() as u32 >= options.civs {
            break;
        }
    }

    world.seed_life_at(&sites, options.animals, options.monsters);
}

/// Draw a parsed map as a PNG, so the human can see what the reader found
/// before committing a world to it.
pub fn render(map: &ParsedMap, scale: u32) -> crate::png::RgbImage {
    let scale = scale.clamp(1, 32);
    let mut img = crate::png::RgbImage::new(
        map.width as u32 * scale,
        map.height as u32 * scale,
    );
    for row in 0..map.height as u32 {
        for col in 0..map.width as u32 {
            let rgb = map
                .biomes
                .get((row * map.width as u32 + col) as usize)
                .map(|b| b.color())
                .unwrap_or((0, 0, 0));
            img.rect(col * scale, row * scale, scale, scale, rgb);
        }
    }
    img
}

/// Everything the reader can say about a file without deciding anything, for a
/// human who has a real save and wants to know what is in it.
pub fn inspect(bytes: &[u8]) -> String {
    let mut out = String::new();
    out.push_str(&format!("{} bytes\n", bytes.len()));
    out.push_str(&format!(
        "starts with `WBOX`: {}\n",
        has_header(bytes)
    ));
    if has_header(bytes) && bytes.len() >= 12 {
        let read = |at: usize| i32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]);
        out.push_str(&format!(
            "  header says {} x {} (tiles from offset 12)\n",
            read(4),
            read(8)
        ));
    }
    match find_tile_run(bytes) {
        Ok(found) => {
            out.push_str(&format!(
                "longest run of tiles: {} at offset {} ({} bytes, {}% of the file)\n",
                found.biomes.len(),
                found.offset,
                found.biomes.len() * 3,
                found.biomes.len() * 3 * 100 / bytes.len().max(1)
            ));
            out.push_str("shapes it could be, best first:\n");
            for layout in found.layouts.iter().take(6) {
                out.push_str(&format!(
                    "  {:>4} x {:<4} score {:.0}%\n",
                    layout.width,
                    layout.height,
                    layout.score * 100.0
                ));
            }
            if found.layouts.is_empty() {
                out.push_str("  (none: the run is too short, or has no factor pair that looks like a map)\n");
            }
            let mut counts: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
            for biome in &found.biomes {
                *counts.entry(biome.name()).or_default() += 1;
            }
            let mut sorted: Vec<(&str, usize)> = counts.into_iter().collect();
            sorted.sort_by(|a, b| b.1.cmp(&a.1));
            out.push_str("what it is made of: ");
            let list: Vec<String> = sorted
                .iter()
                .map(|(name, count)| format!("{name} {count}"))
                .collect();
            out.push_str(&list.join(", "));
            out.push('\n');
        }
        Err(error) => out.push_str(&format!("no tile run found: {error}\n")),
    }
    out.push_str("\nsizes to try if the shape above looks wrong: `worldforge wbox FILE --width W --height H`\n");
    out
}

/// The palette as a `#rrggbb` table, for the CLI's `--palette` output.
pub fn palette_table() -> Vec<(Biome, String)> {
    palette()
        .into_iter()
        .map(|entry| (entry.biome, bridge::hex_color(entry.rgb)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stand-in map: sensible terrain (an ocean, a forest, a mountain range),
    /// which is what makes layout scoring meaningful.
    fn coherent(width: usize, height: usize) -> Vec<Biome> {
        let mut out = Vec::new();
        for row in 0..height {
            for col in 0..width {
                let biome = if col < width / 3 {
                    Biome::Ocean
                } else if col < 2 * width / 3 {
                    if row < height / 2 {
                        Biome::Grass
                    } else {
                        Biome::Forest
                    }
                } else if row % 3 == 0 {
                    Biome::Mountain
                } else {
                    Biome::Snow
                };
                out.push(biome);
            }
        }
        out
    }

    /// A stand-in `.wbox` file: optional header, then one colour per tile, with
    /// junk on either side to prove the reader has to look for the tile block.
    fn synthetic(width: usize, height: usize, header: bool, noise: usize) -> Vec<u8> {
        let mut bytes = Vec::new();
        if header {
            bytes.extend_from_slice(MAGIC);
            bytes.extend_from_slice(&(width as i32).to_le_bytes());
            bytes.extend_from_slice(&(height as i32).to_le_bytes());
        }
        for i in 0..noise {
            // bytes that are deliberately not palette colours
            bytes.push(0x11 ^ (i as u8 & 3));
        }
        for biome in coherent(width, height) {
            let (r, g, b) = biome.color();
            bytes.extend_from_slice(&[r, g, b]);
        }
        for i in 0..noise {
            bytes.push(0x7f ^ (i as u8 & 7));
        }
        bytes
    }

    #[test]
    fn every_biome_colour_is_distinct_enough_to_match() {
        // A palette where two biomes share a colour would make the reader guess.
        let table = palette();
        for (i, a) in table.iter().enumerate() {
            for b in table.iter().skip(i + 1) {
                let d = (a.rgb.0 as i32 - b.rgb.0 as i32).pow(2)
                    + (a.rgb.1 as i32 - b.rgb.1 as i32).pow(2)
                    + (a.rgb.2 as i32 - b.rgb.2 as i32).pow(2);
                assert!(
                    d > COLOR_TOLERANCE * COLOR_TOLERANCE,
                    "{} and {} are too similar to tell apart: {d}",
                    a.biome.name(),
                    b.biome.name()
                );
            }
        }
    }

    #[test]
    fn colour_matching_round_trips_every_biome() {
        let table = palette();
        for entry in &table {
            assert_eq!(
                nearest_biome(entry.rgb, &table, COLOR_TOLERANCE),
                Some(entry.biome),
                "{} did not match its own colour",
                entry.biome.name()
            );
        }
        assert_eq!(nearest_biome((1, 2, 3), &table, COLOR_TOLERANCE), None);
    }

    #[test]
    fn a_save_with_a_header_reads_its_dimensions() {
        let bytes = synthetic(48, 24, true, 5);
        let map = parse(&bytes).unwrap();
        assert_eq!((map.width, map.height), (48, 24));
        assert!(map.sized_by_header);
        assert_eq!(map.biomes.len(), 48 * 24);
        assert_eq!(map.biome_at(0, 0), Some(Biome::Ocean));
        assert_eq!(map.biome_at(20, 0), Some(Biome::Grass));
        assert_eq!(map.biome_at(20, 20), Some(Biome::Forest));
        assert_eq!(map.biome_at(40, 0), Some(Biome::Mountain));
        assert!(map.summary().contains("48x24"));
        assert!(parse(&bytes).unwrap().note.contains("header"));
    }

    #[test]
    fn a_save_without_a_header_is_found_by_colour() {
        let bytes = synthetic(64, 40, false, 3);
        assert!(!has_header(&bytes));
        let map = parse(&bytes).unwrap();
        assert!(!map.sized_by_header);
        assert_eq!((map.width, map.height), (64, 40), "the coherent shape wins: {}", map.note);
        assert_eq!(map.biome_at(2, 0), Some(Biome::Ocean));
    }

    #[test]
    fn a_given_size_beats_every_heuristic() {
        let bytes = synthetic(64, 40, false, 0);
        let map = parse_with_size(&bytes, 80, 32).unwrap();
        assert_eq!((map.width, map.height), (80, 32));
        assert_eq!(map.biomes.len(), 80 * 32);
        assert!(map.note.contains("as given"));
    }

    #[test]
    fn junk_is_rejected_with_a_useful_message() {
        for bytes in [vec![], vec![0u8; 8]] {
            let error = parse(&bytes).unwrap_err();
            assert!(!error.is_empty());
        }
        let error = parse(&vec![0x42u8; 400]).unwrap_err();
        assert!(
            error.contains("tiles") || error.contains("save"),
            "unhelpful message: {error}"
        );
    }

    #[test]
    fn a_truncated_tile_block_falls_back_to_the_colour_search() {
        // header says 64x64, but the file only holds 40x20 of tiles
        let mut bytes = Vec::new();
        bytes.extend_from_slice(MAGIC);
        bytes.extend_from_slice(&64i32.to_le_bytes());
        bytes.extend_from_slice(&64i32.to_le_bytes());
        bytes.extend_from_slice(&synthetic(40, 20, false, 0));
        let map = parse(&bytes).unwrap();
        assert_eq!((map.width, map.height), (40, 20), "the header lied; colour won");
        assert!(!map.sized_by_header);
    }

    #[test]
    fn layout_scoring_picks_coherent_shapes() {
        for (width, height) in [(48, 24), (64, 40), (100, 50)] {
            let biomes = coherent(width, height);
            let layouts = candidate_layouts(&biomes);
            let best = layouts.first().expect("a layout");
            assert_eq!(
                (best.width, best.height),
                (width, height),
                "the true shape did not win: {layouts:?}"
            );
            assert!(best.score > 0.9, "coherent terrain should score high: {}", best.score);
        }
    }

    #[test]
    fn inspect_tells_a_human_what_is_inside() {
        let bytes = synthetic(64, 40, true, 4);
        let report = inspect(&bytes);
        assert!(report.contains("64 x 40"), "{report}");
        assert!(report.contains("tiles"), "{report}");
        assert!(report.contains("ocean"), "{report}");
        assert!(report.contains("--width"), "{report}");
    }

    #[test]
    fn an_imported_map_keeps_its_shape() {
        let bytes = synthetic(60, 30, true, 0);
        let map = parse(&bytes).unwrap();
        let world = to_world(
            &map,
            ImportOptions {
                civs: 0,
                animals: 0,
                monsters: 0,
                terrain_height: true,
            },
        );
        assert_eq!((world.width, world.height), (60, 30));
        for row in 0..30i32 {
            for col in 0..60i32 {
                let want = map.biome_at(col, row).unwrap();
                let got = world.tile(Hex::from_offset(col, row)).unwrap().biome;
                assert_eq!(got, want, "tile {col},{row} changed biome");
            }
        }
        // the ocean is below sea level, the mountains are high
        assert!(world.tile(Hex::from_offset(0, 0)).unwrap().elevation < 0);
        assert!(world.tile(Hex::from_offset(55, 0)).unwrap().elevation > 400);
        assert!(world.units.is_empty(), "no civs, no wildlife");
    }

    #[test]
    fn an_imported_map_can_be_settled_and_runs() {
        let bytes = synthetic(64, 40, true, 0);
        let map = parse(&bytes).unwrap();
        let mut world = to_world(
            &map,
            ImportOptions {
                civs: 3,
                animals: 10,
                monsters: 0,
                terrain_height: true,
            },
        );
        let founded = world.villages.iter().filter(|v| v.alive).count();
        assert!(founded >= 1, "no village could be placed on a 64x40 map");
        world.step_n(200);
        assert!(
            world.population() > 0,
            "the imported villages died immediately"
        );
    }

    #[test]
    fn a_map_can_be_rendered_to_a_picture() {
        let bytes = synthetic(24, 16, true, 0);
        let map = parse(&bytes).unwrap();
        let img = render(&map, 4);
        assert_eq!((img.width, img.height), (96, 64));
        let bytes = crate::png::encode_png(&img);
        assert_eq!(crate::png::probe_png(&bytes), Some((96, 64)));
        assert!(palette_table().len() >= Biome::ALL.len());
    }
}
