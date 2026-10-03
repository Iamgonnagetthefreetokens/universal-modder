//! God powers: the player's toolbox.
//!
//! Every power is a single `cast(power, hex)` call that mutates the world and
//! returns a [`CastOutcome`] (what happened, how many creatures it touched, how
//! many died). That shape is deliberate: it makes powers testable, scriptable and
//! loggable, and it is what the scripted oracle drives.

use crate::disaster::CloudKind;
use crate::hex::Hex;
use crate::races::Race;
use crate::terrain::Biome;
use crate::units::{traits, StatusKind, UnitKind};
use crate::world::{EventKind, World};

/// Which tab of the toolbox a power lives in.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PowerCategory {
    Destruction,
    LifeAndDeath,
    Divine,
    WorldShaping,
    Civilization,
    Summon,
}

impl PowerCategory {
    pub fn name(self) -> &'static str {
        match self {
            PowerCategory::Destruction => "destruction",
            PowerCategory::LifeAndDeath => "life and death",
            PowerCategory::Divine => "divine",
            PowerCategory::WorldShaping => "world shaping",
            PowerCategory::Civilization => "civilization",
            PowerCategory::Summon => "summon",
        }
    }
}

/// Everything the player can do to the world with one click.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Power {
    // destruction
    Lightning,
    FireBeam,
    Meteor,
    Nuke,
    Volcano,
    Tornado,
    BlackHole,
    AcidRain,
    FireRain,
    Rain,
    Earthquake,
    IceStorm,
    Wildfire,
    // life and death
    Plague,
    ZombieInfection,
    Madness,
    Spores,
    AlienMold,
    TumorGrowth,
    // divine
    Blessing,
    Curse,
    Shield,
    Coffee,
    PowerUp,
    DivineLight,
    Dispel,
    BloodRain,
    CloneRain,
    SmoothJazz,
    Sleep,
    Magnet,
    // world shaping
    RaiseLand,
    LowerLand,
    MountainBrush,
    ForestBrush,
    FertileSoil,
    DesertBrush,
    SnowBrush,
    LavaBrush,
    LakeBrush,
    OceanBrush,
    SmoothLand,
    // civilization
    Inspiration,
    Friendship,
    Spite,
    WhisperOfWar,
    // summon
    SummonDragon,
    SummonUfo,
    SummonCrabzilla,
}

