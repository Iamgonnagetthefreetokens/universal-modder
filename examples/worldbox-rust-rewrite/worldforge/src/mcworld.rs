//! The Minecraft side of the bridge, without Minecraft: parse the wire format,
//! build the block world it describes, and draw it.
//!
//! `worldforge mcview` uses this to prove -- and show -- what the mod will build.
//! It is deliberately the *same* information the Fabric mod receives, so a picture
//! out of here is a picture of the mod's output, drawn by a different renderer.

use std::collections::BTreeMap;

use crate::bridge::{block_name, PALETTE};
use crate::json::Json;
use crate::png::RgbImage;

/// One tile column, as it arrives on the wire.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Column {
    pub surface: u16,
    pub h: i32,
    pub top: i32,
    pub water: bool,
    pub extra: Vec<u16>,
}

impl Column {
    fn from_row(row: &[Json]) -> Result<Column, String> {
        Ok(Column {
            surface: row.first().and_then(|v| v.as_i64()).unwrap_or(0) as u16,
            h: row.get(1).and_then(|v| v.as_i64()).unwrap_or(0) as i32,
            top: row.get(2).and_then(|v| v.as_i64()).unwrap_or(0) as i32,
            water: row.get(3).and_then(|v| v.as_i64()).unwrap_or(0) != 0,
            extra: row
                .iter()
                .skip(4)
                .filter_map(|v| v.as_i64())
                .map(|v| v as u16)
                .collect(),
        })
    }
}

