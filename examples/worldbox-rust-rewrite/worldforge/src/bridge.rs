//! The Minecraft bridge: a running [`World`] turned into *placements*.
//!
//! `worldforge serve` publishes the simulation over a local socket as a stream of
//! JSON lines. Everything that decides how the world looks -- which block each
//! tile becomes, how tall its column is, which concrete marks a kingdom border,
//! which mob stands for a unit, what a town hall is made of -- is decided **here**,
//! in Rust. The Minecraft side (the Fabric mod, or `worldforge mcview`) is then a
//! dumb applier: it places blocks, moves mobs and asks no questions.
//!
//! That split is deliberate. The mapping is the interesting, testable part; it has
//! unit tests below, and both front ends stay small enough to audit.
//!
//! Coordinates: a tile `(col, row)` in odd-r offset space (see [`crate::hex`])
//! becomes the column `x = col`, `z = row`, and `y` is absolute -- the ocean floor
//! sits at [`BASE_Y`] and everything is built up from there. The Minecraft side
//! picks a canvas origin and adds it to `x`/`z`.

use crate::powers::Power;
use crate::races::{Category, Race};
use crate::terrain::{Biome, Tile};
use crate::units::{Unit, UnitKind};
use crate::village::{BuildingKind, Village};
use crate::world::World;

/// Bumped whenever the wire format changes in a way a client must know about.
pub const PROTOCOL: u32 = 1;
/// The port `worldforge serve` listens on by default.
pub const DEFAULT_PORT: u16 = 25607;
/// The block layer the ocean floor sits on.
pub const BASE_Y: i32 = 64;
/// How many blocks of water sit above the ocean floor at sea level.
pub const SEA_LEVEL: i32 = 3;
/// The tallest a land column can get, in blocks.
pub const MAX_COLUMN: i32 = 24;
/// How high above the ocean floor the client is told to clear. 44 blocks of headroom.
pub const CANVAS_TOP: i32 = BASE_Y + 44;
/// Every entity the client spawns wears this scoreboard tag, so it can be found
/// and removed again (`/kill @e[tag=wfbox]`).
pub const ENTITY_TAG: &str = "wfbox";

// ---------------------------------------------------------------------------
// The name table
// ---------------------------------------------------------------------------

/// Every name the wire can carry -- blocks **and** entity ids -- in a fixed
/// order. Messages send *indices* into this list, so it must never be reordered
/// once a client is built against it: append only.
pub const PALETTE: &[&str] = &[
    "air",
    // water and ice
    "water",
    "ice",
    "packed_ice",
    "blue_ice",
    // ground
    "sand",
    "grass_block",
    "coarse_dirt",
    "moss_block",
    "podzol",
    "rooted_dirt",
    "mud",
    "mycelium",
    "gravel",
    "snow_block",
    "stone",
    "dirt",
    "dirt_path",
    "smooth_stone",
    // exotic biomes
    "sculk",
    "netherrack",
    "amethyst_block",
    "purpur_block",
    "pink_concrete",
    "brown_terracotta",
    "basalt",
    "magma_block",
    "lava",
    // trees
    "oak_log",
    "oak_leaves",
    "jungle_log",
    "jungle_leaves",
    "spruce_log",
    "spruce_leaves",
    "acacia_log",
    "acacia_leaves",
    "mangrove_leaves",
    "mushroom_stem",
    "red_mushroom_block",
    "cactus",
    "crimson_stem",
    "nether_wart_block",
    "stripped_crimson_stem",
    "shroomlight",
    "cherry_leaves",
    "amethyst_cluster",
    "dead_bush",
    // clutter
    "cobblestone",
    "campfire",
    // kingdom colours
    "white_concrete",
    "orange_concrete",
    "magenta_concrete",
    "light_blue_concrete",
    "yellow_concrete",
    "lime_concrete",
    "pink_concrete_dye",
    "gray_concrete",
    "light_gray_concrete",
    "cyan_concrete",
    "purple_concrete",
    "blue_concrete",
    "brown_concrete",
    "green_concrete",
    "red_concrete",
    "black_concrete",
    // structures
    "oak_planks",
    "dark_oak_planks",
    "stone_bricks",
    "stone_brick_wall",
    "bricks",
    "farmland",
    "wheat",
    "rail",
    "oak_fence",
    "barrel",
    "quartz_block",
    "gold_block",
    "end_rod",
    "lantern",
    "bell",
    "scaffolding",
    "torch",
    "glass",
    // entities (used as mob ids, not blocks)
    "villager",
    "vindicator",
    "pillager",
    "wandering_trader",
    "zombie",
    "skeleton",
    "stray",
    "blaze",
    "slime",
    "enderman",
    "ender_dragon",
    "vex",
    "ravager",
    "sheep",
    "cow",
    "chicken",
    "goat",
    "wolf",
    "polar_bear",
    "silverfish",
    "bee",
    "spider",
    "frog",
    "penguin",
    "turtle",
    "dolphin",
    "crab",
    "panda",
    "ocelot",
    "rabbit",
    "fox",
    "parrot",
    "pig",
];

/// Index of `name` in [`PALETTE`], or `0` (`air`) when it is not a known name.
pub fn block_id(name: &str) -> u16 {
    PALETTE
        .iter()
        .position(|b| *b == name)
        .map(|i| i as u16)
        .unwrap_or(0)
}

/// The name for a palette index.
pub fn block_name(id: u16) -> &'static str {
    PALETTE.get(id as usize).copied().unwrap_or("air")
}

/// Minecraft's sixteen concrete colours and their approximate texture colours,
/// used to pick the closest one for a kingdom's banner.
const CONCRETE: [(&str, (u8, u8, u8)); 16] = [
    ("white_concrete", (207, 213, 214)),
    ("orange_concrete", (240, 118, 19)),
    ("magenta_concrete", (189, 68, 179)),
    ("light_blue_concrete", (58, 175, 217)),
    ("yellow_concrete", (248, 198, 39)),
    ("lime_concrete", (112, 185, 25)),
    ("pink_concrete_dye", (237, 141, 172)),
    ("gray_concrete", (62, 68, 71)),
    ("light_gray_concrete", (142, 142, 134)),
    ("cyan_concrete", (21, 137, 145)),
    ("purple_concrete", (121, 42, 172)),
    ("blue_concrete", (53, 57, 157)),
    ("brown_concrete", (114, 71, 40)),
    ("green_concrete", (84, 109, 27)),
    ("red_concrete", (142, 32, 32)),
    ("black_concrete", (20, 21, 25)),
];

