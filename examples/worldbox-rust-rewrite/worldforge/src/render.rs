//! Rendering: an ASCII/ANSI terminal map and an RGB image of the world.
//!
//! Both renderers are pure functions of the `World`, so the same world always
//! produces the same frame — a rendered frame can be used as a test oracle.

use crate::png::RgbImage;
use crate::terrain::{Biome, Tile};
use crate::units::UnitKind;
use crate::world::World;

/// What a cell looks like on screen.
#[derive(Clone, Copy, Debug)]
pub struct Cell {
    pub ch: char,
    pub color: (u8, u8, u8),
    pub bold: bool,
}

impl Cell {
    fn new(ch: char, color: (u8, u8, u8)) -> Cell {
        Cell {
            ch,
            color,
            bold: false,
        }
    }
}

/// Per-frame switches.
#[derive(Clone, Copy, Debug)]
pub struct RenderOpts {
    /// Emit 256-colour ANSI escape codes.
    pub color: bool,
    /// Tint tiles with their owner's banner colour.
    pub borders: bool,
    /// Draw units on top of the terrain.
    pub units: bool,
    /// Draw fires, lava, clouds, meteors and other effects.
    pub effects: bool,
}

impl Default for RenderOpts {
    fn default() -> Self {
        RenderOpts {
            color: true,
            borders: true,
            units: true,
            effects: true,
        }
    }
}

impl RenderOpts {
    /// Plain ASCII, no escape codes, terrain only.
    pub fn plain() -> RenderOpts {
        RenderOpts {
            color: false,
            borders: false,
            units: false,
            effects: false,
        }
    }

    /// Everything except colour.
    pub fn mono() -> RenderOpts {
        RenderOpts {
            color: false,
            ..RenderOpts::default()
        }
    }
}

/// Base colour for a biome. Water gets darker with depth, land lighter with height.
pub fn biome_color(b: Biome) -> (u8, u8, u8) {
    match b {
        Biome::Ocean => (18, 42, 92),
        Biome::Shallow => (34, 84, 140),
        Biome::Ice => (208, 232, 245),
        Biome::Beach => (219, 205, 150),
        Biome::Grass => (86, 152, 62),
        Biome::Savanna => (172, 168, 84),
        Biome::Desert => (216, 196, 132),
        Biome::Jungle => (48, 116, 48),
        Biome::Forest => (44, 104, 52),
        Biome::Taiga => (60, 106, 84),
        Biome::Swamp => (78, 104, 56),
        Biome::Mushroom => (128, 86, 140),
        Biome::Tundra => (150, 158, 150),
        Biome::Snow => (236, 240, 246),
        Biome::Permafrost => (188, 210, 220),
        Biome::Mountain => (124, 118, 110),
        Biome::Corrupted => (86, 44, 96),
        Biome::Infernal => (96, 32, 32),
        Biome::Crystal => (150, 196, 216),
        Biome::Enchanted => (114, 96, 176),
        Biome::Candy => (222, 148, 178),
        Biome::Wasteland => (132, 120, 96),
        Biome::Ash => (72, 68, 66),
        Biome::Lava => (226, 92, 24),
    }
}

/// The character used for a tile before overlays.
pub fn biome_char(b: Biome) -> char {
    match b {
        Biome::Ocean | Biome::Shallow | Biome::Lava => '~',
        Biome::Ice | Biome::Permafrost | Biome::Snow => '*',
        Biome::Beach | Biome::Desert | Biome::Savanna => ',',
        Biome::Grass => '.',
        Biome::Jungle | Biome::Forest | Biome::Taiga | Biome::Mushroom => '"',
        Biome::Swamp => '%',
        Biome::Tundra => ':',
        Biome::Mountain => '^',
        Biome::Crystal => '+',
        Biome::Enchanted => '&',
        Biome::Candy => 'o',
        Biome::Corrupted | Biome::Infernal | Biome::Ash | Biome::Wasteland => '#',
    }
}

/// Mix `color` toward black (factor < 1) or white (factor > 1).
pub fn shade(color: (u8, u8, u8), factor: f32) -> (u8, u8, u8) {
    let f = |c: u8| -> u8 {
        let v = if factor >= 1.0 {
            let t = (factor - 1.0).min(1.0);
            c as f32 + (255.0 - c as f32) * t
        } else {
            c as f32 * factor.max(0.0)
        };
        v.clamp(0.0, 255.0) as u8
    };
    (f(color.0), f(color.1), f(color.2))
}

/// Blend a tile colour toward an owner banner colour.
pub fn blend(base: (u8, u8, u8), tint: (u8, u8, u8), amount: f32) -> (u8, u8, u8) {
    let a = amount.clamp(0.0, 1.0);
    let mix = |x: u8, y: u8| -> u8 { (x as f32 * (1.0 - a) + y as f32 * a) as u8 };
    (mix(base.0, tint.0), mix(base.1, tint.1), mix(base.2, tint.2))
}