impl Power {
    pub const ALL: [Power; 49] = [
        Power::Lightning,
        Power::FireBeam,
        Power::Meteor,
        Power::Nuke,
        Power::Volcano,
        Power::Tornado,
        Power::BlackHole,
        Power::AcidRain,
        Power::FireRain,
        Power::Rain,
        Power::Earthquake,
        Power::IceStorm,
        Power::Wildfire,
        Power::Plague,
        Power::ZombieInfection,
        Power::Madness,
        Power::Spores,
        Power::AlienMold,
        Power::TumorGrowth,
        Power::Blessing,
        Power::Curse,
        Power::Shield,
        Power::Coffee,
        Power::PowerUp,
        Power::DivineLight,
        Power::Dispel,
        Power::BloodRain,
        Power::CloneRain,
        Power::SmoothJazz,
        Power::Sleep,
        Power::Magnet,
        Power::RaiseLand,
        Power::LowerLand,
        Power::MountainBrush,
        Power::ForestBrush,
        Power::FertileSoil,
        Power::DesertBrush,
        Power::SnowBrush,
        Power::LavaBrush,
        Power::LakeBrush,
        Power::OceanBrush,
        Power::SmoothLand,
        Power::Inspiration,
        Power::Friendship,
        Power::Spite,
        Power::WhisperOfWar,
        Power::SummonDragon,
        Power::SummonUfo,
        Power::SummonCrabzilla,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Power::Lightning => "lightning",
            Power::FireBeam => "fire",
            Power::Meteor => "meteor",
            Power::Nuke => "nuke",
            Power::Volcano => "volcano",
            Power::Tornado => "tornado",
            Power::BlackHole => "black hole",
            Power::AcidRain => "acid rain",
            Power::FireRain => "fire rain",
            Power::Rain => "rain",
            Power::Earthquake => "earthquake",
            Power::IceStorm => "ice storm",
            Power::Wildfire => "wildfire",
            Power::Plague => "plague",
            Power::ZombieInfection => "zombie infection",
            Power::Madness => "madness",
            Power::Spores => "mushroom spores",
            Power::AlienMold => "alien mold",
            Power::TumorGrowth => "tumor",
            Power::Blessing => "blessing",
            Power::Curse => "curse",
            Power::Shield => "shield",
            Power::Coffee => "coffee",
            Power::PowerUp => "powerup",
            Power::DivineLight => "divine light",
            Power::Dispel => "dispel",
            Power::BloodRain => "blood rain",
            Power::CloneRain => "clone rain",
            Power::SmoothJazz => "smooth jazz",
            Power::Sleep => "sleep",
            Power::Magnet => "magnet",
            Power::RaiseLand => "raise land",
            Power::LowerLand => "lower land",
            Power::MountainBrush => "mountain",
            Power::ForestBrush => "forest",
            Power::FertileSoil => "fertile soil",
            Power::DesertBrush => "desert",
            Power::SnowBrush => "snow",
            Power::LavaBrush => "lava",
            Power::LakeBrush => "lake",
            Power::OceanBrush => "ocean",
            Power::SmoothLand => "smooth",
            Power::Inspiration => "inspiration",
            Power::Friendship => "friendship",
            Power::Spite => "spite",
            Power::WhisperOfWar => "whisper of war",
            Power::SummonDragon => "dragon",
            Power::SummonUfo => "ufo",
            Power::SummonCrabzilla => "crabzilla",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Power::Lightning => "A bolt from the sky: burns one creature or sets the ground alight",
            Power::FireBeam => "Set a tile and its neighbours on fire",
            Power::Meteor => "A rock falls for a few seconds, then a crater and a shockwave",
            Power::Nuke => "Crater, firestorm and radiation. Nothing nearby survives",
            Power::Volcano => "Open a vent: a mountain grows, lava spills for years",
            Power::Tornado => "A wandering funnel that wrecks buildings and flings creatures",
            Power::BlackHole => "Drags everything in, then collapses into a scar",
            Power::AcidRain => "Burns creatures and kills plants over a wide area",
            Power::FireRain => "Rains fire over an area",
            Power::Rain => "Puts out fires and waters the soil",
            Power::Earthquake => "Cracks the ground, shakes buildings down, opens lava",
            Power::IceStorm => "Freezes the land and the seas around a point",
            Power::Wildfire => "Sets the whole world's vegetation alight",
            Power::Plague => "A spreading disease: most who catch it die",
            Power::ZombieInfection => "Infected creatures rise again as zombies",
            Power::Madness => "Creatures go insane and attack everything, including each other",
            Power::Spores => "Mushrooms spread, and creatures breathe in the infection",
            Power::AlienMold => "Corrupts the ground and infects whatever touches it",
            Power::TumorGrowth => "Grows tumours that breed and infect",
            Power::Blessing => "Bless creatures: stronger, tougher, luckier",
            Power::Curse => "Curse creatures: weak, brittle and doomed",
            Power::Shield => "Shield creatures from all damage for a while",
            Power::Coffee => "Caffeinated creatures move and strike faster",
            Power::PowerUp => "Doubles the damage of everything nearby",
            Power::DivineLight => "A miracle: cures disease, madness and infection",
            Power::Dispel => "Removes every status effect nearby",
            Power::BloodRain => "Heals creatures and puts out burning",
            Power::CloneRain => "Copies whatever creature is standing there",
            Power::SmoothJazz => "The right music: creatures relax, villages grow",
            Power::Sleep => "Creatures fall asleep",
            Power::Magnet => "Drag the nearest creature to the cursor",
            Power::RaiseLand => "Lift the ground out of the sea",
            Power::LowerLand => "Push land below the waterline",
            Power::MountainBrush => "Raise mountains",
            Power::ForestBrush => "Plant a forest",
            Power::FertileSoil => "Turn the ground into farmland",
            Power::DesertBrush => "Scorch the ground to sand",
            Power::SnowBrush => "Freeze the ground",
            Power::LavaBrush => "Pour lava",
            Power::LakeBrush => "Dig a lake",
            Power::OceanBrush => "Dig deep water",
            Power::SmoothLand => "Flatten the terrain around a point",
            Power::Inspiration => "A village's people demand independence",
            Power::Friendship => "End every war a kingdom is fighting",
            Power::Spite => "Force a kingdom to attack its neighbour",
            Power::WhisperOfWar => "Poison two kingdoms' opinion of each other",
            Power::SummonDragon => "Summon a dragon: it burns what it flies over",
            Power::SummonUfo => "Summon a UFO: it abducts and burns",
            Power::SummonCrabzilla => "Summon Crabzilla, the walking apocalypse",
        }
    }

    pub fn category(self) -> PowerCategory {
        match self {
            Power::Lightning
            | Power::FireBeam
            | Power::Meteor
            | Power::Nuke
            | Power::Volcano
            | Power::Tornado
            | Power::BlackHole
            | Power::AcidRain
            | Power::FireRain
            | Power::Rain
            | Power::Earthquake
            | Power::IceStorm
            | Power::Wildfire => PowerCategory::Destruction,
            Power::Plague
            | Power::ZombieInfection
            | Power::Madness
            | Power::Spores
            | Power::AlienMold
            | Power::TumorGrowth => PowerCategory::LifeAndDeath,
            Power::Blessing
            | Power::Curse
            | Power::Shield
            | Power::Coffee
            | Power::PowerUp
            | Power::DivineLight
            | Power::Dispel
            | Power::BloodRain
            | Power::CloneRain
            | Power::SmoothJazz
            | Power::Sleep
            | Power::Magnet => PowerCategory::Divine,
            Power::RaiseLand
            | Power::LowerLand
            | Power::MountainBrush
            | Power::ForestBrush
            | Power::FertileSoil
            | Power::DesertBrush
            | Power::SnowBrush
            | Power::LavaBrush
            | Power::LakeBrush
            | Power::OceanBrush
            | Power::SmoothLand => PowerCategory::WorldShaping,
            Power::Inspiration | Power::Friendship | Power::Spite | Power::WhisperOfWar => {
                PowerCategory::Civilization
            }
            Power::SummonDragon | Power::SummonUfo | Power::SummonCrabzilla => {
                PowerCategory::Summon
            }
        }
    }

    /// Case/space/underscore-insensitive lookup.
    pub fn parse(s: &str) -> Option<Power> {
        let norm = s
            .to_ascii_lowercase()
            .replace(['_', '-'], " ")
            .trim()
            .to_string();
        Power::ALL
            .into_iter()
            .find(|p| p.name() == norm)
            .or_else(|| {
                Power::ALL.into_iter().find(|p| {
                    let n = p.name();
                    n.starts_with(&norm) || n.replace(' ', "") == norm.replace(' ', "")
                })
            })
    }
}