/// A unit, as it arrives on the wire.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnitWire {
    pub id: u32,
    pub mob: u16,
    pub col: i32,
    pub row: i32,
    pub y: i32,
    pub hp: i32,
    pub max_hp: i32,
    pub glow: bool,
    pub name: String,
    pub color: (u8, u8, u8),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VillageWire {
    pub id: u32,
    pub name: String,
    pub race: String,
    pub pop: u32,
    pub col: i32,
    pub row: i32,
    pub kingdom: i32,
    pub loyalty: i32,
    pub capital: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KingdomWire {
    pub id: u32,
    pub name: String,
    pub color: (u8, u8, u8),
    pub pop: u32,
    pub cities: u32,
    pub wars: u32,
    pub king: i32,
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct FrameInfo {
    pub tick: u64,
    pub year: u32,
    pub pop: u32,
    pub animals: u32,
    pub monsters: u32,
    pub villages: u32,
    pub kingdoms: u32,
    pub age: String,
    pub hash: String,
    pub news: Vec<(u32, String)>,
}

/// One wire message.
#[derive(Clone, Debug, PartialEq)]
pub enum Message {
    Hello {
        protocol: u32,
        size: (i32, i32),
        base_y: i32,
        sea_level: i32,
        canvas_top: i32,
        tps: f32,
        seed: u64,
        world: String,
        tag: String,
        palette: Vec<String>,
    },
    Tiles {
        columns: Vec<Column>,
    },
    Edits {
        tiles: Vec<(usize, Column)>,
    },
    Structures {
        ops: Vec<(i32, i32, i32, u16)>,
    },
    Frame {
        info: FrameInfo,
        units: Vec<UnitWire>,
        villages: Vec<VillageWire>,
        kingdoms: Vec<KingdomWire>,
    },
    Pong {
        tick: u64,
    },
    /// A one-line reply: what a command did, or why it could not be done.
    Notice {
        text: String,
    },
    Other(String),
}

fn rgb(text: &str) -> (u8, u8, u8) {
    let hex = text.trim_start_matches('#');
    if hex.len() != 6 {
        return (255, 255, 255);
    }
    let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).unwrap_or(255);
    (byte(0), byte(2), byte(4))
}

/// Parse one line of the wire format.
pub fn parse_message(line: &str) -> Result<Message, String> {
    let value = crate::json::parse(line)?;
    let kind = value
        .get("t")
        .and_then(|v| v.as_str())
        .ok_or("message has no `t`")?;
    let num = |key: &str| value.get(key).and_then(|v| v.as_i64());
    let text = |key: &str| {
        value
            .get(key)
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    };
    match kind {
        "hello" => {
            let palette = value
                .get("palette")
                .and_then(|v| v.as_arr())
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_else(|| PALETTE.iter().map(|s| s.to_string()).collect());
            Ok(Message::Hello {
                protocol: num("protocol").unwrap_or(0) as u32,
                size: (
                    value.get("size").and_then(|v| v.at(0)).and_then(|v| v.as_i64()).unwrap_or(0)
                        as i32,
                    value.get("size").and_then(|v| v.at(1)).and_then(|v| v.as_i64()).unwrap_or(0)
                        as i32,
                ),
                base_y: num("base_y").unwrap_or(64) as i32,
                sea_level: num("sea_level").unwrap_or(3) as i32,
                canvas_top: num("canvas_top").unwrap_or(108) as i32,
                tps: value.get("tps").and_then(|v| v.as_f64()).unwrap_or(10.0) as f32,
                seed: num("seed").unwrap_or(0) as u64,
                world: text("world").unwrap_or_default(),
                tag: text("tag").unwrap_or_default(),
                palette,
            })
        }
        "tiles" => {
            let surfaces: Vec<i64> = arr_ints(&value, "surface")?;
            let heights: Vec<i64> = arr_ints(&value, "h")?;
            let tops: Vec<i64> = arr_ints(&value, "top")?;
            let water: Vec<i64> = arr_ints(&value, "water")?;
            let extras = value
                .get("extra")
                .and_then(|v| v.as_arr())
                .ok_or("missing `extra`")?;
            let mut columns = Vec::with_capacity(surfaces.len());
            for (i, surface) in surfaces.iter().enumerate() {
                let extra = extras[i]
                    .as_arr()
                    .map(|a| a.iter().filter_map(|v| v.as_i64()).map(|v| v as u16).collect())
                    .unwrap_or_default();
                columns.push(Column {
                    surface: *surface as u16,
                    h: *heights.get(i).unwrap_or(&0) as i32,
                    top: *tops.get(i).unwrap_or(&0) as i32,
                    water: *water.get(i).unwrap_or(&0) != 0,
                    extra,
                });
            }
            Ok(Message::Tiles { columns })
        }
        "edits" => {
            let list = value.get("tiles").and_then(|v| v.as_arr()).ok_or("missing `tiles`")?;
            let mut tiles = Vec::with_capacity(list.len() / 2);
            let mut i = 0;
            while i + 1 < list.len() + 1 && i < list.len() {
                let index = list[i].as_i64().ok_or("bad edit index")? as usize;
                let row = list.get(i + 1).and_then(|v| v.as_arr()).ok_or("bad edit row")?;
                tiles.push((index, Column::from_row(row)?));
                i += 2;
            }
            Ok(Message::Edits { tiles })
        }
        "structures" => {
            let list = value.get("ops").and_then(|v| v.as_arr()).ok_or("missing `ops`")?;
            let mut ops = Vec::with_capacity(list.len());
            for row in list {
                let (Some(x), Some(y), Some(z), Some(block)) = (
                    row.at(0).and_then(|v| v.as_i64()),
                    row.at(1).and_then(|v| v.as_i64()),
                    row.at(2).and_then(|v| v.as_i64()),
                    row.at(3).and_then(|v| v.as_i64()),
                ) else {
                    return Err("bad structure op".into());
                };
                ops.push((x as i32, y as i32, z as i32, block as u16));
            }
            Ok(Message::Structures { ops })
        }
        "frame" => {
            let info = FrameInfo {
                tick: num("tick").unwrap_or(0) as u64,
                year: num("year").unwrap_or(0) as u32,
                pop: num("pop").unwrap_or(0) as u32,
                animals: num("animals").unwrap_or(0) as u32,
                monsters: num("monsters").unwrap_or(0) as u32,
                villages: num("village_count").unwrap_or(0) as u32,
                kingdoms: num("kingdom_count").unwrap_or(0) as u32,
                age: text("age").unwrap_or_default(),
                hash: text("hash").unwrap_or_default(),
                news: value
                    .get("news")
                    .and_then(|v| v.as_arr())
                    .map(|a| {
                        a.iter()
                            .filter_map(|row| {
                                Some((row.at(0)?.as_i64()? as u32, row.at(1)?.as_str()?.to_string()))
                            })
                            .collect()
                    })
                    .unwrap_or_default(),
            };
            let mut units = Vec::new();
            for row in value.get("units").and_then(|v| v.as_arr()).unwrap_or(&[]) {
                let (Some(id), Some(mob), Some(col), Some(row_z), Some(y)) = (
                    row.at(0).and_then(|v| v.as_i64()),
                    row.at(1).and_then(|v| v.as_i64()),
                    row.at(2).and_then(|v| v.as_i64()),
                    row.at(3).and_then(|v| v.as_i64()),
                    row.at(4).and_then(|v| v.as_i64()),
                ) else {
                    continue;
                };
                units.push(UnitWire {
                    id: id as u32,
                    mob: mob as u16,
                    col: col as i32,
                    row: row_z as i32,
                    y: y as i32,
                    hp: row.at(5).and_then(|v| v.as_i64()).unwrap_or(1) as i32,
                    max_hp: row.at(6).and_then(|v| v.as_i64()).unwrap_or(1) as i32,
                    glow: row.at(7).and_then(|v| v.as_i64()).unwrap_or(0) != 0,
                    name: row.at(8).and_then(|v| v.as_str()).unwrap_or("").to_string(),
                    color: rgb(row.at(9).and_then(|v| v.as_str()).unwrap_or("#ffffff")),
                });
            }
            let mut villages = Vec::new();
            for row in value.get("villages").and_then(|v| v.as_arr()).unwrap_or(&[]) {
                let (Some(id), Some(col), Some(row_z)) = (
                    row.at(0).and_then(|v| v.as_i64()),
                    row.at(4).and_then(|v| v.as_i64()),
                    row.at(5).and_then(|v| v.as_i64()),
                ) else {
                    continue;
                };
                villages.push(VillageWire {
                    id: id as u32,
                    name: row.at(1).and_then(|v| v.as_str()).unwrap_or("").to_string(),
                    race: row.at(2).and_then(|v| v.as_str()).unwrap_or("").to_string(),
                    pop: row.at(3).and_then(|v| v.as_i64()).unwrap_or(0) as u32,
                    col: col as i32,
                    row: row_z as i32,
                    kingdom: row.at(6).and_then(|v| v.as_i64()).unwrap_or(-1) as i32,
                    loyalty: row.at(7).and_then(|v| v.as_i64()).unwrap_or(0) as i32,
                    capital: row.at(8).and_then(|v| v.as_i64()).unwrap_or(0) != 0,
                });
            }
            let mut kingdoms = Vec::new();
            for row in value.get("kingdoms").and_then(|v| v.as_arr()).unwrap_or(&[]) {
                let Some(id) = row.at(0).and_then(|v| v.as_i64()) else {
                    continue;
                };
                kingdoms.push(KingdomWire {
                    id: id as u32,
                    name: row.at(1).and_then(|v| v.as_str()).unwrap_or("").to_string(),
                    color: rgb(row.at(2).and_then(|v| v.as_str()).unwrap_or("#ffffff")),
                    pop: row.at(3).and_then(|v| v.as_i64()).unwrap_or(0) as u32,
                    cities: row.at(4).and_then(|v| v.as_i64()).unwrap_or(0) as u32,
                    wars: row.at(5).and_then(|v| v.as_i64()).unwrap_or(0) as u32,
                    king: row.at(6).and_then(|v| v.as_i64()).unwrap_or(-1) as i32,
                });
            }
            Ok(Message::Frame {
                info,
                units,
                villages,
                kingdoms,
            })
        }
        "pong" => Ok(Message::Pong {
            tick: num("tick").unwrap_or(0) as u64,
        }),
        "notice" => Ok(Message::Notice {
            text: text("text").unwrap_or_default(),
        }),
        other => Ok(Message::Other(other.to_string())),
    }
}

fn arr_ints(value: &Json, key: &str) -> Result<Vec<i64>, String> {
    value
        .get(key)
        .and_then(|v| v.as_arr())
        .ok_or_else(|| format!("missing `{key}`"))?
        .iter()
        .map(|v| v.as_i64().ok_or_else(|| format!("`{key}` holds a non-number")))
        .collect()
}

/// What applying a message changed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Applied {
    pub blocks: u64,
    pub columns: u64,
    pub units: usize,
    pub cleared: u64,
}