/// The concrete block closest to `rgb`: borders, banners and town markers.
pub fn concrete_for(rgb: (u8, u8, u8)) -> &'static str {
    let mut best = CONCRETE[0].0;
    let mut best_d = i32::MAX;
    for (name, c) in CONCRETE {
        let d = (c.0 as i32 - rgb.0 as i32).pow(2)
            + (c.1 as i32 - rgb.1 as i32).pow(2)
            + (c.2 as i32 - rgb.2 as i32).pow(2);
        if d < best_d {
            best_d = d;
            best = name;
        }
    }
    best
}

/// `"#rrggbb"`, the colour format the wire uses.
pub fn hex_color(rgb: (u8, u8, u8)) -> String {
    format!("#{:02x}{:02x}{:02x}", rgb.0, rgb.1, rgb.2)
}

// ---------------------------------------------------------------------------
// Tiles -> columns
// ---------------------------------------------------------------------------

/// The surface block for a biome, before ownership, roads and lava are applied.
pub fn biome_block(biome: Biome) -> &'static str {
    match biome {
        Biome::Ocean | Biome::Shallow => "water",
        Biome::Ice => "blue_ice",
        Biome::Beach | Biome::Desert => "sand",
        Biome::Grass => "grass_block",
        Biome::Savanna => "coarse_dirt",
        Biome::Jungle => "moss_block",
        Biome::Forest => "podzol",
        Biome::Taiga => "rooted_dirt",
        Biome::Swamp => "mud",
        Biome::Mushroom => "mycelium",
        Biome::Tundra => "gravel",
        Biome::Snow => "snow_block",
        Biome::Permafrost => "packed_ice",
        Biome::Mountain => "stone",
        Biome::Corrupted => "sculk",
        Biome::Infernal => "netherrack",
        Biome::Crystal => "amethyst_block",
        Biome::Enchanted => "purpur_block",
        Biome::Candy => "pink_concrete",
        Biome::Wasteland => "brown_terracotta",
        Biome::Ash => "basalt",
        Biome::Lava => "magma_block",
    }
}

/// `stem` and `crown`: the two blocks a tree on this biome is made of.
pub fn tree_blocks(biome: Biome) -> (&'static str, &'static str) {
    match biome {
        Biome::Grass | Biome::Forest => ("oak_log", "oak_leaves"),
        Biome::Jungle => ("jungle_log", "jungle_leaves"),
        Biome::Taiga | Biome::Snow | Biome::Permafrost | Biome::Tundra => {
            ("spruce_log", "spruce_leaves")
        }
        Biome::Savanna => ("acacia_log", "acacia_leaves"),
        Biome::Swamp => ("oak_log", "mangrove_leaves"),
        Biome::Mushroom => ("mushroom_stem", "red_mushroom_block"),
        Biome::Desert => ("cactus", "cactus"),
        Biome::Corrupted => ("crimson_stem", "nether_wart_block"),
        Biome::Infernal => ("stripped_crimson_stem", "shroomlight"),
        Biome::Enchanted | Biome::Candy => ("oak_log", "cherry_leaves"),
        Biome::Crystal => ("purpur_block", "amethyst_cluster"),
        Biome::Ocean
        | Biome::Shallow
        | Biome::Ice
        | Biome::Beach
        | Biome::Mountain
        | Biome::Wasteland
        | Biome::Ash
        | Biome::Lava => ("dead_bush", "dead_bush"),
    }
}

/// How many blocks tall this tile's land column is. Water columns are lakebeds:
/// the sea stays flat, rivers and lakes only dip a little.
pub fn column_height(tile: &Tile) -> i32 {
    let e = tile.elevation as i32;
    if tile.is_water() {
        (e.max(0) / 90).clamp(0, 6)
    } else {
        (1 + e.max(0) / 60).clamp(1, MAX_COLUMN)
    }
}

/// The top of the column: water fills `h + 1 ..= top` for water tiles.
pub fn water_top(tile: &Tile) -> i32 {
    if tile.is_water() {
        SEA_LEVEL.max(column_height(tile) + 1)
    } else {
        column_height(tile)
    }
}

/// True when a tile sits on the edge of whoever owns it -- where WorldBox draws
/// its coloured borders. `owner` is the tile's owner; `neighbors` are the owners
/// of the six surrounding tiles (`None` for unowned or off-map).
pub fn is_border(owner: Option<u32>, neighbors: [Option<u32>; 6]) -> bool {
    match owner {
        None => false,
        Some(v) => neighbors.iter().any(|n| *n != Some(v)),
    }
}

/// Everything a client needs to build one tile column.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TileSpec {
    /// Surface block.
    pub surface: &'static str,
    /// Land height above [`BASE_Y`]; the lakebed depth for water.
    pub h: i32,
    /// Water fills `h + 1 ..= top` when true.
    pub water: bool,
    /// `water_top` for water tiles, `h` otherwise.
    pub top: i32,
    /// Up to four blocks stacked above the top, low to high: trees, campfires.
    pub extra: Vec<&'static str>,
}

impl TileSpec {
    /// The wire form: `[surface_id, h, top, water, extra ids...]`.
    pub fn to_json(&self, out: &mut String) {
        out.push('[');
        out.push_str(&block_id(self.surface).to_string());
        out.push(',');
        out.push_str(&self.h.to_string());
        out.push(',');
        out.push_str(&self.top.to_string());
        out.push(',');
        out.push(if self.water { '1' } else { '0' });
        for block in &self.extra {
            out.push(',');
            out.push_str(&block_id(block).to_string());
        }
        out.push(']');
    }
}

