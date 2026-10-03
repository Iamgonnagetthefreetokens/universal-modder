//! Terrain: biomes and the per-tile state.
//!
//! A WorldBox-style world has a ground layer (water/land/elevation) and a biome
//! layer on top of it, plus scattered resources (trees, stone, ore), fire, lava
//! and a territory owner. All of that lives in [`Tile`].

use std::fmt;

/// The biome layer. Kept as a flat enum so it can be indexed by `as usize`,
/// saved as one byte, and switched on exhaustively.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum Biome {
    #[default]
    Ocean,
    Shallow,
    Ice,
    Beach,
    Grass,
    Savanna,
    Desert,
    Jungle,
    Forest,
    Taiga,
    Swamp,
    Mushroom,
    Tundra,
    Snow,
    Permafrost,
    Mountain,
    Corrupted,
    Infernal,
    Crystal,
    Enchanted,
    Candy,
    Wasteland,
    Ash,
    Lava,
}

impl Biome {
    /// Every biome, in enum order (index == discriminant).
    pub const ALL: [Biome; 24] = [
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

    /// Numeric id used by the save format.
    pub fn id(self) -> u8 {
        self as u8
    }

    /// Inverse of [`Biome::id`]; unknown ids fall back to `Grass`.
    pub fn from_id(id: u8) -> Biome {
        *Biome::ALL.get(id as usize).unwrap_or(&Biome::Grass)
    }

    pub fn name(self) -> &'static str {
        match self {
            Biome::Ocean => "ocean",
            Biome::Shallow => "shallow water",
            Biome::Ice => "ice",
            Biome::Beach => "beach",
            Biome::Grass => "grassland",
            Biome::Savanna => "savanna",
            Biome::Desert => "desert",
            Biome::Jungle => "jungle",
            Biome::Forest => "forest",
            Biome::Taiga => "taiga",
            Biome::Swamp => "swamp",
            Biome::Mushroom => "mushroom",
            Biome::Tundra => "tundra",
            Biome::Snow => "snow",
            Biome::Permafrost => "permafrost",
            Biome::Mountain => "mountains",
            Biome::Corrupted => "corrupted land",
            Biome::Infernal => "infernal land",
            Biome::Crystal => "crystal land",
            Biome::Enchanted => "enchanted land",
            Biome::Candy => "candy land",
            Biome::Wasteland => "wasteland",
            Biome::Ash => "ashland",
            Biome::Lava => "lava",
        }
    }

    /// Water tiles. `Ice` is frozen water: walkable, but melts in warm eras.
    pub fn is_water(self) -> bool {
        matches!(self, Biome::Ocean | Biome::Shallow | Biome::Ice)
    }

    /// Solid land (everything that is not water, lava or bare mountain rock).
    pub fn is_land(self) -> bool {
        !self.is_water() && !matches!(self, Biome::Lava)
    }

    /// Can a unit stand here? Mountains and lava need special traits.
    pub fn walkable(self) -> bool {
        self.is_land() && self != Biome::Mountain
    }

    /// Can a boat float here?
    pub fn is_sea(self) -> bool {
        matches!(self, Biome::Ocean | Biome::Shallow | Biome::Ice)
    }

    /// Biomes that burn. Rock, sand, ice and ash do not.
    pub fn can_burn(self) -> bool {
        matches!(
            self,
            Biome::Forest
                | Biome::Jungle
                | Biome::Taiga
                | Biome::Swamp
                | Biome::Mushroom
                | Biome::Grass
                | Biome::Savanna
                | Biome::Corrupted
                | Biome::Enchanted
                | Biome::Candy
        )
    }

    /// How many trees this biome can carry (density cap, 0..=3).
    pub fn tree_capacity(self) -> u8 {
        match self {
            Biome::Forest => 3,
            Biome::Jungle => 3,
            Biome::Taiga => 3,
            Biome::Swamp => 2,
            Biome::Mushroom => 2,
            Biome::Grass => 1,
            Biome::Savanna => 1,
            Biome::Enchanted => 2,
            Biome::Candy => 1,
            Biome::Corrupted => 1,
            _ => 0,
        }
    }

    /// Farmland quality, 0..=100. Villages need fertile soil to grow food.
    pub fn fertility(self) -> u8 {
        match self {
            Biome::Grass => 90,
            Biome::Savanna => 60,
            Biome::Swamp => 70,
            Biome::Beach => 40,
            Biome::Jungle => 50,
            Biome::Forest => 45,
            Biome::Taiga => 40,
            Biome::Tundra => 25,
            Biome::Mushroom => 50,
            Biome::Enchanted => 65,
            Biome::Candy => 70,
            Biome::Corrupted => 15,
            Biome::Infernal => 5,
            Biome::Ash => 20,
            Biome::Wasteland => 15,
            Biome::Snow => 5,
            Biome::Permafrost => 5,
            Biome::Crystal => 10,
            Biome::Mountain => 5,
            Biome::Desert => 10,
            Biome::Lava => 0,
            _ => 0,
        }
    }

