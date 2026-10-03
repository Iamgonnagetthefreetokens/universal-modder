//! Ages: the era system that reshapes the world every few decades.
//!
//! An age is a global modifier set: which biomes spread fastest, whether disasters
//! can happen at all, how loyal citizens feel, how warm the world is, what crawls
//! out of the dark. Ages last tens to a couple of hundred years and rotate.
//!
//! This module also owns the slow environment ticks that depend on the age:
//! biome creep, ice freezing and melting, and the resource drift that keeps the
//! map alive even when nobody is at war.

use crate::hex::Hex;
use crate::races::Race;
use crate::rng::Rng;
use crate::terrain::Biome;
use crate::world::{EventKind, World};

/// The world's current era.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Age {
    #[default]
    Hope,
    Sun,
    Tears,
    Dark,
    Moon,
    Chaos,
    Wonders,
    Ice,
    Ash,
    Despair,
    Skulls,
    Dragons,
    Gods,
}

impl Age {
    pub const ALL: [Age; 13] = [
        Age::Hope,
        Age::Sun,
        Age::Tears,
        Age::Dark,
        Age::Moon,
        Age::Chaos,
        Age::Wonders,
        Age::Ice,
        Age::Ash,
        Age::Despair,
        Age::Skulls,
        Age::Dragons,
        Age::Gods,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Age::Hope => "Age of Hope",
            Age::Sun => "Age of Sun",
            Age::Tears => "Age of Tears",
            Age::Dark => "Age of Dark",
            Age::Moon => "Age of Moon",
            Age::Chaos => "Age of Chaos",
            Age::Wonders => "Age of Wonders",
            Age::Ice => "Age of Ice",
            Age::Ash => "Age of Ash",
            Age::Despair => "Age of Despair",
            Age::Skulls => "Age of Skulls",
            Age::Dragons => "Age of Dragons",
            Age::Gods => "Age of Gods",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Age::Hope => "A bright beginning. Crops grow, kings keep the peace, disasters hold their breath.",
            Age::Sun => "Heat bakes the world. Deserts and savanna spread, fire runs wild.",
            Age::Tears => "Endless rain. Swamps and jungles swallow the lowlands.",
            Age::Dark => "The sun dims. Corruption creeps, the dead walk, loyalty frays.",
            Age::Moon => "Cold moonlight hardens the land into crystal. The undead are restless.",
            Age::Chaos => "Nothing is stable. Disasters strike twice as often and monsters swarm.",
            Age::Wonders => "Magic thickens the air. Enchanted and candy lands bloom, plagues fade.",
            Age::Ice => "The long winter. Seas freeze, permafrost spreads, food is scarce.",
            Age::Ash => "Life is fading. Ash covers the plains and few children are born.",
            Age::Despair => "Cold and dark. Corruption and frost share the world.",
            Age::Skulls => "Death reigns. Skeletons and zombies rise where the living fall.",
            Age::Dragons => "Wyrms wake in the mountains and burn villages for sport.",
            Age::Gods => "Divine power saturates the world: blessings rain, meteors answer prayers.",
        }
    }

    /// Growth multiplier (percent) for a biome in this age.
    pub fn biome_bonus(self, biome: Biome) -> u32 {
        
        match self {
            Age::Hope => 110,
            Age::Sun => match biome {
                Biome::Desert | Biome::Savanna | Biome::Beach => 260,
                Biome::Grass => 110,
                Biome::Forest | Biome::Taiga => 70,
                Biome::Ice | Biome::Snow | Biome::Permafrost => 50,
                _ => 100,
            },
            Age::Tears => match biome {
                Biome::Swamp | Biome::Mushroom | Biome::Jungle => 260,
                Biome::Grass | Biome::Forest => 140,
                Biome::Desert => 40,
                _ => 100,
            },
            Age::Dark => match biome {
                Biome::Corrupted => 300,
                Biome::Grass | Biome::Forest => 60,
                _ => 100,
            },
            Age::Moon => match biome {
                Biome::Crystal => 300,
                Biome::Tundra | Biome::Snow => 140,
                _ => 100,
            },
            Age::Chaos => match biome {
                Biome::Infernal => 320,
                Biome::Corrupted | Biome::Wasteland => 200,
                Biome::Grass | Biome::Forest => 60,
                _ => 100,
            },
            Age::Wonders => match biome {
                Biome::Enchanted | Biome::Candy => 300,
                _ => 100,
            },
            Age::Ice => match biome {
                Biome::Permafrost | Biome::Snow | Biome::Ice => 300,
                Biome::Tundra | Biome::Taiga => 150,
                Biome::Desert | Biome::Savanna | Biome::Jungle => 30,
                _ => 80,
            },
            Age::Ash => match biome {
                Biome::Ash | Biome::Wasteland => 260,
                Biome::Grass | Biome::Forest | Biome::Jungle => 40,
                _ => 80,
            },
            Age::Despair => match biome {
                Biome::Corrupted | Biome::Permafrost => 260,
                Biome::Grass | Biome::Forest => 50,
                _ => 90,
            },
            Age::Skulls => match biome {
                Biome::Ash | Biome::Corrupted => 200,
                Biome::Grass | Biome::Forest => 60,
                _ => 90,
            },
            Age::Dragons => match biome {
                Biome::Mountain | Biome::Wasteland => 180,
                Biome::Grass | Biome::Forest => 70,
                _ => 100,
            },
            Age::Gods => 130,
        }
    }

    /// Global temperature offset in biome-degrees. Negative freezes the world.
    pub fn temperature_delta(self) -> i32 {
        match self {
            Age::Sun => 22,
            Age::Chaos => 12,
            Age::Ice => -30,
            Age::Despair => -16,
            Age::Ash => -8,
            Age::Dark => -6,
            Age::Skulls => -4,
            Age::Gods => 6,
            Age::Wonders => 4,
            Age::Tears => 2,
            Age::Moon => -10,
            Age::Hope | Age::Dragons => 0,
        }
    }

    /// Multiplier (percent) on the chance of natural disasters.
    pub fn disaster_rate(self) -> u32 {
        match self {
            Age::Hope => 0,
            Age::Chaos => 220,
            Age::Dragons => 160,
            Age::Gods => 140,
            Age::Skulls => 130,
            Age::Ash => 120,
            Age::Dark => 110,
            _ => 100,
        }
    }

    /// Flat bonus to every village's loyalty, in percent points.
    pub fn loyalty_bonus(self) -> i32 {
        match self {
            Age::Hope => 25,
            Age::Gods => 20,
            Age::Wonders => 10,
            Age::Dragons => -5,
            Age::Chaos => -15,
            Age::Dark | Age::Despair => -20,
            Age::Skulls => -25,
            _ => 0,
        }
    }

    /// Extra aggression in diplomacy (opinion drift and war chance).
    pub fn war_bonus(self) -> i32 {
        match self {
            Age::Hope => -20,
            Age::Chaos => 35,
            Age::Skulls => 30,
            Age::Dragons => 25,
            Age::Dark => 15,
            Age::Sun | Age::Tears => 5,
            _ => 0,
        }
    }

    /// Multiplier (percent) on breeding.
    pub fn fertility_rate(self) -> u32 {
        match self {
            Age::Hope => 130,
            Age::Gods | Age::Wonders => 120,
            Age::Ash => 45,
            Age::Despair => 60,
            Age::Ice => 70,
            Age::Skulls => 80,
            _ => 100,
        }
    }

    /// Fire spread multiplier (percent).
    pub fn fire_rate(self) -> u32 {
        match self {
            Age::Sun => 220,
            Age::Chaos => 160,
            Age::Dragons => 140,
            Age::Tears => 20,
            Age::Ice => 30,
            _ => 100,
        }
    }

    /// How much the age frightens or heartens the world, used in flavour text.
    pub fn mood(self) -> &'static str {
        match self {
            Age::Hope | Age::Gods | Age::Wonders => "hopeful",
            Age::Sun | Age::Dragons => "restless",
            Age::Tears | Age::Moon => "strange",
            Age::Ice | Age::Ash | Age::Despair => "grim",
            Age::Dark | Age::Chaos | Age::Skulls => "dreadful",
        }
    }

    /// A hostile creature this age likes to spawn, if any.
    pub fn spawns(self) -> Option<Race> {
        match self {
            Age::Dark => Some(Race::Zombie),
            Age::Skulls => Some(Race::Skeleton),
            Age::Chaos => Some(Race::Demon),
            Age::Dragons => Some(Race::Dragon),
            Age::Despair => Some(Race::ColdOne),
            Age::Moon => Some(Race::Alien),
            Age::Ash => Some(Race::Tumor),
            _ => None,
        }
    }

    /// Does this age suppress natural disasters entirely?
    pub fn disasters_allowed(self) -> bool {
        self.disaster_rate() > 0
    }

    pub fn parse(s: &str) -> Option<Age> {
        let lower = s.to_ascii_lowercase();
        Age::ALL
            .into_iter()
            .find(|a| {
                let n = a.name().to_ascii_lowercase();
                n == lower || n == format!("age of {lower}") || n.starts_with(&lower)
            })
            .or_else(|| {
                Age::ALL
                    .into_iter()
                    .find(|a| a.name().to_ascii_lowercase().contains(&lower))
            })
    }
}