/// A tile's colour, including an owner tint when borders are on.
pub fn tile_color(w: &World, t: &Tile, opts: &RenderOpts) -> (u8, u8, u8) {
    let mut color = biome_color(t.biome);
    if t.is_water() {
        // Deeper water is darker.
        let depth = (-t.elevation as f32 / 320.0).clamp(0.0, 1.0);
        color = shade(color, 1.0 - depth * 0.6);
    } else {
        let height = (t.elevation as f32 / 900.0).clamp(0.0, 1.0);
        color = shade(color, 1.0 + height * 0.25);
        if t.trees >= 2 {
            color = shade(color, 0.82);
        }
    }
    if opts.borders {
        if let Some(vid) = t.owner {
            if let Some(v) = w.village(vid) {
                if let Some(kid) = v.kingdom {
                    let c = w.kingdom_color(kid);
                    color = blend(color, c, 0.35);
                }
            }
        }
    }
    if opts.effects {
        if t.lava > 0 {
            color = shade((226, 92, 24), 1.0 + t.lava as f32 * 0.2);
        } else if t.fire > 0 {
            color = shade((247, 170, 40), 1.0 + t.fire as f32 * 0.1);
        } else if t.scorch > 0 {
            color = shade(color, 1.0 - (t.scorch as f32 / 9.0) * 0.55);
        }
    }
    color
}

/// The overlay character for a tile: a unit, a town, fire, or nothing.
fn overlay(w: &World, h: crate::hex::Hex, t: &Tile, opts: &RenderOpts) -> Option<Cell> {
    if opts.units {
        let mut best: Option<Cell> = None;
        let mut best_rank = -1i32;
        for u in &w.units {
            if !u.alive || u.pos != h {
                continue;
            }
            let (ch, rank) = match u.kind {
                UnitKind::King => ('K', 6),
                UnitKind::Leader => ('L', 5),
                UnitKind::Monster => ('M', 4),
                UnitKind::Soldier => ('S', 3),
                UnitKind::Animal => ('a', 1),
                UnitKind::Civilian => ('p', 2),
            };
            if rank > best_rank {
                let color = u
                    .kingdom
                    .and_then(|k| w.kingdom(k))
                    .map(|k| k.color)
                    .unwrap_or((236, 236, 236));
                best_rank = rank;
                best = Some(Cell {
                    ch,
                    color,
                    bold: matches!(u.kind, UnitKind::King | UnitKind::Leader),
                });
            }
        }
        if best.is_some() {
            return best;
        }
    }
    if opts.effects && t.fire > 0 {
        return Some(Cell::new('!', (247, 140, 40)));
    }
    if opts.effects && t.lava > 0 {
        return Some(Cell::new('~', (240, 96, 32)));
    }
    // A village centre: the first letter of its name.
    if let Some(vid) = t.owner {
        if let Some(v) = w.village(vid) {
            if v.center == h {
                let ch = v.name.chars().next().unwrap_or('O').to_ascii_uppercase();
                let color = v
                    .kingdom
                    .and_then(|k| w.kingdom(k))
                    .map(|k| k.color)
                    .unwrap_or((255, 255, 255));
                return Some(Cell {
                    ch,
                    color,
                    bold: true,
                });
            }
        }
    }
    if opts.effects && t.fire > 0 {
        return Some(Cell::new('!', (247, 140, 40)));
    }
    None
}

/// The character and colour for one map cell.
pub fn cell(w: &World, h: crate::hex::Hex, opts: &RenderOpts) -> Cell {
    let Some(t) = w.tile(h) else {
        return Cell::new(' ', (0, 0, 0));
    };
    if let Some(c) = overlay(w, h, t, opts) {
        return c;
    }
    let mut color = tile_color(w, t, opts);
    let ch = if t.river && t.is_land() {
        '~'
    } else if t.road > 0 && t.is_land() {
        '='
    } else {
        biome_char(t.biome)
    };
    if opts.effects && t.scorch > 0 {
        color = shade(color, 0.6);
    }
    Cell {
        ch,
        color,
        bold: t.biome == Biome::Lava || t.biome == Biome::Infernal,
    }
}