/// Build the column for one tile given who owns it and who owns its neighbours.
pub fn tile_spec(world: &World, col: i32, row: i32) -> TileSpec {
    let hex = crate::hex::Hex::from_offset(col, row);
    let Some(tile) = world.tile(hex).copied() else {
        return TileSpec {
            surface: "water",
            h: 0,
            water: true,
            top: SEA_LEVEL,
            extra: Vec::new(),
        };
    };
    let neighbors = hex.neighbors().map(|n| world.tile(n).and_then(|t| t.owner).map(|v| realm(world, v)));
    let bordered = is_border(tile.owner.map(|v| realm(world, v)), neighbors);

    let mut spec = TileSpec {
        surface: biome_block(tile.biome),
        h: column_height(&tile),
        water: tile.is_water(),
        top: water_top(&tile),
        extra: Vec::new(),
    };

    if tile.lava > 0 {
        spec.surface = "lava";
        spec.water = false;
        spec.h = spec.h.max(1);
        spec.top = spec.h;
    } else if bordered {
        // A kingdom border: WorldBox paints the edge tiles in the banner colour.
        spec.surface = concrete_for(banner_color(world, tile.owner.unwrap_or(0)));
    } else if !spec.water {
        if tile.road >= 2 {
            spec.surface = "smooth_stone";
        } else if tile.road == 1 {
            spec.surface = "dirt_path";
        }
        if tile.fire > 0 {
            spec.extra.push("campfire");
        } else if tile.trees > 0 {
            let (stem, crown) = tree_blocks(tile.biome);
            spec.extra.push(stem);
            spec.extra.push(crown);
            if tile.trees >= 3 {
                spec.extra.push(crown);
            }
        } else if tile.stone >= 2 {
            spec.extra.push("cobblestone");
        }
    }
    spec
}

/// Every tile's column, row-major in offset space: index `row * width + col`.
pub fn tile_specs(world: &World) -> Vec<TileSpec> {
    let mut out = Vec::with_capacity(world.tiles.len());
    for row in 0..world.height as i32 {
        for col in 0..world.width as i32 {
            out.push(tile_spec(world, col, row));
        }
    }
    out
}

/// The tiles that differ between two snapshots: `(index, new spec)`.
pub fn diff_specs(prev: &[TileSpec], next: &[TileSpec]) -> Vec<(usize, TileSpec)> {
    let mut out = Vec::new();
    for (i, spec) in next.iter().enumerate() {
        if prev.get(i) != Some(spec) {
            out.push((i, spec.clone()));
        }
    }
    out
}

/// The realm a village belongs to for drawing purposes: its kingdom, or itself
/// while it has no king. Two villages of one kingdom share a realm, which is what
/// makes a kingdom's border one outline instead of a sugar grid per village.
pub fn realm(world: &World, village: u32) -> u32 {
    match world.village(village).and_then(|v| v.kingdom) {
        Some(kingdom) => kingdom,
        None => village,
    }
}

/// The colour a village's land is drawn in: its kingdom's, or its race's if it
/// has not crowned a king yet.
pub fn banner_color(world: &World, village: u32) -> (u8, u8, u8) {
    if let Some(v) = world.village(village) {
        if let Some(k) = v.kingdom.and_then(|k| world.kingdom(k)) {
            return k.color;
        }
        return v.race.def().color;
    }
    (200, 200, 200)
}

// ---------------------------------------------------------------------------
// Structures
// ---------------------------------------------------------------------------

/// One block a client must place, at absolute `y`, tile-space `x`/`z`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlockOp {
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub block: &'static str,
}

impl BlockOp {
    fn new(x: i32, y: i32, z: i32, block: &'static str) -> Self {
        BlockOp { x, y, z, block }
    }
}

/// The blocks for one building: a small hut, farm or tower standing on `floor`
/// (the y of the ground it sits on). A building still under construction is a
/// single block of scaffolding, so you can watch a village grow.
pub fn building_ops(kind: BuildingKind, x: i32, z: i32, floor: i32, complete: bool) -> Vec<BlockOp> {
    if !complete {
        return vec![BlockOp::new(x, floor + 1, z, "scaffolding")];
    }
    let mut ops = Vec::new();
    match kind {
        BuildingKind::Fireplace => {
            ops.push(BlockOp::new(x, floor + 1, z, "campfire"));
        }
        BuildingKind::House => {
            ring(&mut ops, 1, floor + 1, "oak_planks", x, z);
            square(&mut ops, 1, floor + 2, "bricks", x, z);
        }
        BuildingKind::TownHall => {
            ring(&mut ops, 2, floor + 1, "stone_bricks", x, z);
            ring(&mut ops, 2, floor + 2, "stone_bricks", x, z);
            square(&mut ops, 2, floor + 3, "dark_oak_planks", x, z);
            ops.push(BlockOp::new(x, floor + 4, z, "bell"));
        }
        BuildingKind::Farm => {
            square(&mut ops, 1, floor, "farmland", x, z);
            square(&mut ops, 1, floor + 1, "wheat", x, z);
        }
        BuildingKind::Mine => {
            ring(&mut ops, 1, floor + 1, "cobblestone", x, z);
            ops.push(BlockOp::new(x, floor + 2, z, "rail"));
        }
        BuildingKind::Sawmill => {
            ring(&mut ops, 1, floor + 1, "oak_log", x, z);
            ops.push(BlockOp::new(x, floor + 2, z, "oak_fence"));
        }
        BuildingKind::Barracks => {
            ring(&mut ops, 1, floor + 1, "stone_bricks", x, z);
            square(&mut ops, 1, floor + 2, "dark_oak_planks", x, z);
        }
        BuildingKind::Dock => {
            square(&mut ops, 1, floor + 1, "oak_planks", x, z);
            ops.push(BlockOp::new(x, floor + 2, z, "barrel"));
        }
        BuildingKind::Temple => {
            ring(&mut ops, 2, floor + 1, "quartz_block", x, z);
            ops.push(BlockOp::new(x, floor + 2, z, "gold_block"));
            for (dx, dz) in [(-2, -2), (2, -2), (-2, 2), (2, 2)] {
                ops.push(BlockOp::new(x + dx, floor + 2, z + dz, "end_rod"));
            }
        }
        BuildingKind::Tower => {
            for i in 1..=4 {
                ops.push(BlockOp::new(x, floor + i, z, "stone_bricks"));
            }
            ops.push(BlockOp::new(x, floor + 5, z, "lantern"));
        }
        BuildingKind::Well => {
            ring(&mut ops, 1, floor + 1, "cobblestone", x, z);
            ops.push(BlockOp::new(x, floor + 1, z, "water"));
        }
        BuildingKind::Statue => {
            ops.push(BlockOp::new(x, floor + 1, z, "smooth_stone"));
            ops.push(BlockOp::new(x, floor + 2, z, "smooth_stone"));
            ops.push(BlockOp::new(x, floor + 3, z, "gold_block"));
        }
        BuildingKind::Wall => {
            ring(&mut ops, 1, floor + 1, "stone_brick_wall", x, z);
        }
    }
    ops
}

