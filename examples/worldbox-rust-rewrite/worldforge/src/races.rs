//! The creature catalog.
//!
//! Four civilized races (humans, orcs, elves, dwarves) plus bandits, monsters and
//! animals. Every race is data: base combat stats, lifespan, breeding rate, biome
//! preference, who it hates, and the trait bits it spawns with. Balancing is our
//! own — the *shapes* follow the genre (dwarves are tanky and mountain-loving,
//! orcs hit hard and hate humans and elves, elves live long and love forests).
//!
//! Numbers are ours, not extracted from the game: this crate ships no game data.

use crate::terrain::Biome;
use crate::units::traits;

/// Playable/simulated species.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub enum Race {
    // --- civilized ---------------------------------------------------------
    Human,
    Elf,
    Dwarf,
    Orc,
    Bandit,
    // --- monsters ----------------------------------------------------------
    Zombie,
    Skeleton,
    Demon,
    ColdOne,
    Tumor,
    Alien,
    Dragon,
    Ufo,
    Crabzilla,
    // --- animals -----------------------------------------------------------
    Sheep,
    Cow,
    Chicken,
    Deer,
    Wolf,
    Bear,
    Rat,
    Bee,
    Spider,
    Snake,
    Scorpion,
    Frog,
    Penguin,
    Turtle,
    Whale,
    Shark,
    Crab,
    Gorilla,
    Monkey,
    // --- extra animals -----------------------------------------------------
    Rabbit,
    Fox,
    Eagle,
}

/// Coarse class, used by spawners and UI grouping.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Category {
    /// Builds villages and kingdoms.
    Civilized,
    /// Eats, flees, breeds.
    Animal,
    /// Attacks whatever it finds.
    Monster,
    /// A single enormous unit, effectively a walking disaster.
    Boss,
}

/// Static definition of a race.
#[derive(Clone, Copy, Debug)]
pub struct RaceDef {
    pub race: Race,
    pub name: &'static str,
    pub category: Category,
    pub hp: i32,
    pub damage: i32,
    pub armor: i32,
    /// Movement points per tick; 1 tile of plain ground costs `10`.
    pub speed: i32,
    pub accuracy: f32,
    pub dodge: f32,
    /// Ticks between two attacks.
    pub attack_cooldown: u8,
    /// Old-age death becomes possible around here (years). 0 = immortal.
    pub max_age: u16,
    /// Chance per tick of a baby, when the village has food and housing.
    pub breed_chance: f32,
    pub trait_bits: u64,
    pub hates: &'static [Race],
    pub likes_biome: Biome,
    pub color: (u8, u8, u8),
    pub glyph: char,
    /// 1 = small, 2 = medium, 3 = large, 4 = boss.
    pub size: u8,
}

impl Race {
    /// Every race, in enum order.
    pub const ALL: [Race; 36] = [
        Race::Human,
        Race::Elf,
        Race::Dwarf,
        Race::Orc,
        Race::Bandit,
        Race::Zombie,
        Race::Skeleton,
        Race::Demon,
        Race::ColdOne,
        Race::Tumor,
        Race::Alien,
        Race::Dragon,
        Race::Ufo,
        Race::Crabzilla,
        Race::Sheep,
        Race::Cow,
        Race::Chicken,
        Race::Deer,
        Race::Wolf,
        Race::Bear,
        Race::Rat,
        Race::Bee,
        Race::Spider,
        Race::Snake,
        Race::Scorpion,
        Race::Frog,
        Race::Penguin,
        Race::Turtle,
        Race::Whale,
        Race::Shark,
        Race::Crab,
        Race::Gorilla,
        Race::Monkey,
        Race::Rabbit,
        Race::Fox,
        Race::Eagle,
    ];

    /// Races the player can place with the spawn brush, grouped for `--help`.
    pub const SPAWNABLE_CIVILIZED: [Race; 5] = [
        Race::Human,
        Race::Elf,
        Race::Dwarf,
        Race::Orc,
        Race::Bandit,
    ];
    pub const SPAWNABLE_MONSTERS: [Race; 9] = [
        Race::Zombie,
        Race::Skeleton,
        Race::Demon,
        Race::ColdOne,
        Race::Tumor,
        Race::Alien,
        Race::Dragon,
        Race::Ufo,
        Race::Crabzilla,
    ];
    pub const SPAWNABLE_ANIMALS: [Race; 18] = [
        Race::Sheep,
        Race::Cow,
        Race::Chicken,
        Race::Deer,
        Race::Wolf,
        Race::Bear,
        Race::Rat,
        Race::Bee,
        Race::Spider,
        Race::Snake,
        Race::Scorpion,
        Race::Frog,
        Race::Penguin,
        Race::Turtle,
        Race::Whale,
        Race::Shark,
        Race::Crab,
        Race::Gorilla,
    ];