/// Nearest xterm-256 colour index for an RGB triple.
pub fn ansi_index(color: (u8, u8, u8)) -> u8 {
    let q = |c: u8| -> u8 {
        // 0..6 cube steps, rounding to nearest.
        let v = c as u32 * 5 / 255;
        ((v * 255 + 2) / 5) as u8
    };
    let cube = |c: u8| -> u8 { {
        let levels: [u8; 6] = [0, 95, 135, 175, 215, 255];
        let mut best = 0u8;
        let mut best_d = i32::MAX;
        for (i, l) in levels.iter().enumerate() {
            let d = (*l as i32 - c as i32).abs();
            if d < best_d {
                best_d = d;
                best = i as u8;
            }
        }
        best
    } };
    let (r, g, b) = (cube(color.0), cube(color.1), cube(color.2));
    let cube_rgb = |i: u8| -> i32 {
        let levels: [u8; 6] = [0, 95, 135, 175, 215, 255];
        levels[i as usize] as i32
    };
    let cube_dist = (cube_rgb(r) - color.0 as i32).pow(2)
        + (cube_rgb(g) - color.1 as i32).pow(2)
        + (cube_rgb(b) - color.2 as i32).pow(2);
    // Grey ramp: 232..255, steps of 10 from 8.
    let avg = ((color.0 as i32 + color.1 as i32 + color.2 as i32) / 3).clamp(0, 255);
    let grey_i = ((avg - 8).clamp(0, 238) / 10) as u8;
    let grey_v = 8 + grey_i as i32 * 10;
    let grey_dist = (grey_v - color.0 as i32).pow(2)
        + (grey_v - color.1 as i32).pow(2)
        + (grey_v - color.2 as i32).pow(2);
    let _ = q;
    if grey_dist < cube_dist {
        232 + grey_i
    } else {
        16 + 36 * r + 6 * g + b
    }
}

fn ansi_for(cell: &Cell) -> String {
    format!(
        "\x1b[{}38;5;{}m{}\x1b[0m",
        if cell.bold { "1;" } else { "" },
        ansi_index(cell.color),
        cell.ch
    )
}

/// Render the whole map as text, one terminal row per map row.
pub fn render_ascii(w: &World, opts: &RenderOpts) -> String {
    let mut out = String::with_capacity(w.width as usize * w.height as usize * 2);
    for row in 0..w.height as i32 {
        for col in 0..w.width as i32 {
            let h = crate::hex::Hex::from_offset(col, row);
            let c = cell(w, h, opts);
            if opts.color {
                out.push_str(&ansi_for(&c));
            } else {
                out.push(c.ch);
            }
        }
        out.push('\n');
    }
    out
}

/// A colour key for the characters `render_ascii` can emit.
pub fn legend() -> String {
    let mut s = String::from("legend\n");
    s.push_str("  ~ water / lava / river   * ice, snow   , beach, desert, savanna\n");
    s.push_str("  . grass   \" forest, jungle, taiga   % swamp   : tundra\n");
    s.push_str("  ^ mountain   + crystal   & enchanted   o candy   # ash, waste, corrupted\n");
    s.push_str("  = road   ! fire   K king   L leader   S soldier   p civilian   M monster   a animal\n");
    s
}

/// Render the world into an RGB image, one `scale`x`scale` block per hex.
pub fn color_map(w: &World, scale: u32, opts: &RenderOpts) -> RgbImage {
    let scale = scale.max(1);
    let mut img = RgbImage::new(w.width as u32 * scale, w.height as u32 * scale);
    for row in 0..w.height as i32 {
        for col in 0..w.width as i32 {
            let h = crate::hex::Hex::from_offset(col, row);
            let c = cell(w, h, opts);
            let x = col as u32 * scale;
            let y = row as u32 * scale;
            let base = if opts.color { c.color } else { (24, 24, 24) };
            if c.ch == ' ' {
                continue;
            }
            img.rect(x, y, scale, scale, base);
            // Give buildings a roof marker so towns read at a glance.
            if let Some(t) = w.tile(h) {
                if t.owner.is_some() && w.village(t.owner.unwrap()).is_some() {
                    let small = scale.max(2);
                    let roof = shade(base, 0.55);
                    img.rect(x + scale / 4, y + scale / 4, small / 2, small / 2, roof);
                }
            }
        }
    }
    img
}

/// State hash rendered as a short hex string, for logs and tests.
pub fn hash_line(w: &World) -> String {
    format!("0x{:016x}", w.state_hash())
}

fn fit(s: &str, width: usize) -> String {
    if s.chars().count() > width {
        let mut out: String = s.chars().take(width.saturating_sub(1)).collect();
        out.push('…');
        out
    } else {
        s.to_string()
    }
}