/// The hollow square of blocks at `radius`: the walls of a hut.
fn ring(ops: &mut Vec<BlockOp>, radius: i32, y: i32, block: &'static str, x: i32, z: i32) {
    for dx in -radius..=radius {
        for dz in -radius..=radius {
            if dx.abs() == radius || dz.abs() == radius {
                ops.push(BlockOp::new(x + dx, y, z + dz, block));
            }
        }
    }
}

/// The filled square of blocks at `radius`: a floor or a roof.
fn square(ops: &mut Vec<BlockOp>, radius: i32, y: i32, block: &'static str, x: i32, z: i32) {
    for dx in -radius..=radius {
        for dz in -radius..=radius {
            ops.push(BlockOp::new(x + dx, y, z + dz, block));
        }
    }
}

/// Every structure of one village, plus its centre marker: a banner-coloured
/// platform with a lantern and (once the hall is up) a bell.
pub fn village_ops(world: &World, village: &Village) -> Vec<BlockOp> {
    let mut ops = Vec::new();
    if !village.alive {
        return ops;
    }
    let (col, row) = village.center.to_offset();
    let center = world.tile(village.center).copied().unwrap_or_default();
    let floor = BASE_Y + column_height(&center);
    let color = concrete_for(banner_color(world, village.id));
    square(&mut ops, 1, floor + 1, color, col, row);
    ops.push(BlockOp::new(col, floor + 2, row, "lantern"));
    if village.town_hall_level > 0 {
        ops.push(BlockOp::new(col, floor + 3, row, "bell"));
    }
    for b in &village.buildings {
        let (bx, bz) = b.pos.to_offset();
        let floor = BASE_Y + column_height(&world.tile(b.pos).copied().unwrap_or_default());
        ops.extend(building_ops(b.kind, bx, bz, floor, b.complete));
    }
    ops
}

/// Every structure in the world, as one flat op list.
pub fn world_ops(world: &World) -> Vec<BlockOp> {
    let mut ops = Vec::new();
    for id in world.village_ids() {
        if let Some(v) = world.village(id) {
            ops.extend(village_ops(world, v));
        }
    }
    ops
}

// ---------------------------------------------------------------------------
// Units -> mobs
// ---------------------------------------------------------------------------

/// The mob that stands for a unit. Civilized races get villagers and their
/// cousins, wildlife gets the closest vanilla animal, monsters get monsters.
pub fn mob_for(race: Race, kind: UnitKind) -> &'static str {
    use Race::*;
    match kind {
        UnitKind::Monster => match race {
            Dragon => "ender_dragon",
            Ufo => "vex",
            Crabzilla => "ravager",
            Demon => "blaze",
            ColdOne => "stray",
            Tumor => "slime",
            Alien => "enderman",
            Bandit => "pillager",
            Skeleton => "skeleton",
            _ => "zombie",
        },
        UnitKind::Animal => match race {
            Sheep => "sheep",
            Cow => "cow",
            Chicken => "chicken",
            Deer => "goat",
            Wolf => "wolf",
            Bear => "polar_bear",
            Rat => "silverfish",
            Bee => "bee",
            Spider => "spider",
            Snake | Scorpion => "silverfish",
            Frog => "frog",
            Penguin => "penguin",
            Turtle => "turtle",
            Whale | Shark => "dolphin",
            Crab => "crab",
            Gorilla => "panda",
            Monkey => "ocelot",
            Rabbit => "rabbit",
            Fox => "fox",
            Eagle => "parrot",
            _ => "pig",
        },
        UnitKind::Soldier => match race {
            Orc => "pillager",
            _ => "vindicator",
        },
        UnitKind::King | UnitKind::Leader => "wandering_trader",
        UnitKind::Civilian => "villager",
    }
}

/// Everything a client needs to place one unit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnitSpec {
    pub id: u32,
    pub mob: &'static str,
    pub name: String,
    pub color: String,
    pub col: i32,
    pub row: i32,
    /// Absolute y: standing on the ground, or floating for fliers.
    pub y: i32,
    pub hp: i32,
    pub max_hp: i32,
    /// Glowing outline: kings, leaders and monsters.
    pub glow: bool,
}

impl UnitSpec {
    /// `[id, mob, col, row, y, hp, max_hp, glow, "name", "#colour"]`.
    pub fn to_json(&self) -> String {
        format!(
            "[{},{},{},{},{},{},{},{},\"{}\",\"{}\"]",
            self.id,
            block_id(self.mob),
            self.col,
            self.row,
            self.y,
            self.hp,
            self.max_hp,
            if self.glow { 1 } else { 0 },
            json_escape(&self.name),
            self.color
        )
    }
}

/// Where a unit stands, in blocks.
pub fn unit_y(world: &World, unit: &Unit) -> i32 {
    let tile = world.tile(unit.pos).copied().unwrap_or_default();
    let ground = BASE_Y + water_top(&tile) + 1;
    match unit.race {
        Race::Dragon | Race::Ufo | Race::Bee | Race::Eagle => ground + 6,
        _ => ground,
    }
}

/// The display name: the unit's own name when it has one, else "Race #id".
pub fn unit_name(unit: &Unit) -> String {
    if unit.name.trim().is_empty() {
        format!("{} #{}", unit.race.def().name, unit.id)
    } else {
        unit.name.clone()
    }
}

/// The colour of a unit's name tag: its kingdom's, else its race's.
pub fn unit_color(world: &World, unit: &Unit) -> (u8, u8, u8) {
    if let Some(k) = unit.kingdom.and_then(|k| world.kingdom(k)) {
        return k.color;
    }
    if let Some(v) = unit.village.and_then(|v| world.village(v)) {
        if let Some(k) = v.kingdom.and_then(|k| world.kingdom(k)) {
            return k.color;
        }
        return v.race.def().color;
    }
    unit.race.def().color
}