    /// The full definition table.
    pub fn def(self) -> RaceDef {
        use Race::*;
        match self {
            Human => RaceDef {
                race: self,
                name: "Human",
                category: Category::Civilized,
                hp: 100,
                damage: 12,
                armor: 2,
                speed: 42,
                accuracy: 0.85,
                dodge: 0.10,
                attack_cooldown: 2,
                max_age: 70,
                breed_chance: 0.020,
                trait_bits: traits::SMART | traits::FAST_BREEDER,
                hates: &[],
                likes_biome: Biome::Grass,
                color: (232, 196, 150),
                glyph: 'h',
                size: 2,
            },
            Elf => RaceDef {
                race: self,
                name: "Elf",
                category: Category::Civilized,
                hp: 90,
                damage: 13,
                armor: 2,
                speed: 46,
                accuracy: 0.90,
                dodge: 0.15,
                attack_cooldown: 2,
                max_age: 400,
                breed_chance: 0.012,
                trait_bits: traits::NATURE_LOVER | traits::SMART,
                hates: &[Orc, Demon, Skeleton, Zombie],
                likes_biome: Biome::Forest,
                color: (186, 236, 178),
                glyph: 'e',
                size: 2,
            },
            Dwarf => RaceDef {
                race: self,
                name: "Dwarf",
                category: Category::Civilized,
                hp: 200,
                damage: 22,
                armor: 4,
                speed: 30,
                accuracy: 0.80,
                dodge: 0.05,
                attack_cooldown: 3,
                max_age: 160,
                breed_chance: 0.015,
                trait_bits: traits::MOUNTAIN_WALKER | traits::MINER | traits::WARRIOR,
                hates: &[Elf, Demon, Orc],
                likes_biome: Biome::Mountain,
                color: (214, 170, 120),
                glyph: 'd',
                size: 2,
            },
            Orc => RaceDef {
                race: self,
                name: "Orc",
                category: Category::Civilized,
                hp: 140,
                damage: 18,
                armor: 3,
                speed: 38,
                accuracy: 0.75,
                dodge: 0.08,
                attack_cooldown: 3,
                max_age: 60,
                breed_chance: 0.030,
                trait_bits: traits::CANNIBAL | traits::WARRIOR | traits::FAST_BREEDER,
                hates: &[Human, Elf, Dwarf],
                likes_biome: Biome::Savanna,
                color: (120, 170, 90),
                glyph: 'o',
                size: 2,
            },
            Bandit => RaceDef {
                race: self,
                name: "Bandit",
                category: Category::Civilized,
                hp: 110,
                damage: 15,
                armor: 2,
                speed: 44,
                accuracy: 0.78,
                dodge: 0.12,
                attack_cooldown: 2,
                max_age: 55,
                breed_chance: 0.018,
                trait_bits: traits::WARRIOR,
                hates: &[
                    Human, Elf, Dwarf, Orc, Bandit, Zombie, Skeleton, Demon, ColdOne, Tumor,
                ],
                likes_biome: Biome::Desert,
                color: (200, 120, 90),
                glyph: 'b',
                size: 2,
            },
            Zombie => RaceDef {
                race: self,
                name: "Zombie",
                category: Category::Monster,
                hp: 90,
                damage: 10,
                armor: 1,
                speed: 24,
                accuracy: 0.70,
                dodge: 0.0,
                attack_cooldown: 3,
                max_age: 0,
                breed_chance: 0.0,
                trait_bits: traits::ZOMBIE | traits::INFECTIOUS | traits::CANNIBAL,
                hates: &[
                    Human, Elf, Dwarf, Orc, Bandit, Zombie, Skeleton, Demon, ColdOne, Tumor,
                ],
                likes_biome: Biome::Corrupted,
                color: (140, 190, 120),
                glyph: 'z',
                size: 2,
            },
            Skeleton => RaceDef {
                race: self,
                name: "Skeleton",
                category: Category::Monster,
                hp: 60,
                damage: 9,
                armor: 2,
                speed: 34,
                accuracy: 0.88,
                dodge: 0.05,
                attack_cooldown: 2,
                max_age: 0,
                breed_chance: 0.0,
                trait_bits: traits::IMMORTAL | traits::FIREPROOF,
                hates: &[
                    Human, Elf, Dwarf, Orc, Bandit, Skeleton, Demon, ColdOne, Tumor,
                ],
                likes_biome: Biome::Ash,
                color: (222, 220, 205),
                glyph: 'k',
                size: 2,
            },
            Demon => RaceDef {
                race: self,
                name: "Demon",
                category: Category::Monster,
                hp: 260,
                damage: 30,
                armor: 5,
                speed: 40,
                accuracy: 0.85,
                dodge: 0.15,
                attack_cooldown: 2,
                max_age: 0,
                breed_chance: 0.0,
                trait_bits: traits::IMMORTAL | traits::FIREPROOF | traits::GIANT,
                hates: &[
                    Human, Elf, Dwarf, Orc, Bandit, Zombie, Skeleton, ColdOne, Tumor, Demon,
                ],
                likes_biome: Biome::Infernal,
                color: (200, 70, 60),
                glyph: 'D',
                size: 3,
            },
            ColdOne => RaceDef {
                race: self,
                name: "Cold One",
                category: Category::Monster,
                hp: 220,
                damage: 26,
                armor: 4,
                speed: 34,
                accuracy: 0.80,
                dodge: 0.05,
                attack_cooldown: 3,
                max_age: 0,
                breed_chance: 0.0,
                trait_bits: traits::IMMORTAL | traits::GIANT,
                hates: &[
                    Human, Elf, Dwarf, Orc, Bandit, Zombie, Skeleton, Demon, ColdOne, Tumor,
                ],
                likes_biome: Biome::Permafrost,
                color: (150, 210, 235),
                glyph: 'I',
                size: 3,
            },
            Tumor => RaceDef {
                race: self,
                name: "Tumor",
                category: Category::Monster,
                hp: 150,
                damage: 14,
                armor: 2,
                speed: 16,
                accuracy: 0.60,
                dodge: 0.0,
                attack_cooldown: 4,
                max_age: 0,
                breed_chance: 0.04,
                trait_bits: traits::INFECTIOUS,
                hates: &[
                    Human, Elf, Dwarf, Orc, Bandit, Zombie, Skeleton, Demon, ColdOne,
                ],
                likes_biome: Biome::Corrupted,
                color: (200, 110, 170),
                glyph: 'u',
                size: 2,
            },
            Alien => RaceDef {
                race: self,
                name: "Alien",
                category: Category::Monster,
                hp: 180,
                damage: 24,
                armor: 5,
                speed: 44,
                accuracy: 0.90,
                dodge: 0.20,
                attack_cooldown: 2,
                max_age: 0,
                breed_chance: 0.0,
                trait_bits: traits::IMMORTAL | traits::INFECTIOUS,
                hates: &[
                    Human, Elf, Dwarf, Orc, Bandit, Zombie, Skeleton, Demon, ColdOne,
                ],
                likes_biome: Biome::Crystal,
                color: (150, 240, 160),
                glyph: 'A',
                size: 3,
            },
            Dragon => RaceDef {
                race: self,
                name: "Dragon",
                category: Category::Boss,
                hp: 2000,
                damage: 80,
                armor: 12,
                speed: 60,
                accuracy: 0.95,
                dodge: 0.25,
                attack_cooldown: 3,
                max_age: 0,
                breed_chance: 0.0,
                trait_bits: traits::IMMORTAL
                    | traits::FLYING
                    | traits::FIREPROOF
                    | traits::GIANT
                    | traits::BOSS
                    | traits::MOUNTAIN_WALKER,
                hates: &[
                    Human, Elf, Dwarf, Orc, Bandit, Zombie, Skeleton, Demon, ColdOne, Tumor,
                ],
                likes_biome: Biome::Mountain,
                color: (180, 60, 80),
                glyph: 'R',
                size: 4,
            },
            Ufo => RaceDef {
                race: self,
                name: "UFO",
                category: Category::Boss,
                hp: 900,
                damage: 60,
                armor: 10,
                speed: 55,
                accuracy: 0.90,
                dodge: 0.30,
                attack_cooldown: 3,
                max_age: 0,
                breed_chance: 0.0,
                trait_bits: traits::IMMORTAL | traits::FLYING | traits::BOSS | traits::GIANT,
                hates: &[
                    Human, Elf, Dwarf, Orc, Bandit, Zombie, Skeleton, Demon, ColdOne, Tumor,
                ],
                likes_biome: Biome::Crystal,
                color: (190, 200, 220),
                glyph: '@',
                size: 4,
            },
            Crabzilla => RaceDef {
                race: self,
                name: "Crabzilla",
                category: Category::Boss,
                hp: 6000,
                damage: 200,
                armor: 20,
                speed: 20,
                accuracy: 0.95,
                dodge: 0.0,
                attack_cooldown: 4,
                max_age: 0,
                breed_chance: 0.0,
                trait_bits: traits::IMMORTAL | traits::GIANT | traits::BOSS | traits::SWIMMER,
                hates: &[
                    Human, Elf, Dwarf, Orc, Bandit, Zombie, Skeleton, Demon, ColdOne, Tumor,
                ],
                likes_biome: Biome::Beach,
                color: (220, 90, 70),
                glyph: '&',
                size: 4,
            },
            Sheep => animal(self, "Sheep", 40, 2, 0, 30, 0.5, Biome::Grass, (240, 240, 236), 'p'),
            Cow => animal(self, "Cow", 60, 3, 0, 26, 0.5, Biome::Grass, (225, 225, 225), 'q'),
            Chicken => animal(self, "Chicken", 20, 1, 0, 36, 0.6, Biome::Grass, (245, 230, 180), 'v'),
            Deer => animal(self, "Deer", 55, 2, 0, 48, 0.7, Biome::Forest, (170, 120, 80), 'n'),
            Wolf => RaceDef {
                breed_chance: 0.010,
                hates: &[Sheep, Deer, Chicken, Cow, Rabbit, Human, Elf, Dwarf, Orc],
                likes_biome: Biome::Taiga,
                ..animal(self, "Wolf", 70, 12, 1, 55, 0.8, Biome::Taiga, (150, 150, 160), 'w')
            },
            Bear => RaceDef {
                breed_chance: 0.006,
                hates: &[Sheep, Deer, Human, Elf, Dwarf, Orc, Chicken, Cow],
                likes_biome: Biome::Forest,
                ..animal(self, "Bear", 160, 22, 3, 42, 0.75, Biome::Forest, (140, 100, 70), 'B')
            },
            Rat => RaceDef {
                breed_chance: 0.05,
                trait_bits: traits::INFECTIOUS,
                hates: &[Chicken, Human, Elf, Dwarf, Orc],
                likes_biome: Biome::Wasteland,
                ..animal(self, "Rat", 15, 4, 0, 50, 0.7, Biome::Wasteland, (150, 140, 130), 'r')
            },
            Bee => RaceDef {
                breed_chance: 0.08,
                trait_bits: traits::FLYING,
                hates: &[Human, Elf, Dwarf, Orc, Bandit, Cow, Deer],
                likes_biome: Biome::Grass,
                ..animal(self, "Bee", 8, 3, 0, 60, 0.9, Biome::Grass, (240, 200, 60), 'j')
            },
            Spider => RaceDef {
                breed_chance: 0.02,
                hates: &[Human, Elf, Dwarf, Orc, Sheep, Deer, Chicken],
                likes_biome: Biome::Jungle,
                ..animal(self, "Spider", 45, 10, 1, 46, 0.8, Biome::Jungle, (90, 70, 90), 'y')
            },
            Snake => RaceDef {
                breed_chance: 0.01,
                hates: &[Human, Elf, Dwarf, Orc, Rat, Chicken, Frog],
                likes_biome: Biome::Swamp,
                ..animal(self, "Snake", 35, 9, 1, 44, 0.85, Biome::Swamp, (120, 160, 80), 'f')
            },
            Scorpion => RaceDef {
                breed_chance: 0.01,
                hates: &[Human, Elf, Dwarf, Orc, Spider, Rat],
                likes_biome: Biome::Desert,
                ..animal(self, "Scorpion", 40, 11, 2, 40, 0.8, Biome::Desert, (200, 160, 90), 'g')
            },
            Frog => animal(self, "Frog", 18, 3, 0, 34, 0.6, Biome::Swamp, (110, 180, 90), 'Q'),
            Penguin => animal(self, "Penguin", 45, 4, 0, 32, 0.6, Biome::Snow, (60, 70, 100), 'P'),
            Turtle => animal(self, "Turtle", 90, 5, 6, 14, 0.5, Biome::Beach, (110, 150, 110), 'T'),
            Whale => RaceDef {
                trait_bits: traits::SWIMMER,
                likes_biome: Biome::Ocean,
                ..animal(self, "Whale", 400, 20, 4, 30, 0.5, Biome::Ocean, (80, 110, 160), 'W')
            },
            Shark => RaceDef {
                trait_bits: traits::SWIMMER,
                hates: &[Whale, Turtle, Crab],
                likes_biome: Biome::Ocean,
                ..animal(self, "Shark", 140, 26, 2, 58, 0.85, Biome::Ocean, (140, 150, 170), 'S')
            },
            Crab => animal(self, "Crab", 50, 8, 3, 30, 0.7, Biome::Beach, (210, 110, 80), 'K'),
            Gorilla => RaceDef {
                breed_chance: 0.006,
                hates: &[Monkey, Human, Elf, Dwarf, Orc],
                likes_biome: Biome::Jungle,
                ..animal(self, "Gorilla", 180, 24, 3, 40, 0.8, Biome::Jungle, (80, 80, 90), 'G')
            },
            Monkey => animal(self, "Monkey", 45, 6, 0, 52, 0.8, Biome::Jungle, (170, 130, 90), 'M'),
            Rabbit => animal(self, "Rabbit", 18, 1, 0, 52, 0.6, Biome::Grass, (215, 205, 190), 'Y'),
            Fox => RaceDef {
                breed_chance: 0.012,
                hates: &[Rabbit, Chicken, Rat, Sheep],
                likes_biome: Biome::Forest,
                ..animal(self, "Fox", 60, 11, 1, 54, 0.85, Biome::Forest, (215, 130, 70), 'X')
            },
            Eagle => RaceDef {
                breed_chance: 0.008,
                trait_bits: traits::FLYING,
                hates: &[Rabbit, Chicken, Fox, Rat],
                likes_biome: Biome::Mountain,
                ..animal(self, "Eagle", 45, 10, 1, 70, 0.9, Biome::Mountain, (140, 110, 80), 'E')
            },
        }
    }