/// A text panel: summary, census, effects and the tail of the chronicle.
pub fn render_panel(w: &World, width: usize) -> String {
    let width = width.max(20);
    let mut out = String::new();
    out.push_str(&fit(&w.summary(), width));
    out.push('\n');
    out.push_str(&"-".repeat(width));
    out.push('\n');
    out.push_str(&format!(
        "tick {}  hash {}  rng {:016x}\n",
        w.tick,
        hash_line(w),
        w.rng.state()
    ));
    out.push_str(&format!(
        "age: {} ({}/{} years)\n",
        w.age.age.name(),
        w.age.years_in_age,
        w.age.duration_years
    ));
    out.push_str(&format!("effects in flight: {}\n", w.effects.count()));
    out.push_str(&"-".repeat(width));
    out.push('\n');
    for line in w.census() {
        out.push_str(&fit(&line, width));
        out.push('\n');
    }
    let events = w.chronicle_tail(8);
    if !events.is_empty() {
        out.push_str(&"-".repeat(width));
        out.push('\n');
        out.push_str("chronicle\n");
        for e in &events {
            out.push_str(&fit(
                &format!("  y{:>4} [{:>9}] {}", e.year, e.kind.name(), e.text),
                width,
            ));
            out.push('\n');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worldgen::{GenParams, WorldType};

    fn world() -> World {
        let mut w = World::generate(GenParams::new(40, 24, 11, WorldType::Continents));
        w.step_n(200);
        w
    }

    #[test]
    fn every_biome_has_a_colour_and_a_character() {
        let biomes = [
            Biome::Ocean,
            Biome::Shallow,
            Biome::Ice,
            Biome::Beach,
            Biome::Grass,
            Biome::Savanna,
            Biome::Desert,
            Biome::Jungle,
            Biome::Forest,
            Biome::Taiga,
            Biome::Swamp,
            Biome::Mushroom,
            Biome::Tundra,
            Biome::Snow,
            Biome::Permafrost,
            Biome::Mountain,
            Biome::Corrupted,
            Biome::Infernal,
            Biome::Crystal,
            Biome::Enchanted,
            Biome::Candy,
            Biome::Wasteland,
            Biome::Ash,
            Biome::Lava,
        ];
        for b in biomes {
            let c = biome_color(b);
            assert!(
                (c.0 as u32 + c.1 as u32 + c.2 as u32) > 30,
                "{b:?} is nearly black"
            );
            assert!(!biome_char(b).is_whitespace(), "{b:?} has no character");
        }
    }

    #[test]
    fn ansi_colours_are_in_range_and_ordered() {
        assert!(ansi_index((0, 0, 0)) >= 16);
        assert!(ansi_index((255, 255, 255)) >= 16);
        // Pure red should land in the 6x6x6 cube's red face.
        let red = ansi_index((255, 0, 0));
        assert!((16..=231).contains(&red), "red mapped to {red}");
        assert!(ansi_index((128, 128, 128)) != ansi_index((20, 20, 20)));
    }

    #[test]
    fn ascii_render_matches_the_map_size_and_is_deterministic() {
        let w = world();
        let a = render_ascii(&w, &RenderOpts::mono());
        let b = render_ascii(&w, &RenderOpts::mono());
        assert_eq!(a, b, "rendering must be a pure function of the world");
        let lines: Vec<&str> = a.lines().collect();
        assert_eq!(lines.len(), w.height as usize);
        for line in &lines {
            assert_eq!(line.chars().count(), w.width as usize);
        }
        assert!(!a.contains('\x1b'), "mono output must have no escapes");
    }

    #[test]
    fn colour_mode_emits_escapes_and_stays_the_same_shape() {
        let w = world();
        let colored = render_ascii(&w, &RenderOpts::default());
        assert!(colored.contains("\x1b[38;5;"));
        assert_eq!(colored.lines().count(), w.height as usize);
    }

    #[test]
    fn image_is_exactly_the_map_times_scale() {
        let w = world();
        let img = color_map(&w, 4, &RenderOpts::default());
        assert_eq!(img.width, w.width as u32 * 4);
        assert_eq!(img.height, w.height as u32 * 4);
        let plain = color_map(&w, 1, &RenderOpts::plain());
        assert_eq!(plain.width, w.width as u32);
    }

    #[test]
    fn panel_reports_the_hash_and_the_age() {
        let w = world();
        let panel = render_panel(&w, 60);
        assert!(panel.contains(&hash_line(&w)));
        assert!(panel.contains(w.age.age.name()));
        for line in panel.lines() {
            assert!(line.chars().count() <= 60, "panel line too wide: {line:?}");
        }
    }

    #[test]
    fn fires_and_units_show_up_in_the_overlay() {
        let mut w = World::generate(GenParams::new(30, 20, 3, WorldType::Pangaea));
        let land = w.random_land_tile().unwrap();
        let idx = w.idx(land).unwrap();
        w.tiles[idx].fire = 5;
        let text = render_ascii(&w, &RenderOpts::mono());
        assert!(text.contains('!'), "a burning tile should render as '!'");
        let spawn = w.random_land_tile().unwrap();
        w.spawn_unit(crate::races::Race::Dragon, spawn, UnitKind::Monster);
        let text = render_ascii(&w, &RenderOpts::mono());
        assert!(text.contains('M'), "a monster should render as 'M'");
    }
}