/// What a cast actually did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CastOutcome {
    pub power: Power,
    pub target: Hex,
    pub ok: bool,
    pub message: String,
    pub units_affected: u32,
    pub killed: u32,
}

impl World {
    /// Cast a power at a tile.
    pub fn cast(&mut self, power: Power, at: Hex) -> CastOutcome {
        let mut out = CastOutcome {
            power,
            target: at,
            ok: true,
            message: String::new(),
            units_affected: 0,
            killed: 0,
        };
        if !self.in_bounds(at) {
            out.ok = false;
            out.message = format!("{at} is off the map");
            return out;
        }
        let before = self.unit_count();
        match power {
            // --- destruction -------------------------------------------------
            Power::Lightning => {
                let here = self.units_in_radius(at, 0);
                if here.is_empty() {
                    self.ignite(at, 26);
                    self.set_biome(at, Biome::Wasteland);
                    out.message = format!("Lightning strikes the ground at {at}");
                } else {
                    let mut struck = 0;
                    for id in here {
                        let wet = self
                            .unit(id)
                            .map(|u| u.has_status(StatusKind::Wet))
                            .unwrap_or(false);
                        let dmg = if wet { 160 } else { 70 };
                        self.damage_unit(id, dmg, None);
                        if let Some(u) = self.unit_mut(id) {
                            u.apply_status(StatusKind::Burning, 24);
                        }
                        struck += 1;
                    }
                    out.units_affected = struck;
                    out.message = format!("Lightning strikes {struck} creature(s) at {at}");
                }
                self.ignite(at, 20);
            }
            Power::FireBeam => {
                for h in at.spiral(1) {
                    self.ignite(h, 30);
                }
                out.message = format!("Fire catches at {at}");
            }
            Power::Meteor => {
                self.cast_meteor(at, 2, 420, false);
                out.message = format!("A meteor is falling toward {at}");
            }
            Power::Nuke => {
                out.killed = self.nuke(at, 600);
                out.message = format!("Nuclear strike at {at}");
            }
            Power::Volcano => {
                self.cast_volcano(at);
                out.message = format!("A volcano opens at {at}");
            }
            Power::Tornado => {
                self.cast_tornado(at, 40);
                out.message = format!("A tornado touches down at {at}");
            }
            Power::BlackHole => {
                self.cast_black_hole(at, 4);
                out.message = format!("A black hole opens at {at}");
            }
            Power::AcidRain => {
                self.cast_cloud(at, CloudKind::AcidRain);
                out.message = format!("Acid rain falls on {at}");
            }
            Power::FireRain => {
                self.cast_cloud(at, CloudKind::Fire);
                out.message = format!("Fire rains on {at}");
            }
            Power::Rain => {
                self.cast_cloud(at, CloudKind::Rain);
                out.message = format!("Rain falls on {at}");
            }
            Power::Earthquake => {
                out.units_affected = self.units_in_radius(at, 6).len() as u32;
                out.killed = self.earthquake(at, 6);
                out.message = format!("The ground shakes at {at}");
            }
            Power::IceStorm => {
                let frozen = self.freeze_area(at, 5);
                out.message = format!("{frozen} tiles freeze over at {at}");
            }
            Power::Wildfire => {
                let lit = self.ignite_world(24);
                out.message = format!("{lit} tiles catch fire across the world");
            }
            // --- life and death ----------------------------------------------
            Power::Plague => {
                let ids = self.units_in_radius(at, 3);
                out.units_affected = ids.len() as u32;
                for id in ids {
                    if let Some(u) = self.unit_mut(id) {
                        u.apply_status(StatusKind::Plague, 400);
                    }
                }
                out.message = format!("Plague breaks out at {at}");
            }
            Power::ZombieInfection => {
                let ids = self.units_in_radius(at, 3);
                out.units_affected = ids.len() as u32;
                for id in ids {
                    if let Some(u) = self.unit_mut(id) {
                        u.add_trait(traits::INFECTED);
                    }
                }
                out.message = format!("Zombie infection spreads from {at}");
            }
            Power::Madness => {
                let ids = self.units_in_radius(at, 3);
                out.units_affected = ids.len() as u32;
                for id in ids {
                    if let Some(u) = self.unit_mut(id) {
                        u.apply_status(StatusKind::Mad, 300);
                    }
                }
                self.cast_cloud(at, CloudKind::Madness);
                out.message = format!("Creatures at {at} go mad");
            }
            Power::Spores => {
                let mut changed = 0;
                for h in at.spiral(3) {
                    if let Some(t) = self.tile(h).copied() {
                        if t.is_land() {
                            self.set_biome(h, Biome::Mushroom);
                            changed += 1;
                        }
                    }
                }
                let ids = self.units_in_radius(at, 3);
                out.units_affected = ids.len() as u32;
                for id in ids {
                    if let Some(u) = self.unit_mut(id) {
                        u.add_trait(traits::INFECTED);
                    }
                }
                out.message = format!("Mushroom spores cover {changed} tiles at {at}");
            }
            Power::AlienMold => {
                let mut changed = 0;
                for h in at.spiral(2) {
                    if let Some(t) = self.tile(h).copied() {
                        if t.is_land() {
                            self.set_biome(h, Biome::Corrupted);
                            changed += 1;
                        }
                    }
                }
                for id in self.units_in_radius(at, 2) {
                    if let Some(u) = self.unit_mut(id) {
                        u.add_trait(traits::INFECTED);
                    }
                }
                out.message = format!("Alien mold spreads over {changed} tiles at {at}");
            }
            Power::TumorGrowth => {
                let mut spawned = 0;
                for _ in 0..3 {
                    if self.spawn_unit(Race::Tumor, at, UnitKind::Monster).is_some() {
                        spawned += 1;
                    }
                }
                out.units_affected = spawned;
                out.message = format!("{spawned} tumour(s) sprout at {at}");
            }
            // --- divine -------------------------------------------------------
            Power::Blessing | Power::Curse | Power::Shield | Power::Coffee | Power::PowerUp => {
                let (bit, status, label): (u64, Option<StatusKind>, &str) = match power {
                    Power::Blessing => (traits::BLESSED, None, "blessed"),
                    Power::Curse => (traits::CURSED, None, "cursed"),
                    Power::Shield => (
                        traits::SHIELDED,
                        Some(StatusKind::Shielded),
                        "shielded",
                    ),
                    Power::Coffee => (
                        traits::CAFFEINATED,
                        Some(StatusKind::Caffeinated),
                        "caffeinated",
                    ),
                    Power::PowerUp => (traits::POWERED, None, "powered up"),
                    _ => (0, None, ""),
                };
                let ids = self.units_in_radius(at, 3);
                out.units_affected = ids.len() as u32;
                for id in ids {
                    if let Some(u) = self.unit_mut(id) {
                        u.add_trait(bit);
                        if bit == traits::BLESSED {
                            u.remove_trait(traits::CURSED);
                        }
                        if bit == traits::CURSED {
                            u.remove_trait(traits::BLESSED);
                        }
                        if let Some(s) = status {
                            u.apply_status(s, s.default_ticks());
                        }
                    }
                }
                out.message = format!("{} creature(s) at {at} are {label}", out.units_affected);
            }
            Power::DivineLight => {
                let ids = self.units_in_radius(at, 4);
                out.units_affected = ids.len() as u32;
                for id in ids {
                    if let Some(u) = self.unit_mut(id) {
                        u.remove_trait(traits::INFECTED);
                        u.remove_trait(traits::MAD);
                        u.clear_status(StatusKind::Plague);
                        u.clear_status(StatusKind::Mad);
                        u.clear_status(StatusKind::Poisoned);
                        u.clear_status(StatusKind::Burning);
                        u.clear_status(StatusKind::Bleeding);
                    }
                }
                for h in at.spiral(4) {
                    if let Some(t) = self.tile(h).copied() {
                        if matches!(t.biome, Biome::Corrupted | Biome::Mushroom) {
                            self.set_biome(h, Biome::Grass);
                        }
                    }
                }
                out.message = format!("Divine light cleanses {} creature(s) at {at}", out.units_affected);
            }
            Power::Dispel => {
                let ids = self.units_in_radius(at, 4);
                out.units_affected = ids.len() as u32;
                for id in ids {
                    if let Some(u) = self.unit_mut(id) {
                        u.clear_statuses();
                        u.remove_trait(traits::BLESSED);
                        u.remove_trait(traits::CURSED);
                        u.remove_trait(traits::POWERED);
                        u.remove_trait(traits::SHIELDED);
                    }
                }
                self.effects.clouds.clear();
                out.message = format!("All magic dispelled around {at}");
            }
            Power::BloodRain => {
                self.cast_cloud(at, CloudKind::Blood);
                out.message = format!("Blood rain falls on {at}");
            }
            Power::CloneRain => {
                let ids = self.units_in_radius(at, 3);
                if let Some(first) = ids.first().copied() {
                    let (race, kind, traits_bits, item) = match self.unit(first) {
                        Some(u) => (u.race, u.kind, u.traits, u.item),
                        None => (Race::Human, UnitKind::Civilian, 0, crate::units::Item::none()),
                    };
                    let mut clones = 0;
                    for _ in 0..12 {
                        if let Some(cid) = self.spawn_unit(race, at, kind) {
                            if let Some(c) = self.unit_mut(cid) {
                                c.traits |= traits_bits;
                                c.item = item;
                            }
                            clones += 1;
                        }
                    }
                    out.units_affected = clones;
                    out.message = format!("{clones} clones of the {} appear at {at}", race.name());
                } else {
                    out.ok = false;
                    out.message = format!("nothing to clone at {at}");
                }
            }
            Power::SmoothJazz => {
                let ids = self.units_in_radius(at, 4);
                for id in ids {
                    if let Some(u) = self.unit_mut(id) {
                        u.apply_status(StatusKind::Caffeinated, 200);
                        u.hunger = 0;
                    }
                }
                for h in at.spiral(3) {
                    if let Some(v) = self.tile(h).and_then(|t| t.owner) {
                        if let Some(vill) = self.village_mut(v) {
                            vill.loyalty = (vill.loyalty + 5).min(100);
                            vill.food += 5;
                        }
                    }
                }
                out.message = format!("Smooth jazz plays at {at}");
            }
            Power::Sleep => {
                let ids = self.units_in_radius(at, 3);
                out.units_affected = ids.len() as u32;
                for id in ids {
                    if let Some(u) = self.unit_mut(id) {
                        u.apply_status(StatusKind::Sleeping, 200);
                    }
                }
                out.message = format!("{} creature(s) fall asleep at {at}", out.units_affected);
            }
            Power::Magnet => {
                let ids = self.units_in_radius(at, 8);
                if let Some(id) = ids.first().copied() {
                    let race = self.unit(id).map(|u| u.race);
                    if let Some(u) = self.unit_mut(id) {
                        u.pos = at;
                        u.path.clear();
                        u.path_i = 0;
                        u.destination = None;
                    }
                    out.units_affected = 1;
                    out.message = format!(
                        "{} dragged to {at}",
                        race.map(|r| r.name()).unwrap_or("something")
                    );
                } else {
                    out.ok = false;
                    out.message = format!("no creature within reach of {at}");
                }
            }
            // --- world shaping ------------------------------------------------
            Power::RaiseLand | Power::MountainBrush => {
                let amount = if power == Power::MountainBrush { 260 } else { 60 };
                let mut touched = 0;
                for h in at.spiral(2) {
                    let Some(t) = self.tile(h).copied() else { continue };
                    if let Some(tile) = self.tile_mut(h) {
                        tile.elevation = (tile.elevation.saturating_add(amount)).min(900);
                    }
                    if power == Power::MountainBrush {
                        self.set_biome(h, Biome::Mountain);
                        if let Some(tile) = self.tile_mut(h) {
                            tile.stone = tile.stone.max(2);
                        }
                    } else if t.is_water() {
                        self.set_biome(h, Biome::Beach);
                    } else if t.biome == Biome::Mountain {
                        self.set_biome(h, Biome::Tundra);
                    }
                    touched += 1;
                }
                out.message = format!("{touched} tiles rise at {at}");
            }
            Power::LowerLand | Power::LakeBrush | Power::OceanBrush => {
                let amount = match power {
                    Power::OceanBrush => 200,
                    Power::LakeBrush => 90,
                    _ => 60,
                };
                let mut touched = 0;
                for h in at.spiral(2) {
                    let Some(_) = self.tile(h) else { continue };
                    if let Some(tile) = self.tile_mut(h) {
                        let mut lowered = tile.elevation as i32 - amount;
                        if power == Power::OceanBrush {
                            // The brush floods the tile outright, however high it stands.
                            lowered = lowered.min(-80);
                        }
                        tile.elevation = lowered.clamp(-320, 900) as i16;
                        if tile.elevation < 0 {
                            tile.biome = if tile.elevation < -60 {
                                Biome::Ocean
                            } else {
                                Biome::Shallow
                            };
                            tile.trees = 0;
                            tile.owner = None;
                        }
                        tile.refresh_fertility();
                    }
                    touched += 1;
                }
                out.message = format!("{touched} tiles sink at {at}");
            }
            Power::ForestBrush => {
                for h in at.spiral(2) {
                    if let Some(t) = self.tile(h).copied() {
                        if t.is_land() {
                            self.set_biome(h, Biome::Forest);
                            if let Some(tile) = self.tile_mut(h) {
                                tile.trees = 3;
                            }
                        }
                    }
                }
                out.message = format!("A forest grows at {at}");
            }
            Power::FertileSoil => {
                for h in at.spiral(2) {
                    if let Some(t) = self.tile(h).copied() {
                        if t.is_land() {
                            if matches!(t.biome, Biome::Desert | Biome::Ash | Biome::Wasteland) {
                                self.set_biome(h, Biome::Grass);
                            }
                            if let Some(tile) = self.tile_mut(h) {
                                tile.fertile = 100;
                                tile.scorch = 0;
                            }
                        }
                    }
                }
                out.message = format!("The soil at {at} becomes fertile");
            }
            Power::DesertBrush => {
                for h in at.spiral(2) {
                    if let Some(t) = self.tile(h).copied() {
                        if t.is_land() {
                            self.set_biome(h, Biome::Desert);
                            if let Some(tile) = self.tile_mut(h) {
                                tile.trees = 0;
                            }
                        }
                    }
                }
                out.message = format!("The land at {at} turns to desert");
            }
            Power::SnowBrush => {
                for h in at.spiral(2) {
                    if let Some(t) = self.tile(h).copied() {
                        if t.is_sea() {
                            self.set_biome(h, Biome::Ice);
                        } else if t.is_land() {
                            self.set_biome(h, Biome::Snow);
                        }
                    }
                }
                out.message = format!("Frost covers {at}");
            }
            Power::LavaBrush => {
                for h in at.spiral(1) {
                    if let Some(t) = self.tile(h).copied() {
                        if t.is_land() {
                            if let Some(tile) = self.tile_mut(h) {
                                tile.lava = 3;
                                tile.trees = 0;
                                tile.scorch = (tile.scorch + 2).min(9);
                            }
                        }
                    }
                }
                out.message = format!("Lava pours out at {at}");
            }
            Power::SmoothLand => {
                // Flatten: average the elevation of the neighbourhood and pull
                // every tile toward it.
                let cells = at.spiral(2);
                let mut sum = 0i32;
                let mut n = 0i32;
                for h in &cells {
                    if let Some(t) = self.tile(*h) {
                        sum += t.elevation as i32;
                        n += 1;
                    }
                }
                let avg = if n > 0 { sum / n } else { 0 };
                for h in cells {
                    if let Some(tile) = self.tile_mut(h) {
                        let diff = avg - tile.elevation as i32;
                        tile.elevation = (tile.elevation as i32 + diff / 2).clamp(-320, 900) as i16;
                        if tile.elevation > 0 && tile.biome.is_water() {
                            tile.biome = Biome::Beach;
                        }
                        if tile.elevation < 0 && !tile.biome.is_water() {
                            tile.biome = Biome::Shallow;
                        }
                        tile.refresh_fertility();
                    }
                }
                out.message = format!("The land around {at} is smoothed");
            }
            // --- civilization --------------------------------------------------
            Power::Inspiration => {
                let vid = self.village_owner_near(at, 2);
                match vid {
                    Some(v) if self.village(v).map(|v| v.kingdom.is_none()).unwrap_or(false) => {
                        if self.found_kingdom(v, self.village(v).unwrap().race).is_some() {
                            out.message = format!("{} declares independence", self.village(v).unwrap().name);
                        }
                    }
                    Some(v) => {
                        self.rebel(v);
                        out.message = format!("{} rises in rebellion", self.village(v).unwrap().name);
                    }
                    None => {
                        out.ok = false;
                        out.message = format!("no village at {at}");
                    }
                }
            }
            Power::Friendship => {
                let kid = self.tile(at).and_then(|t| t.owner).and_then(|v| self.village(v)).and_then(|v| v.kingdom);
                match kid {
                    Some(k) => {
                        let enemies: Vec<u32> = self
                            .kingdom(k)
                            .map(|k| k.wars.iter().map(|w| w.enemy).collect())
                            .unwrap_or_default();
                        for e in &enemies {
                            self.make_peace(k, *e);
                        }
                        out.units_affected = enemies.len() as u32;
                        out.message = format!("{} makes peace with {} realm(s)", self.kingdom(k).unwrap().name, enemies.len());
                    }
                    None => {
                        out.ok = false;
                        out.message = format!("no kingdom at {at}");
                    }
                }
            }
            Power::Spite => {
                let kid = self.tile(at).and_then(|t| t.owner).and_then(|v| self.village(v)).and_then(|v| v.kingdom);
                match kid {
                    Some(k) => {
                        let enemy = self
                            .kingdom_ids()
                            .into_iter()
                            .filter(|o| *o != k)
                            .min_by_key(|o| {
                                self.kingdom(k)
                                    .map(|kk| kk.opinion(*o))
                                    .unwrap_or(0)
                            });
                        match enemy {
                            Some(e) => {
                                self.declare_war(k, e);
                                out.message = format!(
                                    "{} is goaded into war with {}",
                                    self.kingdom(k).map(|x| x.name.clone()).unwrap_or_default(),
                                    self.kingdom(e).map(|x| x.name.clone()).unwrap_or_default()
                                );
                            }
                            None => {
                                out.ok = false;
                                out.message = "there is nobody to fight".to_string();
                            }
                        }
                    }
                    None => {
                        out.ok = false;
                        out.message = format!("no kingdom at {at}");
                    }
                }
            }
            Power::WhisperOfWar => {
                let ids = self.kingdom_ids();
                if ids.len() < 2 {
                    out.ok = false;
                    out.message = "fewer than two kingdoms exist".to_string();
                } else {
                    let a = ids[0];
                    let b = ids[1];
                    if let Some(k) = self.kingdom_mut(a) {
                        k.add_opinion(b, -50);
                    }
                    if let Some(k) = self.kingdom_mut(b) {
                        k.add_opinion(a, -50);
                    }
                    out.message = "whispers poison two kingdoms' opinion of each other".to_string();
                }
            }
            // --- summons --------------------------------------------------------
            Power::SummonDragon | Power::SummonUfo | Power::SummonCrabzilla => {
                let race = match power {
                    Power::SummonDragon => Race::Dragon,
                    Power::SummonUfo => Race::Ufo,
                    _ => Race::Crabzilla,
                };
                match self.spawn_unit(race, at, UnitKind::Monster) {
                    Some(_) => {
                        self.chronicle(
                            EventKind::Boss,
                            format!("A {} appears at {at}", race.name()),
                        );
                        out.units_affected = 1;
                        out.message = format!("A {} appears at {at}", race.name());
                    }
                    None => {
                        out.ok = false;
                        out.message = format!("a {} cannot stand at {at}", race.name());
                    }
                }
            }
        }
        let after = self.unit_count();
        if out.killed == 0 && before > after {
            out.killed = (before - after) as u32;
        }
        self.stats.powers_cast += 1;
        self.log_power(format!("{}: {}", power.name(), out.message));
        out
    }