    pub fn name(self) -> &'static str {
        self.def().name
    }

    pub fn category(self) -> Category {
        self.def().category
    }

    pub fn is_civilized(self) -> bool {
        matches!(self.def().category, Category::Civilized)
    }

    pub fn is_animal(self) -> bool {
        matches!(self.def().category, Category::Animal)
    }

    pub fn is_monster(self) -> bool {
        matches!(self.def().category, Category::Monster | Category::Boss)
    }

    /// True for races that fight on sight instead of living in villages.
    pub fn is_hostile(self) -> bool {
        !self.is_civilized() || self == Race::Bandit
    }

    pub fn is_predator(self) -> bool {
        !self.def().hates.is_empty()
    }

    pub fn is_herbivore(self) -> bool {
        self.is_animal() && self.def().hates.is_empty()
    }

    pub fn is_immortal(self) -> bool {
        self.def().trait_bits & traits::IMMORTAL != 0
    }

    /// Name lookup, case-insensitive, by full name or first letter.
    pub fn parse(s: &str) -> Option<Race> {
        let lower = s.to_ascii_lowercase();
        Race::ALL
            .into_iter()
            .find(|r| r.name().to_ascii_lowercase() == lower)
            .or_else(|| {
                Race::ALL
                    .into_iter()
                    .find(|r| r.name().to_ascii_lowercase().starts_with(&lower))
            })
    }

    /// Does `self` hate `other`?
    pub fn hates_race(self, other: Race) -> bool {
        self.def().hates.contains(&other)
    }

    /// Diplomatic tension between two races, 0 (fine) .. 100 (sworn enemies).
    pub fn tension(self, other: Race) -> i32 {
        if self == other {
            return 0;
        }
        let mut t = 10;
        if self.hates_race(other) {
            t += 55;
        }
        if other.hates_race(self) {
            t += 35;
        }
        if self.is_monster() || other.is_monster() {
            t += 20;
        }
        t.min(100)
    }

    /// How comfortable this race is on a tile of the given biome, 0..=100.
    /// Used for village site scoring and for animal spawning.
    pub fn habitability(self, biome: Biome) -> i32 {
        let likes = self.def().likes_biome;
        let mut score = 40;
        if biome == likes {
            score += 45;
        }
        if matches!(
            (self, biome),
            (Race::Human, Biome::Grass | Biome::Beach | Biome::Forest)
                | (Race::Elf, Biome::Forest | Biome::Jungle | Biome::Enchanted)
                | (
                    Race::Dwarf,
                    Biome::Mountain | Biome::Tundra | Biome::Crystal | Biome::Snow
                )
                | (
                    Race::Orc,
                    Biome::Savanna | Biome::Wasteland | Biome::Corrupted | Biome::Ash
                )
                | (
                    Race::Bandit,
                    Biome::Desert | Biome::Wasteland | Biome::Ash | Biome::Corrupted
                )
        ) {
            score += 25;
        }
        if biome == Biome::Lava || biome == Biome::Ocean {
            score -= 60;
        }
        if !biome.is_land() {
            score -= 40;
        }
        score.clamp(0, 100)
    }
}