/// The block world the wire describes: a small voxel canvas plus the units in it.
pub struct BlockWorld {
    pub width: i32,
    pub height: i32,
    pub y0: i32,
    pub y1: i32,
    pub base_y: i32,
    pub sea_level: i32,
    pub canvas_top: i32,
    pub palette: Vec<String>,
    vox: Vec<u16>,
    pub units: BTreeMap<u32, UnitWire>,
    pub villages: Vec<VillageWire>,
    pub kingdoms: Vec<KingdomWire>,
    pub last: Option<FrameInfo>,
    pub messages: u64,
    pub blocks_set: u64,
    pub blocks_cleared: u64,
}

impl BlockWorld {
    pub fn new(width: i32, height: i32, base_y: i32, sea_level: i32, canvas_top: i32) -> Self {
        let y0 = base_y - 1;
        let y1 = canvas_top + 8;
        BlockWorld {
            width,
            height,
            y0,
            y1,
            base_y,
            sea_level,
            canvas_top,
            palette: PALETTE.iter().map(|s| s.to_string()).collect(),
            vox: vec![0; (width * height * (y1 - y0)) as usize],
            units: BTreeMap::new(),
            villages: Vec::new(),
            kingdoms: Vec::new(),
            last: None,
            messages: 0,
            blocks_set: 0,
            blocks_cleared: 0,
        }
    }

    /// The block name for a palette index.
    pub fn name(&self, id: u16) -> &str {
        self.palette
            .get(id as usize)
            .map(|s| s.as_str())
            .unwrap_or_else(|| block_name(id))
    }