    /// The village owning a tile, or the nearest village centre within `radius`.
    pub fn village_owner_near(&self, at: Hex, radius: i32) -> Option<u32> {
        if let Some(owner) = self.tile(at).and_then(|t| t.owner) {
            return Some(owner);
        }
        self.villages
            .iter()
            .filter(|v| v.alive && v.center.distance(at) <= radius)
            .min_by_key(|v| v.center.distance(at))
            .map(|v| v.id)
    }

    /// Earthquake: terrain cracks, buildings shake down, lava sometimes opens.
    pub fn earthquake(&mut self, at: Hex, radius: i32) -> u32 {
        let mut killed = 0;
        for h in at.spiral(radius) {
            let d = h.distance(at);
            let Some(t) = self.tile(h).copied() else { continue };
            // Ground moves.
            let jitter = self.rng.range(-24, 24);
            let opens_lava = self.rng.chance(0.05);
            if let Some(tile) = self.tile_mut(h) {
                tile.elevation = (tile.elevation as i32 + jitter).clamp(-320, 900) as i16;
                if opens_lava {
                    tile.lava = tile.lava.max(1);
                }
                tile.refresh_fertility();
            }
            // Creatures take a beating.
            for id in self.units_in_radius(h, 0) {
                if self.damage_unit(id, 26 - d * 2, None) {
                    killed += 1;
                }
            }
            // Buildings come down.
            if let Some(v) = t.owner {
                if self.rng.chance(0.12) {
                    self.village_building_damage(v, 1);
                }
            }
        }
        self.stats.disasters += 1;
        killed
    }