/// Shared constructor for the simple animal rows.
#[allow(clippy::too_many_arguments)]
fn animal(
    race: Race,
    name: &'static str,
    hp: i32,
    damage: i32,
    armor: i32,
    speed: i32,
    dodge: f32,
    likes: Biome,
    color: (u8, u8, u8),
    glyph: char,
) -> RaceDef {
    RaceDef {
        race,
        name,
        category: Category::Animal,
        hp,
        damage,
        armor,
        speed,
        accuracy: 0.70,
        dodge,
        attack_cooldown: 3,
        max_age: 25,
        breed_chance: 0.015,
        trait_bits: 0,
        hates: &[],
        likes_biome: likes,
        color,
        glyph,
        size: 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_races_have_sane_stats() {
        for r in Race::ALL {
            let d = r.def();
            assert_eq!(d.race, r);
            assert!(d.hp > 0, "{} has no hp", d.name);
            assert!(d.speed > 0, "{} cannot move", d.name);
            assert!((0.0..=1.0).contains(&d.accuracy), "{} accuracy", d.name);
            assert!((0.0..=1.0).contains(&d.dodge), "{} dodge", d.name);
            assert!(d.attack_cooldown >= 1, "{} attacks every tick", d.name);
            assert!(d.size >= 1 && d.size <= 4);
        }
    }

    #[test]
    fn only_the_four_races_plus_bandits_are_civilized() {
        let civs: Vec<&str> = Race::ALL
            .iter()
            .filter(|r| r.is_civilized())
            .map(|r| r.name())
            .collect();
        assert_eq!(civs.len(), 5);
        assert!(civs.contains(&"Human") && civs.contains(&"Orc"));
        assert!(!Race::Wolf.is_civilized());
    }

    #[test]
    fn every_boss_is_huge_and_immortal() {
        for r in Race::ALL.iter().filter(|r| r.category() == Category::Boss) {
            let d = r.def();
            assert!(d.trait_bits & traits::BOSS != 0, "{} is not a boss", d.name);
            assert!(d.trait_bits & traits::IMMORTAL != 0, "{} ages", d.name);
            assert!(d.hp >= 900, "{} is too squishy", d.name);
            assert_eq!(d.size, 4);
        }
    }

    #[test]
    fn race_tensions_are_sane_and_symmetric_enough() {
        assert!(Race::Orc.tension(Race::Human) > 40);
        assert!(Race::Elf.tension(Race::Orc) > 40);
        assert!(Race::Dwarf.tension(Race::Elf) > 40);
        assert_eq!(Race::Human.tension(Race::Human), 0);
        assert!(Race::Human.tension(Race::Orc) > 0);
        assert!(Race::Zombie.tension(Race::Human) > Race::Human.tension(Race::Sheep));
    }

    #[test]
    fn parsing_accepts_names_and_prefixes() {
        assert_eq!(Race::parse("human"), Some(Race::Human));
        assert_eq!(Race::parse("DWARF"), Some(Race::Dwarf));
        assert_eq!(Race::parse("dragon"), Some(Race::Dragon));
        assert_eq!(Race::parse("nope"), None);
    }

    #[test]
    fn habitability_follows_biome_preference() {
        assert!(Race::Dwarf.habitability(Biome::Mountain) > Race::Dwarf.habitability(Biome::Swamp));
        assert!(Race::Elf.habitability(Biome::Forest) > Race::Human.habitability(Biome::Forest) - 40);
        assert!(Race::Human.habitability(Biome::Lava) < 20);
    }

    #[test]
    fn spawnable_lists_are_valid() {
        for r in Race::SPAWNABLE_CIVILIZED {
            assert!(r.is_civilized());
        }
        for r in Race::SPAWNABLE_MONSTERS {
            assert!(r.is_monster());
        }
        for r in Race::SPAWNABLE_ANIMALS {
            assert!(r.is_animal());
        }
    }
}