/// The world's age state: current era, how long it has lasted, and the history.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgeState {
    pub age: Age,
    pub years_in_age: u32,
    pub duration_years: u32,
    /// `(age, year it began)` in order.
    pub history: Vec<(Age, u32)>,
}

impl AgeState {
    pub fn new(seed: u64) -> Self {
        let mut rng = Rng::stream(seed, 0xa9e0);
        AgeState {
            age: Age::Hope,
            years_in_age: 0,
            duration_years: rng.range(40, 120) as u32,
            history: vec![(Age::Hope, 0)],
        }
    }

    /// Pick the next age. Never the same one twice in a row, and ages with a
    /// strong planetary flavour (Ice, Sun, Ash) are rarer so the world does not
    /// whiplash between extremes.
    pub fn roll_next(&self, rng: &mut Rng) -> Age {
        let weights: Vec<u32> = Age::ALL
            .iter()
            .map(|a| {
                if *a == self.age {
                    return 0;
                }
                let base = match a {
                    Age::Hope => 10,
                    Age::Sun | Age::Tears | Age::Dark | Age::Moon => 12,
                    Age::Wonders | Age::Dragons | Age::Gods => 7,
                    Age::Chaos | Age::Skulls => 9,
                    Age::Ice | Age::Ash | Age::Despair => 6,
                };
                // Opposites are more interesting than neighbours in the list.
                let opposite = match (self.age, a) {
                    (Age::Ice, Age::Sun)
                    | (Age::Sun, Age::Ice)
                    | (Age::Hope, Age::Despair)
                    | (Age::Despair, Age::Hope)
                    | (Age::Chaos, Age::Wonders)
                    | (Age::Wonders, Age::Chaos) => 8,
                    _ => 0,
                };
                base + opposite
            })
            .collect();
        Age::ALL[rng.weighted(&weights)]
    }
}