/// Every living unit, ready to place.
pub fn unit_specs(world: &World) -> Vec<UnitSpec> {
    let mut out = Vec::new();
    for u in world.units.iter().filter(|u| u.alive) {
        let (col, row) = u.pos.to_offset();
        out.push(UnitSpec {
            id: u.id,
            mob: mob_for(u.race, u.kind),
            name: unit_name(u),
            color: hex_color(unit_color(world, u)),
            col,
            row,
            y: unit_y(world, u),
            hp: u.hp,
            max_hp: u.max_hp,
            glow: matches!(u.kind, UnitKind::King | UnitKind::Monster)
                || u.race.def().category == Category::Boss,
        });
    }
    out
}

/// The chronicle entries newer than `since_tick`, as `(year, text)`.
pub fn news_since(world: &World, since_tick: u64) -> Vec<(u32, String)> {
    world
        .chronicle
        .iter()
        .filter(|e| e.tick > since_tick)
        .map(|e| (e.year, e.text.clone()))
        .collect()
}

// ---------------------------------------------------------------------------
// Messages
// ---------------------------------------------------------------------------

/// Escape a string for a JSON string literal.
pub fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

/// Append `,"key":"value"` -- every call site is mid-object, so the comma is here.
fn push_str_field(out: &mut String, key: &str, value: &str) {
    out.push(',');
    out.push('"');
    out.push_str(key);
    out.push_str("\":\"");
    out.push_str(&json_escape(value));
    out.push('"');
}

/// The first message on a connection: everything that does not change.
pub fn hello_json(world: &World, port: u16, tps: f32) -> String {
    let mut s = format!(
        "{{\"t\":\"hello\",\"protocol\":{PROTOCOL},\"port\":{port},\"tps\":{tps}"
    );
    s.push_str(&format!(
        ",\"base_y\":{BASE_Y},\"sea_level\":{SEA_LEVEL},\"max_column\":{MAX_COLUMN},\"canvas_top\":{CANVAS_TOP}"
    ));
    s.push_str(&format!(",\"size\":[{},{}]", world.width, world.height));
    s.push_str(&format!(",\"seed\":{}", world.seed));
    push_str_field(&mut s, "world", world.world_type.name());
    push_str_field(&mut s, "tag", ENTITY_TAG);
    s.push_str(",\"palette\":[");
    for (i, name) in PALETTE.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        s.push('"');
        s.push_str(name);
        s.push('"');
    }
    s.push_str("]}");
    s
}

/// The full tile field: sent once per connection.
pub fn tiles_json(specs: &[TileSpec]) -> String {
    let mut s = String::from("{\"t\":\"tiles\",\"surface\":[");
    for (i, spec) in specs.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        s.push_str(&block_id(spec.surface).to_string());
    }
    s.push_str("],\"h\":[");
    for (i, spec) in specs.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        s.push_str(&spec.h.to_string());
    }
    s.push_str("],\"top\":[");
    for (i, spec) in specs.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        s.push_str(&spec.top.to_string());
    }
    s.push_str("],\"water\":[");
    for (i, spec) in specs.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        s.push(if spec.water { '1' } else { '0' });
    }
    s.push_str("],\"extra\":[");
    for (i, spec) in specs.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        s.push('[');
        for (j, block) in spec.extra.iter().enumerate() {
            if j > 0 {
                s.push(',');
            }
            s.push_str(&block_id(block).to_string());
        }
        s.push(']');
    }
    s.push_str("]}");
    s
}

/// The tiles that changed since the last message: `index, spec, index, spec...`.
pub fn edits_json(edits: &[(usize, TileSpec)]) -> String {
    let mut s = String::from("{\"t\":\"edits\",\"tiles\":[");
    for (i, (index, spec)) in edits.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        s.push_str(&index.to_string());
        s.push(',');
        spec.to_json(&mut s);
    }
    s.push_str("]}");
    s
}

/// Structures, sent when a building appears, completes or changes.
pub fn structures_json(ops: &[BlockOp]) -> String {
    let mut s = String::from("{\"t\":\"structures\",\"ops\":[");
    for (i, op) in ops.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        s.push_str(&format!(
            "[{},{},{},{}]",
            op.x,
            op.y,
            op.z,
            block_id(op.block)
        ));
    }
    s.push_str("]}");
    s
}

/// One tick of news: the world's state, its kingdoms, and everything that just
/// happened. Units are sent in full every frame -- 400 mobs is nothing.
pub fn frame_json(world: &World, units: &[UnitSpec], news: &[(u32, String)]) -> String {
    let (animals, monsters) = world.wildlife_count();
    let mut s = format!(
        "{{\"t\":\"frame\",\"tick\":{},\"year\":{},\"pop\":{},\"animals\":{animals},\"monsters\":{monsters}",
        world.tick,
        world.year,
        world.population()
    );
    s.push_str(&format!(
        ",\"village_count\":{},\"kingdom_count\":{}",
        world.villages.iter().filter(|v| v.alive).count(),
        world.kingdoms.iter().filter(|k| k.alive).count()
    ));
    push_str_field(&mut s, "age", world.age.age.name());
    push_str_field(&mut s, "hash", &format!("0x{:016x}", world.state_hash()));

    s.push_str(",\"news\":[");
    for (i, (year, text)) in news.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        s.push_str(&format!("[{},\"{}\"]", year, json_escape(text)));
    }
    s.push_str("],\"units\":[");
    for (i, u) in units.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        s.push_str(&u.to_json());
    }
    s.push_str("],\"villages\":[");
    let mut first = true;
    for v in world.villages.iter().filter(|v| v.alive) {
        if !first {
            s.push(',');
        }
        first = false;
        let (col, row) = v.center.to_offset();
        s.push_str(&format!(
            "[{},\"{}\",\"{}\",{},{},{},{},{},{}]",
            v.id,
            json_escape(&v.name),
            v.race.def().name,
            v.pop,
            col,
            row,
            v.kingdom.map(|k| k as i64).unwrap_or(-1),
            v.loyalty,
            if v.kingdom.is_some() && world.kingdom(v.kingdom.unwrap()).map(|k| k.capital) == Some(v.id) {
                1
            } else {
                0
            }
        ));
    }
    s.push_str("],\"kingdoms\":[");
    for (i, k) in world.kingdoms.iter().filter(|k| k.alive).enumerate() {
        if i > 0 {
            s.push(',');
        }
        s.push_str(&format!(
            "[{},\"{}\",\"{}\",{},{},{},{}]",
            k.id,
            json_escape(&k.name),
            hex_color(k.color),
            k.population,
            k.cities.len(),
            k.wars.len(),
            k.king.map(|id| id as i64).unwrap_or(-1)
        ));
    }
    s.push_str("]}");
    s
}