    fn index(&self, x: i32, y: i32, z: i32) -> Option<usize> {
        if x < 0 || z < 0 || x >= self.width || z >= self.height || y < self.y0 || y >= self.y1 {
            return None;
        }
        Some(((x * (self.y1 - self.y0)) + (y - self.y0)) as usize * self.height as usize + z as usize)
    }

    pub fn set(&mut self, x: i32, y: i32, z: i32, id: u16) {
        if let Some(i) = self.index(x, y, z) {
            let was = self.vox[i];
            if was != id {
                self.vox[i] = id;
                if id == 0 {
                    self.blocks_cleared += 1;
                } else {
                    self.blocks_set += 1;
                }
            }
        }
    }

    pub fn get(&self, x: i32, y: i32, z: i32) -> u16 {
        self.index(x, y, z).map(|i| self.vox[i]).unwrap_or(0)
    }

    /// The highest non-air block in a column, if any.
    pub fn top_of(&self, x: i32, z: i32) -> Option<(i32, u16)> {
        for y in (self.y0..self.y1).rev() {
            let block = self.get(x, y, z);
            if block != 0 {
                return Some((y, block));
            }
        }
        None
    }

    /// Build one tile column from the wire description.
    pub fn build_column(&mut self, col: i32, row: i32, spec: &Column) -> Applied {
        let mut applied = Applied::default();
        for y in self.y0..self.y1 {
            if self.get(col, y, row) != 0 {
                self.set(col, y, row, 0);
                applied.cleared += 1;
            }
        }
        let base = self.base_y;
        let top = spec.h.max(0);
        for h in 0..=top {
            let block = if h == top {
                spec.surface
            } else if h + 1 == top {
                // one block of soil under the surface, stone under that
                match self.name(spec.surface) {
                    "sand" | "red_sand" => block_id_of("sand"),
                    "gravel" | "stone" => block_id_of("stone"),
                    _ => block_id_of("dirt"),
                }
            } else {
                block_id_of("stone")
            };
            self.set(col, base + h, row, block);
            applied.blocks += 1;
        }
        if spec.water {
            for h in (top + 1)..=spec.top.max(top) {
                self.set(col, base + h, row, block_id_of("water"));
                applied.blocks += 1;
            }
        }
        for (i, block) in spec.extra.iter().enumerate() {
            let y = base + spec.top.max(top) + 1 + i as i32;
            self.set(col, y, row, *block);
            applied.blocks += 1;
        }
        applied.columns += 1;
        applied
    }

    /// Apply one wire message.
    pub fn apply(&mut self, message: &Message) -> Applied {
        self.messages += 1;
        let mut applied = Applied::default();
        match message {
            Message::Hello {
                size,
                base_y,
                sea_level,
                canvas_top,
                palette,
                ..
            } => {
                if *size != (self.width, self.height) {
                    // a new map: start over
                    self.width = size.0;
                    self.height = size.1;
                    self.base_y = *base_y;
                    self.sea_level = *sea_level;
                    self.canvas_top = *canvas_top;
                    self.y0 = base_y - 1;
                    self.y1 = canvas_top + 8;
                    self.vox = vec![0; (self.width * self.height * (self.y1 - self.y0)) as usize];
                    self.units.clear();
                    self.villages.clear();
                    self.kingdoms.clear();
                }
                self.palette = palette.clone();
            }
            Message::Tiles { columns } => {
                for (i, spec) in columns.iter().enumerate() {
                    let col = (i as i32) % self.width;
                    let row = (i as i32) / self.width;
                    applied += self.build_column(col, row, spec);
                }
            }
            Message::Edits { tiles } => {
                for (index, spec) in tiles {
                    let col = (*index as i32) % self.width;
                    let row = (*index as i32) / self.width;
                    applied += self.build_column(col, row, spec);
                }
            }
            Message::Structures { ops } => {
                for (x, y, z, block) in ops {
                    self.set(*x, *y, *z, *block);
                    applied.blocks += 1;
                }
            }
            Message::Frame {
                info,
                units,
                villages,
                kingdoms,
            } => {
                self.units.clear();
                for u in units {
                    self.units.insert(u.id, u.clone());
                }
                self.villages = villages.clone();
                self.kingdoms = kingdoms.clone();
                self.last = Some(info.clone());
                applied.units = units.len();
            }
            Message::Pong { .. } | Message::Notice { .. } | Message::Other(_) => {}
        }
        applied
    }
}

/// Palette index for a block we know is in the table.
fn block_id_of(name: &str) -> u16 {
    crate::bridge::block_id(name)
}