impl World {
    /// One year of age bookkeeping: transitions, announcements, and the global
    /// climate shift that comes with a new era.
    pub fn age_tick(&mut self) {
        let year = self.year;
        self.age.years_in_age += 1;
        if self.age.years_in_age < self.age.duration_years {
            return;
        }
        let next = self.age.roll_next(&mut self.rng);
        let mut rng = Rng::stream(self.seed ^ year as u64, 0x5151);
        self.age.age = next;
        self.age.years_in_age = 0;
        self.age.duration_years = rng.range(40, 160) as u32;
        self.age.history.push((next, year));
        self.stats.age_changes += 1;
        self.apply_age_climate();
        let text = format!("{} begins: {}", next.name(), next.description());
        self.chronicle(EventKind::Age, text);
        // The new era may immediately invite its creatures in.
        if let Some(race) = next.spawns() {
            if next != Age::Hope {
                self.spawn_age_creatures(race, 2);
            }
        }
    }

    /// Convert tiles when the climate flips: water freezes, ice melts, snow
    /// retreats, corruption dies back.
    pub fn apply_age_climate(&mut self) {
        let delta = self.age.age.temperature_delta();
        for i in 0..self.tiles.len() {
            let biome = self.tiles[i].biome;
            let new_biome = if delta <= -15 {
                match biome {
                    Biome::Shallow | Biome::Ocean => Biome::Ice,
                    Biome::Grass => Biome::Tundra,
                    Biome::Forest => Biome::Taiga,
                    Biome::Tundra => Biome::Permafrost,
                    Biome::Jungle => Biome::Forest,
                    Biome::Swamp => Biome::Tundra,
                    _ => biome,
                }
            } else if delta >= 15 {
                match biome {
                    Biome::Ice => Biome::Shallow,
                    Biome::Permafrost => Biome::Tundra,
                    Biome::Snow => Biome::Tundra,
                    Biome::Tundra => Biome::Grass,
                    Biome::Taiga => Biome::Forest,
                    _ => biome,
                }
            } else {
                biome
            };
            if new_biome != biome {
                self.tiles[i].biome = new_biome;
                let mut t = self.tiles[i];
                t.refresh_fertility();
                self.tiles[i] = t;
            }
        }
    }