    /// What a tile of this biome turns into when the biome spreads.
    /// `None` means "does not spread".
    pub fn spread_target(self) -> Option<Biome> {
        match self {
            Biome::Forest => Some(Biome::Forest),
            Biome::Jungle => Some(Biome::Jungle),
            Biome::Taiga => Some(Biome::Taiga),
            Biome::Swamp => Some(Biome::Swamp),
            Biome::Mushroom => Some(Biome::Mushroom),
            Biome::Desert => Some(Biome::Desert),
            Biome::Corrupted => Some(Biome::Corrupted),
            Biome::Infernal => Some(Biome::Infernal),
            Biome::Crystal => Some(Biome::Crystal),
            Biome::Enchanted => Some(Biome::Enchanted),
            Biome::Candy => Some(Biome::Candy),
            Biome::Permafrost => Some(Biome::Permafrost),
            Biome::Wasteland => Some(Biome::Wasteland),
            Biome::Ash => Some(Biome::Ash),
            _ => None,
        }
    }

    /// Base RGB used by the PNG renderer.
    pub fn color(self) -> (u8, u8, u8) {
        match self {
            Biome::Ocean => (22, 56, 112),
            Biome::Shallow => (46, 104, 170),
            Biome::Ice => (198, 224, 242),
            Biome::Beach => (222, 206, 150),
            Biome::Grass => (104, 158, 74),
            Biome::Savanna => (170, 168, 86),
            Biome::Desert => (226, 208, 140),
            Biome::Jungle => (44, 110, 58),
            Biome::Forest => (62, 124, 64),
            Biome::Taiga => (74, 110, 88),
            Biome::Swamp => (86, 102, 60),
            Biome::Mushroom => (146, 116, 152),
            Biome::Tundra => (150, 150, 130),
            Biome::Snow => (234, 238, 242),
            Biome::Permafrost => (186, 212, 224),
            Biome::Mountain => (124, 116, 108),
            Biome::Corrupted => (88, 60, 100),
            Biome::Infernal => (120, 40, 30),
            Biome::Crystal => (118, 178, 200),
            Biome::Enchanted => (140, 110, 200),
            Biome::Candy => (230, 150, 190),
            Biome::Wasteland => (110, 100, 84),
            Biome::Ash => (70, 66, 64),
            Biome::Lava => (220, 90, 30),
        }
    }

    /// Single ASCII glyph for the text renderer.
    pub fn glyph(self) -> char {
        match self {
            Biome::Ocean => ' ',
            Biome::Shallow => '~',
            Biome::Ice => '=',
            Biome::Beach => '.',
            Biome::Grass => ',',
            Biome::Savanna => ';',
            Biome::Desert => ':',
            Biome::Jungle => 'J',
            Biome::Forest => 'T',
            Biome::Taiga => 't',
            Biome::Swamp => 's',
            Biome::Mushroom => 'm',
            Biome::Tundra => '-',
            Biome::Snow => '*',
            Biome::Permafrost => '%',
            Biome::Mountain => '^',
            Biome::Corrupted => 'c',
            Biome::Infernal => 'i',
            Biome::Crystal => 'x',
            Biome::Enchanted => 'e',
            Biome::Candy => 'C',
            Biome::Wasteland => 'w',
            Biome::Ash => 'a',
            Biome::Lava => 'L',
        }
    }

    /// Pick the biome for a land tile from climate values.
    ///
    /// `elevation` is height above sea level (0 = coastline), `moisture` and
    /// `temperature` are 0..=100.
    pub fn from_climate(elevation: i16, moisture: u8, temperature: u8) -> Biome {
        if elevation > 620 {
            return if temperature < 25 {
                Biome::Snow
            } else {
                Biome::Mountain
            };
        }
        if elevation > 430 {
            return match temperature {
                t if t < 20 => Biome::Snow,
                t if t < 40 => Biome::Taiga,
                _ => Biome::Mountain,
            };
        }
        if temperature < 15 {
            return if moisture > 45 {
                Biome::Snow
            } else {
                Biome::Permafrost
            };
        }
        if temperature < 32 {
            return if moisture > 55 {
                Biome::Taiga
            } else {
                Biome::Tundra
            };
        }
        if temperature > 78 {
            return if moisture > 65 {
                Biome::Jungle
            } else if moisture > 30 {
                Biome::Savanna
            } else {
                Biome::Desert
            };
        }
        // Temperate band.
        if moisture > 78 && elevation < 120 {
            Biome::Swamp
        } else if moisture > 60 {
            Biome::Forest
        } else if moisture > 30 {
            Biome::Grass
        } else {
            Biome::Savanna
        }
    }
}