impl std::ops::AddAssign for Applied {
    fn add_assign(&mut self, other: Self) {
        self.blocks += other.blocks;
        self.columns += other.columns;
        self.units += other.units;
        self.cleared += other.cleared;
    }
}

// ---------------------------------------------------------------------------
// Drawing
// ---------------------------------------------------------------------------

/// The colour of a block, from the top: Minecraft's textures, roughly.
pub fn block_color(name: &str) -> (u8, u8, u8) {
    match name {
        "water" => (58, 100, 186),
        "ice" => (160, 205, 240),
        "packed_ice" => (145, 180, 220),
        "blue_ice" => (110, 160, 230),
        "sand" => (219, 205, 150),
        "grass_block" => (106, 170, 80),
        "coarse_dirt" => (150, 120, 80),
        "moss_block" => (90, 140, 60),
        "podzol" => (110, 80, 40),
        "rooted_dirt" => (140, 110, 80),
        "mud" => (90, 80, 75),
        "mycelium" => (150, 140, 150),
        "gravel" => (140, 138, 132),
        "snow_block" => (240, 245, 250),
        "stone" => (130, 130, 130),
        "dirt" => (140, 110, 75),
        "dirt_path" => (150, 120, 80),
        "smooth_stone" => (160, 160, 155),
        "sculk" => (30, 50, 60),
        "netherrack" => (110, 45, 45),
        "amethyst_block" => (150, 110, 190),
        "purpur_block" => (170, 120, 170),
        "pink_concrete" => (230, 150, 175),
        "brown_terracotta" => (120, 75, 55),
        "basalt" => (70, 70, 75),
        "magma_block" => (150, 70, 30),
        "lava" => (235, 110, 30),
        "oak_log" => (110, 85, 50),
        "oak_leaves" => (70, 120, 50),
        "jungle_log" => (95, 80, 45),
        "jungle_leaves" => (50, 110, 40),
        "spruce_log" => (85, 65, 40),
        "spruce_leaves" => (45, 85, 60),
        "acacia_log" => (110, 90, 60),
        "acacia_leaves" => (110, 150, 50),
        "mangrove_leaves" => (60, 120, 70),
        "mushroom_stem" => (200, 195, 180),
        "red_mushroom_block" => (200, 60, 60),
        "cactus" => (60, 120, 50),
        "crimson_stem" => (110, 50, 70),
        "nether_wart_block" => (140, 20, 25),
        "stripped_crimson_stem" => (140, 80, 95),
        "shroomlight" => (240, 150, 60),
        "cherry_leaves" => (240, 180, 200),
        "amethyst_cluster" => (190, 150, 230),
        "dead_bush" => (140, 110, 60),
        "cobblestone" => (120, 120, 120),
        "campfire" => (150, 90, 40),
        "white_concrete" => (207, 213, 214),
        "orange_concrete" => (240, 118, 19),
        "magenta_concrete" => (189, 68, 179),
        "light_blue_concrete" => (58, 175, 217),
        "yellow_concrete" => (248, 198, 39),
        "lime_concrete" => (112, 185, 25),
        "pink_concrete_dye" => (237, 141, 172),
        "gray_concrete" => (62, 68, 71),
        "light_gray_concrete" => (142, 142, 134),
        "cyan_concrete" => (21, 137, 145),
        "purple_concrete" => (121, 42, 172),
        "blue_concrete" => (53, 57, 157),
        "brown_concrete" => (114, 71, 40),
        "green_concrete" => (84, 109, 27),
        "red_concrete" => (142, 32, 32),
        "black_concrete" => (20, 21, 25),
        "oak_planks" => (160, 125, 75),
        "dark_oak_planks" => (70, 50, 30),
        "stone_bricks" => (120, 120, 118),
        "stone_brick_wall" => (118, 118, 116),
        "bricks" => (150, 85, 70),
        "farmland" => (140, 110, 70),
        "wheat" => (200, 190, 90),
        "rail" => (170, 170, 170),
        "oak_fence" => (160, 125, 75),
        "barrel" => (130, 100, 60),
        "quartz_block" => (235, 232, 225),
        "gold_block" => (250, 220, 80),
        "end_rod" => (240, 240, 230),
        "lantern" => (200, 150, 60),
        "bell" => (240, 200, 90),
        "scaffolding" => (200, 170, 110),
        "torch" => (250, 200, 80),
        "glass" => (200, 230, 240),
        "air" => (18, 20, 26),
        _ => (200, 200, 200),
    }
}

/// A cheap deterministic hash, for texture noise.
fn sprinkle(x: i32, z: i32, y: i32, salt: u32) -> f32 {
    let mut h = (x as u32)
        .wrapping_mul(0x9e37_79b9)
        ^ (z as u32).wrapping_mul(0x85eb_ca6b)
        ^ (y as u32).wrapping_mul(0xc2b2_ae35)
        ^ salt;
    h ^= h >> 13;
    h = h.wrapping_mul(0x27d4_eb2f);
    h ^= h >> 15;
    (h & 0xff) as f32 / 255.0
}