    /// Spawn a handful of this age's creatures in a plausible place.
    pub fn spawn_age_creatures(&mut self, race: Race, count: i32) -> u32 {
        let mut spawned = 0;
        for _ in 0..count {
            if let Some(at) = self.random_land_tile() {
                let kind = if race.is_civilized() {
                    crate::units::UnitKind::Civilian
                } else {
                    crate::units::UnitKind::Monster
                };
                if self.spawn_unit(race, at, kind).is_some() {
                    spawned += 1;
                }
            }
        }
        spawned
    }

    /// Slow environmental change: biomes creep, forests regrow, deserts expand,
    /// corruption crawls. Runs every `BIOME_TICK_EVERY` ticks.
    pub fn biome_tick(&mut self) {
        let rate = self.age.age.biome_bonus(Biome::Grass); // baseline reference
        let age = self.age.age;
        // Which biomes are "growing" this age, and what they overgrow.
        let creep: [(Biome, Biome, u32); 12] = [
            (Biome::Forest, Biome::Grass, 26),
            (Biome::Jungle, Biome::Forest, 24),
            (Biome::Swamp, Biome::Grass, 22),
            (Biome::Desert, Biome::Savanna, 24),
            (Biome::Savanna, Biome::Grass, 18),
            (Biome::Corrupted, Biome::Forest, 26),
            (Biome::Infernal, Biome::Wasteland, 30),
            (Biome::Crystal, Biome::Tundra, 26),
            (Biome::Enchanted, Biome::Grass, 26),
            (Biome::Candy, Biome::Grass, 24),
            (Biome::Permafrost, Biome::Tundra, 26),
            (Biome::Ash, Biome::Grass, 22),
        ];
        for (from, onto, base_chance) in creep {
            let bonus = self.age.age.biome_bonus(from);
            let chance = base_chance as f32 * bonus as f32 / 10_000.0; // 0..~0.1
            if chance <= 0.0 {
                continue;
            }
            // Sample a few random tiles per tick rather than all of them.
            let samples = (self.tiles.len() / 220).max(6);
            for _ in 0..samples {
                let i = self.rng.below(self.tiles.len() as u32) as usize;
                if self.tiles[i].biome != from || self.tiles[i].river || self.tiles[i].fire > 0 {
                    continue;
                }
                if !self.rng.chance(chance) {
                    continue;
                }
                // Spread in a direction: pick a neighbour that is `onto`.
                let (c, r) = (
                    (i % self.width as usize) as i32,
                    (i / self.width as usize) as i32,
                );
                let h = Hex::from_offset(c, r);
                for nb in h.neighbors() {
                    if let Some(ni) = self.idx(nb) {
                        if self.tiles[ni].biome == onto && self.tiles[ni].lava == 0 {
                            self.tiles[ni].biome = from;
                            let mut t = self.tiles[ni];
                            t.refresh_fertility();
                            // New forest comes with trees; new desert with none.
                            if from.tree_capacity() > 0 && self.rng.chance(0.4) {
                                t.trees = t.trees.max(1);
                            } else if from.tree_capacity() == 0 {
                                t.trees = 0;
                            }
                            self.tiles[ni] = t;
                            break;
                        }
                    }
                }
            }
        }
        // Forest regrowth: trees come back on fertile, unowned tiles.
        let samples = (self.tiles.len() / 400).max(4);
        for _ in 0..samples {
            let i = self.rng.below(self.tiles.len() as u32) as usize;
            let cap = self.tiles[i].biome.tree_capacity();
            if cap == 0 || self.tiles[i].trees >= cap || self.tiles[i].fire > 0 {
                continue;
            }
            let greed = self.age.age.biome_bonus(self.tiles[i].biome) as f32 / 100.0;
            if self.rng.chance(0.05 * greed) {
                self.tiles[i].trees += 1;
            }
        }
        // Scorch and rubble fade over decades.
        if self.tick % 200 == 0 {
            for t in self.tiles.iter_mut() {
                if t.scorch > 0 {
                    t.scorch -= 1;
                }
            }
        }
        let _ = (rate, age);
    }