    /// Freeze an area: water becomes ice, land becomes permafrost.
    pub fn freeze_area(&mut self, at: Hex, radius: i32) -> u32 {
        let mut frozen = 0;
        for h in at.spiral(radius) {
            if let Some(t) = self.tile(h).copied() {
                let new_biome = if t.is_sea() {
                    Some(Biome::Ice)
                } else if t.is_land() && t.biome != Biome::Mountain {
                    Some(Biome::Permafrost)
                } else {
                    None
                };
                if let Some(b) = new_biome {
                    self.set_biome(h, b);
                    if let Some(tile) = self.tile_mut(h) {
                        tile.fire = 0;
                    }
                    frozen += 1;
                }
            }
        }
        frozen
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::units::UnitKind as K;
    use crate::village::FOUNDING_POP;
    use crate::worldgen::{GenParams, WorldType};

    fn world() -> World {
        World::generate(GenParams::new(36, 28, 42, WorldType::Continents))
    }

    fn village(w: &mut World) -> u32 {
        let spot = w.random_land_tile().unwrap();
        let mut founders = Vec::new();
        for _ in 0..FOUNDING_POP {
            if let Some(id) = w.spawn_unit(Race::Human, spot, K::Civilian) {
                founders.push(id);
            }
        }
        w.found_village(spot, Race::Human, &founders, None, None).unwrap()
    }

    #[test]
    fn power_names_round_trip() {
        for p in Power::ALL {
            assert_eq!(Power::parse(p.name()), Some(p), "{p:?}");
            assert!(!p.description().is_empty());
        }
        assert_eq!(Power::parse("zombie_infection"), Some(Power::ZombieInfection));
        assert_eq!(Power::parse("BLACK HOLE"), Some(Power::BlackHole));
        assert_eq!(Power::parse("nonsense"), None);
    }

    #[test]
    fn lightning_kills_or_burns_but_always_reports() {
        let mut w = world();
        let h = w.random_land_tile().unwrap();
        let id = w.spawn_unit(Race::Human, h, K::Civilian).unwrap();
        let out = w.cast(Power::Lightning, h);
        assert!(out.ok);
        assert_eq!(out.units_affected, 1);
        assert!(w.unit(id).map(|u| u.hp < u.max_hp).unwrap_or(true));
        // Casting off the map fails cleanly instead of panicking.
        let off = w.cast(Power::Lightning, Hex::from_offset(999, 999));
        assert!(!off.ok);
    }

    #[test]
    fn nuke_wipes_out_a_village_and_counts_the_dead() {
        let mut w = world();
        let vid = village(&mut w);
        let center = w.village(vid).unwrap().center;
        let out = w.cast(Power::Nuke, center);
        assert!(out.ok);
        assert!(out.killed >= 3, "a nuke should kill: {}", out.message);
        assert!(w.village(vid).map(|v| v.pop).unwrap_or(0) < FOUNDING_POP);
    }

    #[test]
    fn blessings_and_curses_are_mutually_exclusive() {
        let mut w = world();
        let h = w.random_land_tile().unwrap();
        let id = w.spawn_unit(Race::Human, h, K::Civilian).unwrap();
        w.cast(Power::Blessing, h);
        assert!(w.unit(id).unwrap().has_trait(traits::BLESSED));
        w.cast(Power::Curse, h);
        let u = w.unit(id).unwrap();
        assert!(u.has_trait(traits::CURSED));
        assert!(!u.has_trait(traits::BLESSED));
    }

    #[test]
    fn divine_light_cures_infection_and_madness() {
        let mut w = world();
        let h = w.random_land_tile().unwrap();
        let id = w.spawn_unit(Race::Human, h, K::Civilian).unwrap();
        w.cast(Power::ZombieInfection, h);
        w.cast(Power::Madness, h);
        assert!(w.unit(id).unwrap().has_trait(traits::INFECTED));
        w.cast(Power::DivineLight, h);
        let u = w.unit(id).unwrap();
        assert!(!u.has_trait(traits::INFECTED));
        assert!(!u.has_status(StatusKind::Mad));
    }

    #[test]
    fn dispel_clears_the_sky() {
        let mut w = world();
        let h = w.random_land_tile().unwrap();
        w.cast(Power::AcidRain, h);
        w.cast(Power::FireRain, h);
        assert!(!w.effects.clouds.is_empty());
        w.cast(Power::Dispel, h);
        assert!(w.effects.clouds.is_empty());
    }

    #[test]
    fn terrain_brushes_change_the_map_and_keep_it_sane() {
        let mut w = world();
        let sea = w
            .iter_hexes()
            .into_iter()
            .find(|h| w.tile(*h).map(|t| t.biome == Biome::Ocean).unwrap_or(false))
            .unwrap();
        w.cast(Power::RaiseLand, sea);
        w.cast(Power::RaiseLand, sea);
        w.cast(Power::RaiseLand, sea);
        assert!(w.tile(sea).unwrap().elevation > -320);
        let land = w.random_land_tile().unwrap();
        w.cast(Power::MountainBrush, land);
        assert_eq!(w.tile(land).unwrap().biome, Biome::Mountain);
        w.cast(Power::OceanBrush, land);
        assert!(w.tile(land).unwrap().is_water());
        let fresh = w.random_land_tile().unwrap();
        w.cast(Power::ForestBrush, fresh);
        assert_eq!(w.tile(fresh).unwrap().biome, Biome::Forest);
        w.cast(Power::FertileSoil, fresh);
        assert_eq!(w.tile(fresh).unwrap().fertile, 100);
        w.cast(Power::LavaBrush, fresh);
        assert!(w.tile(fresh).unwrap().lava > 0);
        // Nothing ever leaves the elevation range.
        assert!(w.tiles.iter().all(|t| (-320..=900).contains(&t.elevation)));
    }

    #[test]
    fn inspiration_turns_a_village_into_a_kingdom_and_then_a_rebellion() {
        let mut w = world();
        let vid = village(&mut w);
        let center = w.village(vid).unwrap().center;
        w.cast(Power::Inspiration, center);
        assert!(w.village(vid).unwrap().kingdom.is_some(), "first use founds a kingdom");
        let kid = w.village(vid).unwrap().kingdom.unwrap();
        w.cast(Power::Inspiration, center);
        let new_kid = w.village(vid).unwrap().kingdom.unwrap();
        assert_ne!(new_kid, kid, "second use rebels");
        assert!(w.kingdom(kid).unwrap().at_war_with(new_kid));
    }

    #[test]
    fn friendship_ends_wars_and_spite_starts_them() {
        let mut w = world();
        let a = village(&mut w);
        let b = village(&mut w);
        let ka = w.found_kingdom(a, Race::Human).unwrap();
        let kb = w.found_kingdom(b, Race::Human).unwrap();
        w.declare_war(ka, kb);
        let center_a = w.village(a).unwrap().center;
        w.cast(Power::Friendship, center_a);
        assert!(!w.kingdom(ka).unwrap().at_war_with(kb));
        w.cast(Power::Spite, center_a);
        assert!(!w.kingdom(ka).unwrap().wars.is_empty());
    }

    #[test]
    fn clone_rain_copies_a_creature() {
        let mut w = world();
        let h = w.random_land_tile().unwrap();
        w.spawn_unit(Race::Dwarf, h, K::Soldier);
        let before = w.unit_count();
        let out = w.cast(Power::CloneRain, h);
        assert!(out.ok);
        assert!(w.unit_count() > before + 5, "clones should appear: {}", out.message);
    }

    #[test]
    fn magnet_moves_a_creature_to_the_cursor() {
        let mut w = world();
        let h = w.random_land_tile().unwrap();
        let id = w.spawn_unit(Race::Human, h, K::Civilian).unwrap();
        let far = w.random_land_tile().unwrap();
        let out = w.cast(Power::Magnet, far);
        assert!(out.ok);
        assert_eq!(w.unit(id).unwrap().pos, far);
    }

    #[test]
    fn summons_create_bosses_who_then_actually_hurt_things() {
        let mut w = world();
        let h = w.random_land_tile().unwrap();
        let out = w.cast(Power::SummonCrabzilla, h);
        assert!(out.ok, "{}", out.message);
        let kraken = w
            .units
            .iter()
            .find(|u| u.alive && u.race == Race::Crabzilla)
            .map(|u| u.id)
            .unwrap();
        // A village next door should suffer.
        let vid = village(&mut w);
        let center = w.village(vid).unwrap().center;
        if let Some(u) = w.unit_mut(kraken) {
            u.pos = center;
        }
        for _ in 0..200 {
            w.step();
        }
        assert!(
            w.village(vid).map(|v| v.pop).unwrap_or(0) < FOUNDING_POP
                || w.village(vid).is_none(),
            "Crabzilla should flatten a village"
        );
    }

    #[test]
    fn every_power_can_be_cast_anywhere_without_panicking() {
        let mut w = world();
        let spots = [
            Hex::from_offset(0, 0),
            Hex::from_offset(5, 5),
            Hex::from_offset(20, 15),
        ];
        for p in Power::ALL {
            for s in spots {
                let out = w.cast(p, s);
                assert_eq!(out.power, p);
            }
            w.step();
        }
        assert!(w.stats.powers_cast >= Power::ALL.len() as u64);
    }
}