fn shade(color: (u8, u8, u8), factor: f32) -> (u8, u8, u8) {
    let f = |c: u8| (c as f32 * factor).clamp(0.0, 255.0) as u8;
    (f(color.0), f(color.1), f(color.2))
}

/// How the isometric picture is framed.
#[derive(Clone, Copy, Debug)]
pub struct IsoOpts {
    /// Half-width of a tile in pixels; the diamond is `2 * scale` wide.
    pub scale: i32,
    /// Crop to this tile rectangle `(col, row, width, height)`.
    pub region: Option<(i32, i32, i32, i32)>,
    /// Draw unit markers.
    pub units: bool,
    /// Draw a one-pixel grid outline on every tile.
    pub grid: bool,
}

impl Default for IsoOpts {
    fn default() -> Self {
        IsoOpts {
            scale: 6,
            region: None,
            units: true,
            grid: false,
        }
    }
}

impl BlockWorld {
    /// Screen position of the *centre* of a tile's top face.
    fn screen(&self, col: i32, row: i32, top_y: i32, s: i32, ox: i32, oy: i32) -> (i32, i32) {
        let block_px = (s / 2).max(1);
        let x = (col - row) * s + ox;
        let y = (col + row) * (s / 2) + oy - (top_y - self.base_y) * block_px;
        (x, y)
    }