impl fmt::Display for Biome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// A single map tile.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tile {
    /// Height above sea level; negative is underwater. Range roughly -300..=900.
    pub elevation: i16,
    pub biome: Biome,
    /// Tree density 0..=3. Chopped for wood.
    pub trees: u8,
    /// Ore deposits 0..=3 (mined for ore/gold).
    pub ore: u8,
    /// Loose stone 0..=3 (mined for stone).
    pub stone: u8,
    /// Farm quality, derived from the biome but mutable (`Fertile Soil` power).
    pub fertile: u8,
    /// Ticks of fire left on this tile. 0 = not burning.
    pub fire: u8,
    /// Lava level 0..=3. Lava spreads, cools into `Infernal`/`Ash` and kills.
    pub lava: u8,
    /// Scorch/rubble left by nukes, meteors and volcanoes; decays slowly.
    pub scorch: u8,
    /// True if this tile is a river (rendered as water, crossable by land units).
    pub river: bool,
    /// Road level: 0 none, 1 dirt, 2 stone.
    pub road: u8,
    /// Owning village id, if any.
    pub owner: Option<u32>,
}

impl Default for Tile {
    fn default() -> Self {
        Tile {
            elevation: 0,
            biome: Biome::Ocean,
            trees: 0,
            ore: 0,
            stone: 0,
            fertile: 0,
            fire: 0,
            lava: 0,
            scorch: 0,
            river: false,
            road: 0,
            owner: None,
        }
    }
}

impl Tile {
    pub fn is_water(&self) -> bool {
        self.biome.is_water() || self.river
    }

    pub fn is_sea(&self) -> bool {
        self.biome.is_sea()
    }

    pub fn is_land(&self) -> bool {
        !self.is_water() && self.biome.is_land()
    }

    pub fn is_walkable(&self) -> bool {
        self.is_land() && self.biome.walkable()
    }

    pub fn is_mountain(&self) -> bool {
        self.biome == Biome::Mountain
    }

    pub fn is_burning(&self) -> bool {
        self.fire > 0
    }

    pub fn has_lava(&self) -> bool {
        self.lava > 0
    }

    pub fn has_tree(&self) -> bool {
        self.trees > 0
    }

    /// True when the tile is good enough to build on: dry, flat-ish, not lava.
    pub fn is_buildable(&self) -> bool {
        self.is_walkable() && self.elevation < 430 && self.lava == 0
    }

    /// Refreshes `fertile` from the biome. Called after terrain changes.
    pub fn refresh_fertility(&mut self) {
        let base = self.biome.fertility() as u16;
        // Rivers only enrich soil that can grow something in the first place.
        let bonus = if self.river && base > 0 { 20 } else { 0 };
        self.fertile = (base + bonus).min(100) as u8;
    }

    /// One-line description used by the CLI's tile inspector.
    pub fn describe(&self) -> String {
        let mut s = format!("{} (elev {})", self.biome.name(), self.elevation);
        if self.river {
            s.push_str(", river");
        }
        if self.trees > 0 {
            s.push_str(&format!(", {} trees", self.trees));
        }
        if self.ore > 0 {
            s.push_str(&format!(", ore {}", self.ore));
        }
        if self.stone > 0 {
            s.push_str(&format!(", stone {}", self.stone));
        }
        if self.fertile > 0 {
            s.push_str(&format!(", fertile {}", self.fertile));
        }
        if self.has_lava() {
            s.push_str(&format!(", lava {}", self.lava));
        }
        if self.is_burning() {
            s.push_str(&format!(", burning {}", self.fire));
        }
        if self.scorch > 0 {
            s.push_str(&format!(", scorched {}", self.scorch));
        }
        if self.road > 0 {
            s.push_str(", road");
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn biome_ids_roundtrip() {
        for b in Biome::ALL {
            assert_eq!(Biome::from_id(b.id()), b);
        }
        assert_eq!(Biome::from_id(200), Biome::Grass);
    }

    #[test]
    fn water_and_land_are_exclusive() {
        for b in Biome::ALL {
            assert!(!(b.is_water() && b.is_land()), "{b:?}");
        }
        assert!(Biome::Ocean.is_water() && !Biome::Ocean.is_land());
        assert!(Biome::Grass.is_land() && !Biome::Grass.is_water());
        assert!(!Biome::Mountain.walkable());
        assert!(Biome::Mountain.is_land());
    }

    #[test]
    fn climate_picks_sensible_biomes() {
        assert_eq!(Biome::from_climate(10, 20, 90), Biome::Desert);
        assert_eq!(Biome::from_climate(10, 90, 90), Biome::Jungle);
        assert_eq!(Biome::from_climate(10, 50, 50), Biome::Grass);
        assert_eq!(Biome::from_climate(10, 90, 50), Biome::Swamp);
        assert_eq!(Biome::from_climate(10, 20, 5), Biome::Permafrost);
        assert_eq!(Biome::from_climate(700, 50, 50), Biome::Mountain);
    }

    #[test]
    fn fertility_tracks_biome() {
        let mut t = Tile {
            biome: Biome::Grass,
            ..Default::default()
        };
        t.refresh_fertility();
        assert!(t.fertile >= 80);
        t.river = true;
        t.refresh_fertility();
        assert!(t.fertile >= 100);
        t.biome = Biome::Lava;
        t.refresh_fertility();
        assert_eq!(t.fertile, 0);
    }

    #[test]
    fn only_flora_burns() {
        assert!(Biome::Forest.can_burn());
        assert!(!Biome::Ocean.can_burn());
        assert!(!Biome::Mountain.can_burn());
        assert!(!Biome::Ice.can_burn());
    }
}