    /// A random land tile (uniform over land, not over the map).
    pub fn random_land_tile(&mut self) -> Option<Hex> {
        for _ in 0..64 {
            let c = self.rng.below(self.width as u32) as i32;
            let r = self.rng.below(self.height as u32) as i32;
            let h = Hex::from_offset(c, r);
            if let Some(t) = self.tile(h) {
                if t.is_walkable() && t.lava == 0 {
                    return Some(h);
                }
            }
        }
        // Fall back to a scan so we never fail on an odd map.
        for r in 0..self.height as i32 {
            for c in 0..self.width as i32 {
                let h = Hex::from_offset(c, r);
                if let Some(t) = self.tile(h) {
                    if t.is_walkable() && t.lava == 0 {
                        return Some(h);
                    }
                }
            }
        }
        None
    }

    /// A random tile anywhere on the map (including water).
    pub fn random_tile(&mut self) -> Hex {
        let c = self.rng.below(self.width as u32) as i32;
        let r = self.rng.below(self.height as u32) as i32;
        Hex::from_offset(c, r)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worldgen::{GenParams, WorldType};

    fn test_world() -> World {
        World::generate(GenParams::new(32, 24, 4242, WorldType::Continents))
    }

    #[test]
    fn every_age_has_a_description_and_a_parse() {
        for age in Age::ALL {
            assert!(!age.description().is_empty());
            assert_eq!(Age::parse(age.name()), Some(age));
        }
        assert_eq!(Age::parse("ice"), Some(Age::Ice));
        assert_eq!(Age::parse("age of dragons"), Some(Age::Dragons));
        assert_eq!(Age::parse("nonsense"), None);
    }

    #[test]
    fn hope_is_peaceful_and_ice_is_cold() {
        assert!(!Age::Hope.disasters_allowed());
        assert!(Age::Hope.loyalty_bonus() > 0);
        assert!(Age::Ice.temperature_delta() < 0);
        assert!(Age::Sun.temperature_delta() > 0);
        assert!(Age::Chaos.disaster_rate() > 100);
        assert!(Age::Ash.fertility_rate() < 100);
    }

    #[test]
    fn age_rolls_do_not_repeat_the_current_age() {
        let mut rng = Rng::new(1);
        let mut state = AgeState::new(1);
        for _ in 0..500 {
            let next = state.roll_next(&mut rng);
            assert_ne!(next, state.age);
            state.age = next;
        }
    }

    #[test]
    fn ice_age_freezes_water_and_sun_age_thaws_it() {
        let mut world = test_world();
        world.age.age = Age::Ice;
        world.apply_age_climate();
        let ice = world
            .tiles
            .iter()
            .filter(|t| t.biome == Biome::Ice)
            .count();
        assert!(ice > 0, "ice age should freeze the seas");
        world.age.age = Age::Sun;
        world.apply_age_climate();
        let remaining = world
            .tiles
            .iter()
            .filter(|t| t.biome == Biome::Ice)
            .count();
        assert_eq!(remaining, 0, "sun age should melt all ice");
    }

    #[test]
    fn age_transitions_are_chronicled() {
        let mut world = test_world();
        world.age.duration_years = 1;
        world.age_tick();
        assert_eq!(world.age.history.len(), 2);
        assert_eq!(world.stats.age_changes, 1);
        assert!(world.chronicle.iter().any(|e| e.kind == EventKind::Age));
    }

    #[test]
    fn biome_ticks_change_the_map_but_never_water_into_land() {
        let mut world = test_world();
        world.age.age = Age::Chaos;
        let before: Vec<Biome> = world.tiles.iter().map(|t| t.biome).collect();
        for _ in 0..200 {
            world.biome_tick();
        }
        let after: Vec<Biome> = world.tiles.iter().map(|t| t.biome).collect();
        assert_ne!(before, after, "biomes should creep");
        for (i, b) in after.iter().enumerate() {
            if before[i] == Biome::Ocean {
                assert_eq!(*b, Biome::Ocean, "ocean tiles must stay ocean");
            }
        }
    }

    #[test]
    fn random_land_tiles_are_walkable() {
        let mut world = test_world();
        for _ in 0..200 {
            let h = world.random_land_tile().unwrap();
            let t = world.tile(h).unwrap();
            assert!(t.is_walkable() && t.lava == 0);
        }
    }
}