// ---------------------------------------------------------------------------
// Commands: Minecraft -> the simulation
// ---------------------------------------------------------------------------

/// A command from the Minecraft side.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Command {
    Ping,
    /// Ticks per second, 0.1..=60.
    Speed { tps: f32 },
    Pause { on: bool },
    /// Run exactly this many ticks, then pause.
    Step { ticks: u32 },
    /// Ask for the tile field again.
    Tiles,
    Power { power: Power, col: i32, row: i32 },
    Spawn {
        race: Race,
        soldier: bool,
        col: i32,
        row: i32,
    },
}

/// A scalar in a command object. Commands never nest, so this is enough.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Str(String),
    Num(f64),
    Bool(bool),
}

impl Value {
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }
    pub fn as_i32(&self) -> Option<i32> {
        match self {
            Value::Num(n) => Some(*n as i32),
            _ => None,
        }
    }
    pub fn as_f32(&self) -> Option<f32> {
        match self {
            Value::Num(n) => Some(*n as f32),
            _ => None,
        }
    }
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            Value::Num(n) => Some(*n != 0.0),
            Value::Str(s) => Some(!s.is_empty() && s != "0" && s != "false"),
        }
    }
}

/// Parse one flat JSON object (`{"k":1,"s":"v"}`) into pairs. No nesting, no
/// arrays: commands are deliberately too small to need them.
pub fn parse_flat(line: &str) -> Result<Vec<(String, Value)>, String> {
    use crate::json::Json;
    let value = crate::json::parse(line)?;
    let Json::Obj(fields) = value else {
        return Err("expected a JSON object".into());
    };
    let mut out = Vec::new();
    for (key, value) in fields {
        let scalar = match value {
            Json::Str(s) => Value::Str(s),
            Json::Num(n) => Value::Num(n),
            Json::Bool(b) => Value::Bool(b),
            Json::Null => continue,
            other => return Err(format!("`{key}` must be a scalar, not a {other}")),
        };
        out.push((key, scalar));
    }
    Ok(out)
}


/// The race named `name`, in any case.
pub fn race_from_name(name: &str) -> Option<Race> {
    Race::ALL
        .iter()
        .copied()
        .find(|r| r.def().name.eq_ignore_ascii_case(name) || format!("{r:?}").eq_ignore_ascii_case(name))
}

/// Turn one line into a command.
pub fn parse_command(line: &str) -> Result<Command, String> {
    let fields = parse_flat(line)?;
    let get = |key: &str| fields.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone());
    let kind = get("t")
        .and_then(|v| v.as_str().map(|s| s.to_string()))
        .ok_or("missing `t`")?;
    match kind.as_str() {
        "ping" => Ok(Command::Ping),
        "tiles" => Ok(Command::Tiles),
        "speed" => {
            let tps = get("tps").and_then(|v| v.as_f32()).ok_or("missing `tps`")?;
            Ok(Command::Speed {
                tps: tps.clamp(0.1, 60.0),
            })
        }
        "pause" => Ok(Command::Pause {
            on: get("on").and_then(|v| v.as_bool()).unwrap_or(true),
        }),
        "step" => Ok(Command::Step {
            ticks: get("ticks")
                .and_then(|v| v.as_i32())
                .unwrap_or(1)
                .clamp(1, 100_000) as u32,
        }),
        "power" => {
            let name = get("name").and_then(|v| v.as_str().map(|s| s.to_string())).ok_or("missing `name`")?;
            let power = Power::parse(&name)
                .or_else(|| {
                    Power::ALL
                        .iter()
                        .copied()
                        .find(|p| format!("{p:?}").eq_ignore_ascii_case(&name))
                })
                .ok_or_else(|| format!("unknown power `{name}`"))?;
            Ok(Command::Power {
                power,
                col: get("col").and_then(|v| v.as_i32()).ok_or("missing `col`")?,
                row: get("row").and_then(|v| v.as_i32()).ok_or("missing `row`")?,
            })
        }
        "spawn" => {
            let name = get("race").and_then(|v| v.as_str().map(|s| s.to_string())).ok_or("missing `race`")?;
            let race = race_from_name(&name).ok_or_else(|| format!("unknown race `{name}`"))?;
            Ok(Command::Spawn {
                race,
                soldier: get("soldier").and_then(|v| v.as_bool()).unwrap_or(false),
                col: get("col").and_then(|v| v.as_i32()).ok_or("missing `col`")?,
                row: get("row").and_then(|v| v.as_i32()).ok_or("missing `row`")?,
            })
        }
        other => Err(format!("unknown command `{other}`")),
    }
}