    /// Draw the world as an isometric voxel picture.
    ///
    /// The picture is framed around whatever is being drawn -- the whole map, or
    /// the tile rectangle in `opts.region` -- so a zoom is a zoom and not a small
    /// figure in the middle of a big empty canvas.
    pub fn render_iso(&self, opts: &IsoOpts) -> RgbImage {
        let s = opts.scale.clamp(2, 32);
        let block_px = (s / 2).max(1);
        let (min_col, min_row, w, h) = match opts.region {
            Some((c, r, w, h)) => (c, r, w, h),
            None => (0, 0, self.width, self.height),
        };
        let max_col = (min_col + w).min(self.width);
        let max_row = (min_row + h).min(self.height);

        // Collect the tiles to draw, back to front.
        let mut drawn: Vec<(i32, i32, i32, u16)> = Vec::new();
        for row in min_row..max_row {
            for col in min_col..max_col {
                if let Some((top_y, block)) = self.top_of(col, row) {
                    drawn.push((col, row, top_y, block));
                }
            }
        }
        drawn.sort_by_key(|(col, row, _, _)| (col + row, *col));

        // Frame: work out where the drawing lands, then translate it to the corner.
        let x_of = |col: i32, row: i32| (col - row) * s;
        let y_of = |col: i32, row: i32, top: i32| (col + row) * (s / 2) - (top - self.base_y) * block_px;
        let (mut sx0, mut sy0, mut sx1, mut sy1) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
        for (col, row, top, _) in &drawn {
            let x = x_of(*col, *row);
            let y = y_of(*col, *row, *top);
            sx0 = sx0.min(x - s);
            sx1 = sx1.max(x + s);
            sy0 = sy0.min(y - s / 2);
            // the side face of this column only reaches down to the ocean floor
            sy1 = sy1.max(y + (top - self.y0).max(0) * block_px + s / 2);
        }
        if drawn.is_empty() {
            return RgbImage::filled(32, 32, (14, 16, 22));
        }
        let margin = 6;
        let ox = margin - sx0;
        let oy = margin - sy0;
        let img_w = (sx1 - sx0 + margin * 2).max(8) as u32;
        let img_h = (sy1 - sy0 + margin * 2).max(8) as u32;
        let mut img = RgbImage::filled(img_w, img_h, (14, 16, 22));

        for (col, row, top_y, top_block) in drawn {
            let name = self.name(top_block);
            let base = block_color(name);
            let (cx, cy) = self.screen(col, row, top_y, s, ox, oy);

            // The column's sides go down to the ocean floor, so the front tiles
            // hide the back of the ones behind them.
            let depth = ((top_y - self.y0).max(0)) * block_px;
            let left = shade(base, 0.74);
            let right = shade(base, 0.52);
            for dy in 0..=depth {
                let ty = cy + dy;
                for dx in -s..=0 {
                    let px = (cx + dx + s) as u32;
                    let py = ty as u32;
                    if px < img_w && py < img_h {
                        let fade = 1.0 - 0.35 * (dy as f32 / (depth as f32 + 1.0));
                        img.set(px, py, shade(left, fade));
                    }
                }
                for dx in 0..=s {
                    let px = (cx + dx) as u32;
                    let py = ty as u32;
                    if px < img_w && py < img_h {
                        let fade = 1.0 - 0.35 * (dy as f32 / (depth as f32 + 1.0));
                        img.set(px, py, shade(right, fade));
                    }
                }
            }

            // The top face: a diamond.
            let noise = 0.94 + sprinkle(col, row, top_y, 7) * 0.12;
            for dy in -s / 2..=s / 2 {
                let half = s - (dy.abs() * 2).max(0) / 2;
                for dx in -half..=half {
                    let px = (cx + dx) as u32;
                    let py = (cy + dy) as u32;
                    if px < img_w && py < img_h {
                        img.set(px, py, shade(base, noise));
                    }
                }
            }
            if opts.grid {
                for dx in -s..=s {
                    let px = (cx + dx) as u32;
                    if px < img_w {
                        let py = (cy + dx.abs() / 2) as u32;
                        if py < img_h {
                            img.set(px, py, shade(base, 0.8));
                        }
                    }
                }
            }
        }

        if opts.units {
            let mut units: Vec<&UnitWire> = self.units.values().collect();
            units.sort_by_key(|u| (u.col + u.row, u.col));
            for u in units {
                let (top_y, _) = self.top_of(u.col, u.row).unwrap_or((self.base_y, 0));
                let (cx, cy) = self.screen(u.col, u.row, top_y.max(u.y - self.base_y), s, ox, oy);
                let r = if u.glow { (s / 2).max(2) } else { (s / 3).max(2) };
                let (color, edge) = if u.glow {
                    (u.color, (255, 255, 255))
                } else {
                    (u.color, shade(u.color, 0.5))
                };
                for dy in -r..=r {
                    for dx in -r..=r {
                        let px = (cx + dx) as u32;
                        let py = (cy - s / 2 + dy) as u32;
                        if px < img_w && py < img_h {
                            let on_edge = dx.abs() == r || dy.abs() == r;
                            img.set(px, py, if on_edge { edge } else { color });
                        }
                    }
                }
            }
        }
        img
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bridge;

    #[test]
    fn hello_round_trips() {
        let mut world = crate::world::World::empty(8, 6, 4);
        world.set_biome(crate::hex::Hex::from_offset(2, 2), crate::terrain::Biome::Grass);
        let line = bridge::hello_json(&world, 25607, 10.0);
        match parse_message(&line).unwrap() {
            Message::Hello {
                protocol,
                size,
                base_y,
                palette,
                tag,
                ..
            } => {
                assert_eq!(protocol, bridge::PROTOCOL);
                assert_eq!(size, (8, 6));
                assert_eq!(base_y, bridge::BASE_Y);
                assert_eq!(palette.len(), bridge::PALETTE.len());
                assert_eq!(tag, bridge::ENTITY_TAG);
            }
            other => panic!("expected hello, got {other:?}"),
        }
    }

    #[test]
    fn tiles_and_edits_round_trip() {
        let mut world = crate::world::World::empty(8, 6, 4);
        world.set_biome(crate::hex::Hex::from_offset(2, 2), crate::terrain::Biome::Grass);
        let specs = bridge::tile_specs(&world);
        let line = bridge::tiles_json(&specs);
        let Message::Tiles { columns } = parse_message(&line).unwrap() else {
            panic!("expected tiles");
        };
        assert_eq!(columns.len(), 48);
        assert_eq!(columns[2 * 8 + 2].surface, bridge::block_id("grass_block"));

        let edits = bridge::diff_specs(&specs, &specs);
        assert!(edits.is_empty());
        let line = bridge::edits_json(&[(3, specs[3].clone())]);
        let Message::Edits { tiles } = parse_message(&line).unwrap() else {
            panic!("expected edits");
        };
        assert_eq!(tiles.len(), 1);
        assert_eq!(tiles[0].0, 3);
    }

    #[test]
    fn frames_round_trip() {
        let mut world = crate::world::World::empty(8, 6, 4);
        world.set_biome(crate::hex::Hex::from_offset(1, 1), crate::terrain::Biome::Grass);
        world.tile_mut(crate::hex::Hex::from_offset(1, 1)).unwrap().elevation = 30;
        world
            .spawn_unit(
                crate::races::Race::Human,
                crate::hex::Hex::from_offset(1, 1),
                crate::units::UnitKind::Civilian,
            )
            .unwrap();
        let line = bridge::frame_json(&world, &bridge::unit_specs(&world), &[(1, "hello".into())]);
        let Message::Frame { info, units, .. } = parse_message(&line).unwrap() else {
            panic!("expected a frame");
        };
        assert_eq!(info.tick, world.tick);
        assert_eq!(units.len(), 1);
        assert_eq!(units[0].name, "Human #0");
        assert_eq!(info.news, vec![(1, "hello".to_string())]);
        assert!(info.hash.starts_with("0x"));
    }

    #[test]
    fn structures_round_trip() {
        let ops = vec![(1, 70, 2, bridge::block_id("bell")), (3, 68, 4, bridge::block_id("water"))];
        let line = bridge::structures_json(
            &ops.iter()
                .map(|(x, y, z, b)| bridge::BlockOp {
                    x: *x,
                    y: *y,
                    z: *z,
                    block: crate::bridge::block_name(*b),
                })
                .collect::<Vec<_>>(),
        );
        let Message::Structures { ops: back } = parse_message(&line).unwrap() else {
            panic!("expected structures");
        };
        assert_eq!(back, ops);
    }

    #[test]
    fn applying_messages_builds_the_ground() {
        let mut world = crate::world::World::empty(6, 4, 9);
        for hex in [
            crate::hex::Hex::from_offset(1, 1),
            crate::hex::Hex::from_offset(2, 1),
        ] {
            world.set_biome(hex, crate::terrain::Biome::Grass);
            world.tile_mut(hex).unwrap().elevation = 120;
            world.tile_mut(hex).unwrap().trees = 2;
        }
        let specs = bridge::tile_specs(&world);
        let mut blocks = BlockWorld::new(6, 4, bridge::BASE_Y, bridge::SEA_LEVEL, bridge::CANVAS_TOP);
        let applied = blocks.apply(&Message::Tiles { columns: vec![] });
        assert_eq!(applied.blocks, 0);
        let Message::Tiles { columns } = parse_message(&bridge::tiles_json(&specs)).unwrap() else {
            panic!("expected tiles");
        };
        let applied = blocks.apply(&Message::Tiles { columns });
        assert!(applied.blocks > 0);

        let index = 6 + 1;
        let (top_y, block) = blocks.top_of(1, 1).expect("a column was built");
        assert_eq!(blocks.name(block), "oak_leaves");
        assert_eq!(top_y, bridge::BASE_Y + specs[index].top + 2);

        // an edit replaces the column in place
        let Message::Edits { tiles } = parse_message(&bridge::edits_json(&[(
            index,
            crate::bridge::TileSpec {
                surface: "lava",
                h: 3,
                water: false,
                top: 3,
                extra: vec![],
            },
        )]))
        .unwrap() else {
            panic!("expected edits");
        };
        blocks.apply(&Message::Edits { tiles });
        let (top_y, block) = blocks.top_of(1, 1).unwrap();
        assert_eq!(blocks.name(block), "lava");
        assert_eq!(top_y, bridge::BASE_Y + 3);
    }

    #[test]
    fn rendering_produces_a_picture() {
        let mut world = crate::world::World::empty(12, 8, 3);
        for hex in [
            crate::hex::Hex::from_offset(4, 4),
            crate::hex::Hex::from_offset(5, 4),
        ] {
            world.set_biome(hex, crate::terrain::Biome::Grass);
            world.tile_mut(hex).unwrap().elevation = 200;
        }
        world
            .spawn_unit(
                crate::races::Race::Orc,
                crate::hex::Hex::from_offset(4, 4),
                crate::units::UnitKind::Soldier,
            )
            .unwrap();
        let specs = bridge::tile_specs(&world);
        let Message::Tiles { columns } = parse_message(&bridge::tiles_json(&specs)).unwrap() else {
            panic!("expected tiles");
        };
        let mut blocks = BlockWorld::new(12, 8, bridge::BASE_Y, bridge::SEA_LEVEL, bridge::CANVAS_TOP);
        blocks.apply(&Message::Tiles { columns });
        let frame = bridge::frame_json(&world, &bridge::unit_specs(&world), &[]);
        blocks.apply(&parse_message(&frame).unwrap());
        assert_eq!(blocks.units.len(), 1);

        let img = blocks.render_iso(&IsoOpts {
            scale: 6,
            ..IsoOpts::default()
        });
        assert!(img.width > 100 && img.height > 40);
        // the picture is not one flat colour
        let first = img.get(0, 0);
        let mut different = 0;
        for x in 0..img.width {
            for y in 0..img.height {
                if img.get(x, y) != first {
                    different += 1;
                }
            }
        }
        assert!(different > 1000, "the render is almost empty ({different} pixels)");
    }

    #[test]
    fn jpeg_like_garbage_does_not_panic() {
        for line in ["", "{}", "{\"t\":\"nonsense\"}", "{\"t\":\"tiles\"}", "[1,2,3]"] {
            match parse_message(line) {
                Ok(_) | Err(_) => {}
            }
        }
    }
}