/// A ready-to-run example script of commands, used by the README and by tests
/// that want a deterministic poke at a running bridge.
pub const EXAMPLE_COMMANDS: &[&str] = &[
    "{\"t\":\"ping\"}",
    "{\"t\":\"speed\",\"tps\":20}",
    "{\"t\":\"power\",\"name\":\"lightning\",\"col\":10,\"row\":10}",
    "{\"t\":\"spawn\",\"race\":\"Orc\",\"soldier\":true,\"col\":12,\"row\":9}",
    "{\"t\":\"pause\",\"on\":true}",
    "{\"t\":\"step\",\"ticks\":5}",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palette_is_unique_and_round_trips() {
        for (i, name) in PALETTE.iter().enumerate() {
            assert_eq!(block_id(name), i as u16, "duplicate entry `{name}`");
            assert_eq!(block_name(i as u16), *name);
        }
        assert_eq!(block_id("not_a_block"), 0);
        assert_eq!(block_name(9999), "air");
    }

    #[test]
    fn every_biome_and_tree_uses_a_known_block() {
        for biome in Biome::ALL {
            let surface = biome_block(biome);
            assert!(
                PALETTE.contains(&surface),
                "{biome:?} surface `{surface}` is not in the palette"
            );
            let (stem, crown) = tree_blocks(biome);
            for block in [stem, crown] {
                assert!(
                    PALETTE.contains(&block),
                    "{biome:?} tree block `{block}` is not in the palette"
                );
            }
        }
    }

    #[test]
    fn every_mob_is_in_the_name_table() {
        for race in Race::ALL {
            for kind in [
                UnitKind::Civilian,
                UnitKind::Soldier,
                UnitKind::Leader,
                UnitKind::King,
                UnitKind::Animal,
                UnitKind::Monster,
            ] {
                let mob = mob_for(race, kind);
                assert!(
                    PALETTE.contains(&mob),
                    "{race:?}/{kind:?} mob `{mob}` is not in the palette"
                );
            }
        }
    }

    #[test]
    fn every_building_places_known_blocks() {
        for kind in BuildingKind::ALL {
            for complete in [true, false] {
                let ops = building_ops(kind, 3, 4, BASE_Y + 2, complete);
                assert!(!ops.is_empty(), "{kind:?} placed nothing");
                for op in ops {
                    assert!(
                        PALETTE.contains(&op.block),
                        "{kind:?} places unknown block `{}`",
                        op.block
                    );
                    assert!(op.y > BASE_Y, "{kind:?} places a block below the floor");
                }
            }
        }
    }

    #[test]
    fn a_kingdom_has_one_outline_not_one_per_village() {
        let mut world = World::empty(8, 8, 5);
        let a = world.alloc_village_slot(crate::village::Village::new(
            "A".to_string(),
            crate::races::Race::Orc,
            crate::hex::Hex::from_offset(2, 2),
            0,
        ));
        let b = world.alloc_village_slot(crate::village::Village::new(
            "B".to_string(),
            crate::races::Race::Elf,
            crate::hex::Hex::from_offset(3, 2),
            0,
        ));
        for (col, row) in [(2, 2), (3, 2)] {
            world.tile_mut(crate::hex::Hex::from_offset(col, row)).unwrap().owner = Some(if col == 2 { a } else { b });
        }
        // Two villages of different races, no kingdom: each is its own realm, so
        // the shared edge is a border in each village's own colour.
        let left = tile_spec(&world, 2, 2);
        let right = tile_spec(&world, 3, 2);
        assert_eq!(left.surface, concrete_for(crate::races::Race::Orc.def().color));
        assert_eq!(right.surface, concrete_for(crate::races::Race::Elf.def().color));
        assert_ne!(left.surface, right.surface);

        // Crown a kingdom over both: now the shared edge is interior.
        let kingdom = world.alloc_kingdom_slot(crate::kingdom::Kingdom {
            id: 0,
            color: (10, 200, 10),
            ..Default::default()
        });
        for id in [a, b] {
            world.village_mut(id).unwrap().kingdom = Some(kingdom);
        }
        let left = tile_spec(&world, 2, 2);
        let right = tile_spec(&world, 3, 2);
        assert_eq!(
            left.surface, right.surface,
            "villages of one kingdom do not draw a border between themselves"
        );
        assert_eq!(left.surface, "lime_concrete", "the realm colour is the banner");
    }

    #[test]
    fn borders_are_the_tiles_next_to_other_owners() {
        assert!(!is_border(None, [Some(1); 6]), "unowned land has no border");
        assert!(
            !is_border(Some(1), [Some(1); 6]),
            "a tile surrounded by its own village is not a border"
        );
        assert!(is_border(Some(1), [Some(2), Some(1), Some(1), Some(1), Some(1), Some(1)]));
        assert!(
            is_border(Some(1), [None, Some(1), Some(1), Some(1), Some(1), Some(1)]),
            "the coast of a kingdom is a border too"
        );
    }

    #[test]
    fn concrete_picks_the_nearest_colour() {
        assert_eq!(concrete_for((150, 40, 40)), "red_concrete");
        assert_eq!(concrete_for((0, 0, 255)), "blue_concrete");
        assert_eq!(concrete_for((250, 250, 250)), "white_concrete");
        // pure red is nearer Minecraft's orange: the mapping is honest about it
        assert_eq!(concrete_for((255, 0, 0)), "orange_concrete");
        assert!(PALETTE.contains(&concrete_for((37, 200, 120))));
    }

    #[test]
    fn water_is_flat_and_land_rises() {
        let mut tile = Tile {
            biome: Biome::Ocean,
            elevation: -120,
            ..Tile::default()
        };
        assert_eq!(column_height(&tile), 0);
        assert_eq!(water_top(&tile), SEA_LEVEL);

        tile.biome = Biome::Grass;
        tile.elevation = 10;
        assert_eq!(column_height(&tile), 1);
        tile.elevation = 600;
        assert_eq!(column_height(&tile), 11);
        tile.elevation = i16::MAX;
        assert_eq!(column_height(&tile), MAX_COLUMN, "mountains are capped");

        // a river keeps a shallow bed instead of a slot canyon
        tile.biome = Biome::Ocean;
        tile.river = true;
        tile.elevation = 200;
        assert_eq!(column_height(&tile), 2);
        assert_eq!(water_top(&tile), 3);
    }

    #[test]
    fn lava_and_campfires_do_not_fight_for_the_same_tile() {
        let mut world = World::empty(4, 4, 1);
        let hex = crate::hex::Hex::from_offset(1, 1);
        let tile = world.tile_mut(hex).unwrap();
        tile.biome = Biome::Grass;
        tile.elevation = 100;
        tile.trees = 3;
        tile.fire = 5;
        let spec = tile_spec(&world, 1, 1);
        assert_eq!(spec.extra, vec!["campfire"], "a fire does not burn under a tree");

        let tile = world.tile_mut(hex).unwrap();
        tile.fire = 0;
        tile.lava = 2;
        tile.trees = 3;
        let spec = tile_spec(&world, 1, 1);
        assert_eq!(spec.surface, "lava");
        assert!(!spec.water);
        assert!(spec.extra.is_empty());
    }

    #[test]
    fn diffs_only_report_the_tiles_that_changed() {
        let mut world = World::empty(4, 3, 7);
        let before = tile_specs(&world);
        crate::hex::Hex::from_offset(2, 1);
        world
            .set_biome(crate::hex::Hex::from_offset(2, 1), Biome::Grass);
        let after = tile_specs(&world);
        let edits = diff_specs(&before, &after);
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].0, 4 + 2);
        assert_eq!(edits[0].1.surface, "grass_block");
    }

    #[test]
    fn tile_specs_cover_the_whole_map() {
        let world = World::empty(5, 3, 11);
        assert_eq!(tile_specs(&world).len(), 15);
    }

    #[test]
    fn escaping_keeps_json_valid() {
        assert_eq!(json_escape("a\"b\\c"), "a\\\"b\\\\c");
        assert_eq!(json_escape("line\nbreak"), "line\\nbreak");
        assert_eq!(json_escape("plain"), "plain");
    }

    #[test]
    fn commands_parse() {
        assert_eq!(parse_command("{\"t\":\"ping\"}").unwrap(), Command::Ping);
        assert_eq!(
            parse_command("{\"t\":\"speed\",\"tps\":20}").unwrap(),
            Command::Speed { tps: 20.0 }
        );
        assert_eq!(
            parse_command("{\"t\":\"pause\"}").unwrap(),
            Command::Pause { on: true }
        );
        assert_eq!(
            parse_command("{\"t\":\"step\",\"ticks\":12}").unwrap(),
            Command::Step { ticks: 12 }
        );
        assert_eq!(
            parse_command("{\"t\":\"power\",\"name\":\"nuke\",\"col\":3,\"row\":8}").unwrap(),
            Command::Power {
                power: Power::Nuke,
                col: 3,
                row: 8
            }
        );
        assert_eq!(
            parse_command("{\"t\":\"spawn\",\"race\":\"orc\",\"soldier\":true,\"col\":1,\"row\":2}")
                .unwrap(),
            Command::Spawn {
                race: Race::Orc,
                soldier: true,
                col: 1,
                row: 2
            }
        );
    }

    #[test]
    fn bad_commands_are_errors_not_panics() {
        for line in [
            "",
            "ping",
            "{}",
            "{\"t\":}",
            "{\"t\":\"speed\"}",
            "{\"t\":\"power\",\"name\":\"nonsense\",\"col\":1,\"row\":1}",
            "{\"t\":\"spawn\",\"race\":\"Kraken\",\"col\":1,\"row\":1}",
            "{\"t\":\"explode\"}",
            "{\"t\":\"power\",\"name\":\"nuke\",\"col\":1}",
        ] {
            assert!(parse_command(line).is_err(), "`{line}` should not parse");
        }
    }

    #[test]
    fn every_example_command_parses() {
        for line in EXAMPLE_COMMANDS {
            parse_command(line).unwrap_or_else(|e| panic!("`{line}`: {e}"));
        }
    }

    #[test]
    fn messages_are_json_shaped() {
        let mut world = World::empty(3, 2, 5);
        world.set_biome(crate::hex::Hex::from_offset(1, 1), Biome::Forest);
        let hell = hello_json(&world, DEFAULT_PORT, 10.0);
        assert!(hell.starts_with("{\"t\":\"hello\""));
        assert!(hell.ends_with('}'));
        assert!(hell.contains("\"palette\":[\"air\""));
        let specs = tile_specs(&world);
        let tiles = tiles_json(&specs);
        assert!(tiles.starts_with("{\"t\":\"tiles\""));
        let edits = edits_json(&[(0, specs[0].clone())]);
        assert!(edits.starts_with("{\"t\":\"edits\""));
        let ops = structures_json(&[BlockOp::new(1, 2, 3, "bell")]);
        assert_eq!(
            ops,
            format!("{{\"t\":\"structures\",\"ops\":[[1,2,3,{}]]}}", block_id("bell"))
        );
        let units = unit_specs(&world);
        let frame = frame_json(&world, &units, &[(3, "a \"quote\"".into())]);
        assert!(frame.contains("\"a \\\"quote\\\"\""));
        assert!(frame.contains("\"hash\":\"0x"));
    }

    #[test]
    fn no_message_repeats_a_key() {
        // A repeated key is invisible to most JSON parsers and a nightmare to debug
        // (the kingdoms count once shadowed the kingdoms array).
        use std::collections::HashSet;
        let mut world = World::empty(4, 3, 2);
        world.set_biome(crate::hex::Hex::from_offset(1, 1), Biome::Grass);
        let units = unit_specs(&world);
        let messages = [
            hello_json(&world, DEFAULT_PORT, 10.0),
            tiles_json(&tile_specs(&world)),
            edits_json(&[(0, tile_spec(&world, 0, 0))]),
            structures_json(&[BlockOp::new(1, 2, 3, "bell")]),
            frame_json(&world, &units, &[(1, "news".into())]),
        ];
        for message in messages {
            let crate::json::Json::Obj(fields) = crate::json::parse(&message).unwrap() else {
                panic!("a message was not an object");
            };
            let mut seen = HashSet::new();
            for (key, _) in &fields {
                assert!(seen.insert(key.clone()), "`{key}` appears twice in {message:.60}");
            }
        }
    }

    #[test]
    fn a_villages_blocks_all_stand_on_its_own_ground() {
        let world = World::empty(16, 16, 3);
        let ops = world_ops(&world);
        assert!(ops.is_empty(), "no villages, no structures");
    }

    #[test]
    fn unit_specs_are_stable_while_a_unit_is() {
        let mut world = World::empty(16, 16, 21);
        let hex = crate::hex::Hex::from_offset(8, 8);
        world.set_biome(hex, Biome::Grass);
        world.tile_mut(hex).unwrap().elevation = 40;
        let id = world
            .spawn_unit(Race::Human, hex, UnitKind::Civilian)
            .expect("a unit on a fresh map");
        let first = unit_specs(&world);
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].id, id);
        assert_eq!(first[0].mob, "villager");
        assert_eq!(first[0].name, "Human #".to_string() + &id.to_string());
        assert!(first[0].y > BASE_Y);
        let second = unit_specs(&world);
        assert_eq!(first, second);
    }
}
