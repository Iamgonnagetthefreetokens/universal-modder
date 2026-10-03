//! Save/load codec for the whole world, written by hand so the crate stays
//! dependency-free.
//!
//! The format is little-endian and versioned. A save round-trips exactly: loading
//! a save and stepping it reproduces the same `state_hash()` as the original
//! world stepping the same number of ticks, which the tests check.

use std::collections::VecDeque;
use std::fmt;
use std::io::Write;

use crate::ages::{Age, AgeState};
use crate::disaster::{BlackHole, Cloud, CloudKind, Effects, Meteor, Tornado, Volcano};
use crate::hex::Hex;
use crate::kingdom::{Kingdom, War};
use crate::races::Race;
use crate::terrain::{Biome, Tile};
use crate::units::{ItemKind, Resource, StatusKind, Unit, UnitKind, UnitState};
use crate::village::{Building, BuildingKind, Village};
use crate::world::{Event, EventKind, World, WorldStats};
use crate::worldgen::WorldType;

/// File magic: "WORLDFORGE" plus a format byte.
pub const MAGIC: [u8; 11] = *b"WORLDFORGE\x01";
/// Current save version.
pub const VERSION: u16 = 1;

/// Everything that can go wrong reading a save.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SaveError {
    BadMagic,
    BadVersion(u16),
    Truncated,
    BadEnum(&'static str, u8),
    BadBool(u8),
    TrailingBytes(usize),
}

impl fmt::Display for SaveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SaveError::BadMagic => write!(f, "not a worldforge save"),
            SaveError::BadVersion(v) => write!(f, "unsupported save version {v}"),
            SaveError::Truncated => write!(f, "save ended early"),
            SaveError::BadEnum(what, id) => write!(f, "unknown {what} id {id}"),
            SaveError::BadBool(v) => write!(f, "invalid boolean tag {v}"),
            SaveError::TrailingBytes(n) => write!(f, "{n} trailing bytes after the save"),
        }
    }
}

impl std::error::Error for SaveError {}

// ---------------------------------------------------------------------------
// enum tables
// ---------------------------------------------------------------------------

fn biomes() -> &'static [Biome] {
    &[
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
    ]
}

fn unit_kinds() -> &'static [UnitKind] {
    &[
        UnitKind::Civilian,
        UnitKind::Leader,
        UnitKind::King,
        UnitKind::Soldier,
        UnitKind::Animal,
        UnitKind::Monster,
    ]
}

fn unit_states() -> &'static [UnitState] {
    &[
        UnitState::Idle,
        UnitState::Wander,
        UnitState::Sleep,
        UnitState::Graze,
        UnitState::Hunt,
        UnitState::Flee,
        UnitState::Gather,
        UnitState::Deliver,
        UnitState::Build,
        UnitState::Patrol,
        UnitState::March,
        UnitState::Attack,
        UnitState::Rampage,
        UnitState::Follow,
        UnitState::Sail,
    ]
}

fn item_kinds() -> &'static [ItemKind] {
    &[
        ItemKind::None,
        ItemKind::Sword,
        ItemKind::Bow,
        ItemKind::Spear,
        ItemKind::Axe,
        ItemKind::Hammer,
        ItemKind::Staff,
        ItemKind::Scythe,
        ItemKind::Armor,
        ItemKind::Shield,
        ItemKind::HealthPotion,
        ItemKind::Bomb,
        ItemKind::FireBomb,
        ItemKind::AcidBomb,
        ItemKind::TeleportScroll,
    ]
}

fn resources() -> &'static [Resource] {
    &[
        Resource::Wood,
        Resource::Stone,
        Resource::Ore,
        Resource::Gold,
        Resource::Food,
    ]
}

fn building_kinds() -> &'static [BuildingKind] {
    &[
        BuildingKind::Fireplace,
        BuildingKind::TownHall,
        BuildingKind::House,
        BuildingKind::Farm,
        BuildingKind::Mine,
        BuildingKind::Sawmill,
        BuildingKind::Barracks,
        BuildingKind::Dock,
        BuildingKind::Temple,
        BuildingKind::Tower,
        BuildingKind::Well,
        BuildingKind::Statue,
        BuildingKind::Wall,
    ]
}

fn cloud_kinds() -> &'static [CloudKind] {
    &[
        CloudKind::Rain,
        CloudKind::AcidRain,
        CloudKind::Madness,
        CloudKind::Blood,
        CloudKind::Spores,
        CloudKind::Blessing,
        CloudKind::Curse,
        CloudKind::Fire,
        CloudKind::AlienMold,
        CloudKind::Love,
    ]
}

fn ages() -> &'static [Age] {
    &[
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
    ]
}

fn world_types() -> &'static [WorldType] {
    &[
        WorldType::Continents,
        WorldType::Archipelago,
        WorldType::Pangaea,
        WorldType::Highlands,
        WorldType::Lakes,
        WorldType::Desert,
        WorldType::Frozen,
    ]
}

fn event_kinds() -> &'static [EventKind] {
    &[
        EventKind::Age,
        EventKind::VillageFounded,
        EventKind::KingdomFounded,
        EventKind::War,
        EventKind::Peace,
        EventKind::Rebellion,
        EventKind::CityCaptured,
        EventKind::KingdomFallen,
        EventKind::Disaster,
        EventKind::Power,
        EventKind::Birth,
        EventKind::Death,
        EventKind::Boss,
        EventKind::Discovery,
        EventKind::Alliance,
    ]
}

fn index_of<T: PartialEq>(table: &[T], value: &T) -> u8 {
    table.iter().position(|v| v == value).unwrap_or(0) as u8
}

fn pick<T: Copy>(table: &[T], id: u8, what: &'static str) -> Result<T, SaveError> {
    table
        .get(id as usize)
        .copied()
        .ok_or(SaveError::BadEnum(what, id))
}

// ---------------------------------------------------------------------------
// byte plumbing
// ---------------------------------------------------------------------------

struct Writer {
    out: Vec<u8>,
}

impl Writer {
    fn new() -> Writer {
        Writer { out: Vec::new() }
    }

    fn u8(&mut self, v: u8) {
        self.out.push(v);
    }

    fn bool(&mut self, v: bool) {
        self.out.push(if v { 1 } else { 0 });
    }

    fn u16(&mut self, v: u16) {
        self.out.extend_from_slice(&v.to_le_bytes());
    }

    fn i16(&mut self, v: i16) {
        self.out.extend_from_slice(&v.to_le_bytes());
    }

    fn u32(&mut self, v: u32) {
        self.out.extend_from_slice(&v.to_le_bytes());
    }

    fn i32(&mut self, v: i32) {
        self.out.extend_from_slice(&v.to_le_bytes());
    }

    fn u64(&mut self, v: u64) {
        self.out.extend_from_slice(&v.to_le_bytes());
    }

    fn f32(&mut self, v: f32) {
        // Canonicalise NaNs so a save is byte-stable.
        self.out.extend_from_slice(&v.to_bits().to_le_bytes());
    }

    fn usize(&mut self, v: usize) {
        self.u32(v as u32);
    }

    fn opt_u32(&mut self, v: Option<u32>) {
        match v {
            Some(x) => {
                self.bool(true);
                self.u32(x);
            }
            None => self.bool(false),
        }
    }

    fn str(&mut self, s: &str) {
        self.usize(s.len());
        self.out.extend_from_slice(s.as_bytes());
    }

    fn hex(&mut self, h: Hex) {
        self.i32(h.q);
        self.i32(h.r);
    }

    fn opt_hex(&mut self, h: Option<Hex>) {
        match h {
            Some(x) => {
                self.bool(true);
                self.hex(x);
            }
            None => self.bool(false),
        }
    }

    fn list<T>(&mut self, xs: &[T], mut f: impl FnMut(&mut Writer, &T)) {
        self.usize(xs.len());
        for x in xs {
            f(self, x);
        }
    }
}

struct Reader<'a> {
    data: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn new(data: &'a [u8]) -> Reader<'a> {
        Reader { data, at: 0 }
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], SaveError> {
        let end = self.at.checked_add(n).ok_or(SaveError::Truncated)?;
        if end > self.data.len() {
            return Err(SaveError::Truncated);
        }
        let slice = &self.data[self.at..end];
        self.at = end;
        Ok(slice)
    }

    fn u8(&mut self) -> Result<u8, SaveError> {
        Ok(self.take(1)?[0])
    }

    fn bool(&mut self) -> Result<bool, SaveError> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            v => Err(SaveError::BadBool(v)),
        }
    }

    fn u16(&mut self) -> Result<u16, SaveError> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }

    fn i16(&mut self) -> Result<i16, SaveError> {
        Ok(i16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }

    fn u32(&mut self) -> Result<u32, SaveError> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }

    fn i32(&mut self) -> Result<i32, SaveError> {
        Ok(i32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }

    fn u64(&mut self) -> Result<u64, SaveError> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }

    fn f32(&mut self) -> Result<f32, SaveError> {
        Ok(f32::from_bits(self.u32()?))
    }

    fn usize(&mut self) -> Result<usize, SaveError> {
        Ok(self.u32()? as usize)
    }

    fn opt_u32(&mut self) -> Result<Option<u32>, SaveError> {
        if self.bool()? {
            Ok(Some(self.u32()?))
        } else {
            Ok(None)
        }
    }

    fn str(&mut self) -> Result<String, SaveError> {
        let n = self.usize()?;
        let bytes = self.take(n)?;
        String::from_utf8(bytes.to_vec()).map_err(|_| SaveError::Truncated)
    }

    fn hex(&mut self) -> Result<Hex, SaveError> {
        Ok(Hex {
            q: self.i32()?,
            r: self.i32()?,
        })
    }

    fn opt_hex(&mut self) -> Result<Option<Hex>, SaveError> {
        if self.bool()? {
            Ok(Some(self.hex()?))
        } else {
            Ok(None)
        }
    }

    fn vec<T>(
        &mut self,
        mut f: impl FnMut(&mut Reader<'a>) -> Result<T, SaveError>,
    ) -> Result<Vec<T>, SaveError> {
        let n = self.usize()?;
        let mut out = Vec::with_capacity(n.min(1 << 20));
        for _ in 0..n {
            out.push(f(self)?);
        }
        Ok(out)
    }
}

// ---------------------------------------------------------------------------
// per-type codecs
// ---------------------------------------------------------------------------

fn write_tile(w: &mut Writer, t: &Tile) {
    w.i16(t.elevation);
    w.u8(index_of(biomes(), &t.biome));
    w.u8(t.trees);
    w.u8(t.ore);
    w.u8(t.stone);
    w.u8(t.fertile);
    w.u8(t.fire);
    w.u8(t.lava);
    w.u8(t.scorch);
    w.bool(t.river);
    w.u8(t.road);
    w.opt_u32(t.owner);
}

#[allow(clippy::field_reassign_with_default)] // decoded field by field
fn read_tile(r: &mut Reader<'_>) -> Result<Tile, SaveError> {
    let mut t = Tile::default();
    t.elevation = r.i16()?;
    t.biome = pick(biomes(), r.u8()?, "biome")?;
    t.trees = r.u8()?;
    t.ore = r.u8()?;
    t.stone = r.u8()?;
    t.fertile = r.u8()?;
    t.fire = r.u8()?;
    t.lava = r.u8()?;
    t.scorch = r.u8()?;
    t.river = r.bool()?;
    t.road = r.u8()?;
    t.owner = r.opt_u32()?;
    Ok(t)
}

fn write_unit(w: &mut Writer, u: &Unit) {
    w.u32(u.id);
    w.bool(u.alive);
    w.u8(index_of(Race::ALL.as_slice(), &u.race));
    w.u8(index_of(unit_kinds(), &u.kind));
    w.str(&u.name);
    w.hex(u.pos);
    w.i32(u.hp);
    w.i32(u.max_hp);
    w.i32(u.damage);
    w.i32(u.armor);
    w.i32(u.speed);
    w.f32(u.accuracy);
    w.f32(u.dodge);
    w.u8(u.attack_cooldown);
    w.u8(u.cooldown);
    w.u64(u.traits);
    let statuses = u.statuses.to_vec();
    w.list(&statuses, |w, s| w.u16(*s));
    w.u16(u.age);
    w.u8(u.level);
    w.u32(u.xp);
    w.u32(u.kills);
    w.i32(u.move_points);
    w.u8(index_of(unit_states(), &u.state));
    w.opt_u32(u.village);
    w.opt_u32(u.kingdom);
    w.opt_u32(u.enemy);
    w.opt_hex(u.destination);
    w.hex(u.home);
    let path = u.path.clone();
    w.list(&path, |w, h| w.hex(*h));
    w.usize(u.path_i);
    match u.carry {
        Some((res, amount)) => {
            w.bool(true);
            w.u8(index_of(resources(), &res));
            w.u8(amount);
        }
        None => w.bool(false),
    }
    w.u8(index_of(item_kinds(), &u.item.kind));
    w.u8(u.item.tier);
    w.u8(u.hunger);
    w.bool(u.sailing);
    w.u16(u.siege_progress);
}

#[allow(clippy::field_reassign_with_default)] // decoded field by field
fn read_unit(r: &mut Reader<'_>) -> Result<Unit, SaveError> {
    let id = r.u32()?;
    let alive = r.bool()?;
    let race = pick(Race::ALL.as_slice(), r.u8()?, "race")?;
    let mut u = Unit::new(id, race, Hex::default(), UnitKind::Civilian);
    u.id = id;
    u.alive = alive;
    u.race = race;
    u.kind = pick(unit_kinds(), r.u8()?, "unit kind")?;
    u.name = r.str()?;
    u.pos = r.hex()?;
    u.hp = r.i32()?;
    u.max_hp = r.i32()?;
    u.damage = r.i32()?;
    u.armor = r.i32()?;
    u.speed = r.i32()?;
    u.accuracy = r.f32()?;
    u.dodge = r.f32()?;
    u.attack_cooldown = r.u8()?;
    u.cooldown = r.u8()?;
    u.traits = r.u64()?;
    let statuses = r.vec(|r| r.u16())?;
    if statuses.len() != StatusKind::COUNT {
        return Err(SaveError::Truncated);
    }
    u.statuses.copy_from_slice(&statuses);
    u.age = r.u16()?;
    u.level = r.u8()?;
    u.xp = r.u32()?;
    u.kills = r.u32()?;
    u.move_points = r.i32()?;
    u.state = pick(unit_states(), r.u8()?, "unit state")?;
    u.village = r.opt_u32()?;
    u.kingdom = r.opt_u32()?;
    u.enemy = r.opt_u32()?;
    u.destination = r.opt_hex()?;
    u.home = r.hex()?;
    u.path = r.vec(|r| r.hex())?;
    u.path_i = r.usize()?;
    u.carry = if r.bool()? {
        let res = pick(resources(), r.u8()?, "resource")?;
        Some((res, r.u8()?))
    } else {
        None
    };
    u.item = crate::units::Item::new(pick(item_kinds(), r.u8()?, "item")?, r.u8()?);
    u.hunger = r.u8()?;
    u.sailing = r.bool()?;
    u.siege_progress = r.u16()?;
    Ok(u)
}

fn write_building(w: &mut Writer, b: &Building) {
    w.u8(index_of(building_kinds(), &b.kind));
    w.hex(b.pos);
    w.u8(b.level);
    w.bool(b.complete);
    w.u16(b.progress);
}

fn read_building(r: &mut Reader<'_>) -> Result<Building, SaveError> {
    Ok(Building {
        kind: pick(building_kinds(), r.u8()?, "building")?,
        pos: r.hex()?,
        level: r.u8()?,
        complete: r.bool()?,
        progress: r.u16()?,
    })
}

fn write_village(w: &mut Writer, v: &Village) {
    w.u32(v.id);
    w.bool(v.alive);
    w.str(&v.name);
    w.u8(index_of(Race::ALL.as_slice(), &v.race));
    w.hex(v.center);
    w.u32(v.pop);
    w.u16(v.houses);
    w.u8(v.town_hall_level);
    w.i32(v.food);
    w.i32(v.wood);
    w.i32(v.stone);
    w.i32(v.ore);
    w.i32(v.gold);
    w.list(&v.buildings, write_building);
    match &v.build_site {
        Some(b) => {
            w.bool(true);
            write_building(w, b);
        }
        None => w.bool(false),
    }
    w.opt_u32(v.kingdom);
    w.i16(v.loyalty);
    w.u32(v.founded_year);
    w.u32(v.births);
    w.u32(v.deaths);
    w.u16(v.soldiers);
    w.opt_u32(v.leader);
    w.bool(v.capital);
    w.u8(v.walls);
    w.i32(v.claim_radius);
    w.opt_u32(v.parent);
    w.u32(v.last_war_year);
    w.u16(v.boats);
    w.bool(v.starving);
    w.i32(v.siege);
    w.opt_u32(v.besieged_by);
}

#[allow(clippy::field_reassign_with_default)] // decoded field by field
fn read_village(r: &mut Reader<'_>) -> Result<Village, SaveError> {
    let mut v = Village::default();
    v.id = r.u32()?;
    v.alive = r.bool()?;
    v.name = r.str()?;
    v.race = pick(Race::ALL.as_slice(), r.u8()?, "race")?;
    v.center = r.hex()?;
    v.pop = r.u32()?;
    v.houses = r.u16()?;
    v.town_hall_level = r.u8()?;
    v.food = r.i32()?;
    v.wood = r.i32()?;
    v.stone = r.i32()?;
    v.ore = r.i32()?;
    v.gold = r.i32()?;
    v.buildings = r.vec(read_building)?;
    v.build_site = if r.bool()? {
        Some(read_building(r)?)
    } else {
        None
    };
    v.kingdom = r.opt_u32()?;
    v.loyalty = r.i16()?;
    v.founded_year = r.u32()?;
    v.births = r.u32()?;
    v.deaths = r.u32()?;
    v.soldiers = r.u16()?;
    v.leader = r.opt_u32()?;
    v.capital = r.bool()?;
    v.walls = r.u8()?;
    v.claim_radius = r.i32()?;
    v.parent = r.opt_u32()?;
    v.last_war_year = r.u32()?;
    v.boats = r.u16()?;
    v.starving = r.bool()?;
    v.siege = r.i32()?;
    v.besieged_by = r.opt_u32()?;
    Ok(v)
}

fn write_kingdom(w: &mut Writer, k: &Kingdom) {
    w.u32(k.id);
    w.bool(k.alive);
    w.str(&k.name);
    w.u8(index_of(Race::ALL.as_slice(), &k.race));
    w.u8(k.color.0);
    w.u8(k.color.1);
    w.u8(k.color.2);
    w.u32(k.capital);
    w.opt_u32(k.king);
    w.str(&k.king_name);
    w.u32(k.founded_year);
    w.list(&k.cities, |w, c| w.u32(*c));
    w.u32(k.population);
    w.list(&k.wars, |w, war| {
        w.u32(war.enemy);
        w.u32(war.start_year);
    });
    w.list(&k.allies, |w, a| w.u32(*a));
    w.list(&k.relations, |w, (id, op)| {
        w.u32(*id);
        w.i16(*op);
    });
    w.str(&k.motto);
    w.u32(k.culture);
    w.i16(k.aggression);
    w.u32(k.wars_won);
    w.u32(k.wars_lost);
    w.u32(k.cities_captured);
    w.u32(k.rebellions_survived);
}

#[allow(clippy::field_reassign_with_default)] // decoded field by field
fn read_kingdom(r: &mut Reader<'_>) -> Result<Kingdom, SaveError> {
    let mut k = Kingdom::default();
    k.id = r.u32()?;
    k.alive = r.bool()?;
    k.name = r.str()?;
    k.race = pick(Race::ALL.as_slice(), r.u8()?, "race")?;
    k.color = (r.u8()?, r.u8()?, r.u8()?);
    k.capital = r.u32()?;
    k.king = r.opt_u32()?;
    k.king_name = r.str()?;
    k.founded_year = r.u32()?;
    k.cities = r.vec(|r| r.u32())?;
    k.population = r.u32()?;
    k.wars = r.vec(|r| {
        Ok(War {
            enemy: r.u32()?,
            start_year: r.u32()?,
        })
    })?;
    k.allies = r.vec(|r| r.u32())?;
    k.relations = r.vec(|r| Ok((r.u32()?, r.i16()?)))?;
    k.motto = r.str()?;
    k.culture = r.u32()?;
    k.aggression = r.i16()?;
    k.wars_won = r.u32()?;
    k.wars_lost = r.u32()?;
    k.cities_captured = r.u32()?;
    k.rebellions_survived = r.u32()?;
    Ok(k)
}

fn write_effects(w: &mut Writer, e: &Effects) {
    w.list(&e.clouds, |w, c| {
        w.hex(c.pos);
        w.u8(index_of(cloud_kinds(), &c.kind));
        w.u16(c.ticks_left);
        w.i32(c.radius);
    });
    w.list(&e.meteors, |w, m| {
        w.hex(m.pos);
        w.hex(m.target);
        w.u16(m.ticks_left);
        w.i32(m.radius);
        w.i32(m.power);
        w.bool(m.crater);
    });
    w.list(&e.tornadoes, |w, t| {
        w.hex(t.pos);
        w.u32(t.dir as u32);
        w.u16(t.ticks_left);
        w.i32(t.power);
    });
    w.list(&e.volcanoes, |w, v| {
        w.hex(v.pos);
        w.u16(v.ticks_left);
        w.i32(v.power);
    });
    w.list(&e.black_holes, |w, b| {
        w.hex(b.pos);
        w.u16(b.ticks_left);
        w.i32(b.radius);
    });
}

fn read_effects(r: &mut Reader<'_>) -> Result<Effects, SaveError> {
    let clouds = r.vec(|r| {
        Ok(Cloud {
            pos: r.hex()?,
            kind: pick(cloud_kinds(), r.u8()?, "cloud")?,
            ticks_left: r.u16()?,
            radius: r.i32()?,
        })
    })?;
    let meteors = r.vec(|r| {
        Ok(Meteor {
            pos: r.hex()?,
            target: r.hex()?,
            ticks_left: r.u16()?,
            radius: r.i32()?,
            power: r.i32()?,
            crater: r.bool()?,
        })
    })?;
    let tornadoes = r.vec(|r| {
        Ok(Tornado {
            pos: r.hex()?,
            dir: r.u32()? as usize,
            ticks_left: r.u16()?,
            power: r.i32()?,
        })
    })?;
    let volcanoes = r.vec(|r| {
        Ok(Volcano {
            pos: r.hex()?,
            ticks_left: r.u16()?,
            power: r.i32()?,
        })
    })?;
    let black_holes = r.vec(|r| {
        Ok(BlackHole {
            pos: r.hex()?,
            ticks_left: r.u16()?,
            radius: r.i32()?,
        })
    })?;
    Ok(Effects {
        clouds,
        meteors,
        tornadoes,
        volcanoes,
        black_holes,
    })
}

fn write_stats(w: &mut Writer, s: &WorldStats) {
    for v in [
        s.units_spawned,
        s.births,
        s.deaths,
        s.kills,
        s.damage_dealt,
        s.villages_founded,
        s.kingdoms_founded,
        s.wars_declared,
        s.wars_ended,
        s.alliances,
        s.cities_captured,
        s.kingdoms_fallen,
        s.rebellions,
        s.buildings_built,
        s.trees_chopped,
        s.disasters,
        s.explosions,
        s.nukes_dropped,
        s.powers_cast,
        s.zombies_raised,
        s.age_changes,
        s.boats_built,
    ] {
        w.u64(v);
    }
}

#[allow(clippy::field_reassign_with_default)] // decoded field by field
fn read_stats(r: &mut Reader<'_>) -> Result<WorldStats, SaveError> {
    let mut s = WorldStats::default();
    s.units_spawned = r.u64()?;
    s.births = r.u64()?;
    s.deaths = r.u64()?;
    s.kills = r.u64()?;
    s.damage_dealt = r.u64()?;
    s.villages_founded = r.u64()?;
    s.kingdoms_founded = r.u64()?;
    s.wars_declared = r.u64()?;
    s.wars_ended = r.u64()?;
    s.alliances = r.u64()?;
    s.cities_captured = r.u64()?;
    s.kingdoms_fallen = r.u64()?;
    s.rebellions = r.u64()?;
    s.buildings_built = r.u64()?;
    s.trees_chopped = r.u64()?;
    s.disasters = r.u64()?;
    s.explosions = r.u64()?;
    s.nukes_dropped = r.u64()?;
    s.powers_cast = r.u64()?;
    s.zombies_raised = r.u64()?;
    s.age_changes = r.u64()?;
    s.boats_built = r.u64()?;
    Ok(s)
}

// ---------------------------------------------------------------------------
// public API
// ---------------------------------------------------------------------------

/// Serialise a whole world. The result is byte-for-byte deterministic.
pub fn encode(w: &World) -> Vec<u8> {
    let mut out = Writer::new();
    out.out.extend_from_slice(&MAGIC);
    out.u16(VERSION);
    out.u16(w.width);
    out.u16(w.height);
    out.u64(w.seed);
    out.u8(index_of(world_types(), &w.world_type));
    out.u8(w.land_ratio);
    out.u64(w.tick);
    out.u32(w.year);
    out.u64(w.rng.state());
    out.u64(w.rng.inc());
    out.list(&w.tiles, write_tile);
    out.list(&w.free_units, |w, id| w.u32(*id));
    out.list(&w.graveyard, |w, id| w.u32(*id));
    out.list(&w.free_villages, |w, id| w.u32(*id));
    out.list(&w.free_kingdoms, |w, id| w.u32(*id));
    out.list(&w.units, write_unit);
    out.list(&w.villages, write_village);
    out.list(&w.kingdoms, write_kingdom);
    // age
    out.u8(index_of(ages(), &w.age.age));
    out.u32(w.age.years_in_age);
    out.u32(w.age.duration_years);
    out.list(&w.age.history, |w, (age, year)| {
        w.u8(index_of(ages(), age));
        w.u32(*year);
    });
    write_effects(&mut out, &w.effects);
    // chronicle
    out.usize(w.chronicle.len());
    for e in &w.chronicle {
        out.u64(e.tick);
        out.u32(e.year);
        out.u8(index_of(event_kinds(), &e.kind));
        out.str(&e.text);
    }
    write_stats(&mut out, &w.stats);
    out.list(&w.pending_settlements, |w, (site, race, kingdom, parent)| {
        w.hex(*site);
        w.u8(index_of(Race::ALL.as_slice(), race));
        w.opt_u32(*kingdom);
        w.u32(*parent);
    });
    out.out
}

/// Parse a save produced by [`encode`].
pub fn decode(bytes: &[u8]) -> Result<World, SaveError> {
    let mut r = Reader::new(bytes);
    if r.take(MAGIC.len())? != MAGIC {
        return Err(SaveError::BadMagic);
    }
    let version = r.u16()?;
    if version != VERSION {
        return Err(SaveError::BadVersion(version));
    }
    let width = r.u16()?;
    let height = r.u16()?;
    let seed = r.u64()?;
    let world_type = pick(world_types(), r.u8()?, "world type")?;
    let land_ratio = r.u8()?;
    let tick = r.u64()?;
    let year = r.u32()?;
    let rng_state = r.u64()?;
    let rng_inc = r.u64()?;

    let mut w = World::empty(width, height, seed);
    w.world_type = world_type;
    w.land_ratio = land_ratio;
    w.tick = tick;
    w.year = year;
    w.rng = crate::rng::Rng::from_state(rng_state, rng_inc);
    w.tiles = r.vec(read_tile)?;
    if w.tiles.len() != width as usize * height as usize {
        return Err(SaveError::Truncated);
    }
    w.free_units = r.vec(|r| r.u32())?;
    w.graveyard = r.vec(|r| r.u32())?;
    w.free_villages = r.vec(|r| r.u32())?;
    w.free_kingdoms = r.vec(|r| r.u32())?;
    w.units = r.vec(read_unit)?;
    w.villages = r.vec(read_village)?;
    w.kingdoms = r.vec(read_kingdom)?;

    let age = pick(ages(), r.u8()?, "age")?;
    let years_in_age = r.u32()?;
    let duration_years = r.u32()?;
    let history = r.vec(|r| Ok((pick(ages(), r.u8()?, "age")?, r.u32()?)))?;
    w.age = AgeState {
        age,
        years_in_age,
        duration_years,
        history,
    };

    w.effects = read_effects(&mut r)?;
    let n = r.usize()?;
    let mut chronicle = VecDeque::new();
    for _ in 0..n {
        chronicle.push_back(Event {
            tick: r.u64()?,
            year: r.u32()?,
            kind: pick(event_kinds(), r.u8()?, "event")?,
            text: r.str()?,
        });
    }
    w.chronicle = chronicle;
    w.stats = read_stats(&mut r)?;
    w.pending_settlements = r.vec(|r| {
        Ok((
            r.hex()?,
            pick(Race::ALL.as_slice(), r.u8()?, "race")?,
            r.opt_u32()?,
            r.u32()?,
        ))
    })?;

    if r.at != bytes.len() {
        return Err(SaveError::TrailingBytes(bytes.len() - r.at));
    }
    Ok(w)
}

/// Write a save to disk.
pub fn save_to_file(path: &str, w: &World) -> std::io::Result<usize> {
    let bytes = encode(w);
    let mut f = std::fs::File::create(path)?;
    f.write_all(&bytes)?;
    f.flush()?;
    Ok(bytes.len())
}

/// Read a save from disk.
pub fn load_from_file(path: &str) -> std::io::Result<World> {
    let bytes = std::fs::read(path)?;
    decode(&bytes).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::powers::Power;
    use crate::worldgen::GenParams;

    fn busy_world() -> World {
        let mut w = World::generate(GenParams::new(60, 40, 99, WorldType::Continents));
        w.step_n(400);
        // A bit of everything: a war, a disaster, a power and a few units in flight.
        let at = w.random_land_tile().unwrap();
        w.spawn_unit(Race::Human, at, UnitKind::Soldier);
        w.spawn_unit(Race::Orc, at, UnitKind::Monster);
        w.spawn_unit(Race::Sheep, at, UnitKind::Animal);
        w.cast(Power::Nuke, at);
        w.cast(Power::Rain, at);
        w.step_n(40);
        w
    }

    #[test]
    fn round_trip_preserves_the_state_hash() {
        let w = busy_world();
        let bytes = encode(&w);
        let back = decode(&bytes).expect("save must decode");
        assert_eq!(back.state_hash(), w.state_hash());
        assert_eq!(back.summary(), w.summary());
        assert_eq!(back.units.len(), w.units.len());
        assert_eq!(back.villages.len(), w.villages.len());
        assert_eq!(back.chronicle.len(), w.chronicle.len());
        assert_eq!(back.effects, w.effects);
        assert_eq!(back.age, w.age);
        assert_eq!(back.rng.state(), w.rng.state());
        assert_eq!(back.rng.inc(), w.rng.inc());
    }

    #[test]
    fn a_loaded_world_keeps_evolving_identically() {
        let mut original = busy_world();
        let mut loaded = decode(&encode(&original)).unwrap();
        for _ in 0..300 {
            original.step();
            loaded.step();
            assert_eq!(loaded.state_hash(), original.state_hash());
        }
    }

    #[test]
    fn encoding_is_byte_stable() {
        let w = busy_world();
        assert_eq!(encode(&w), encode(&w));
    }

    #[test]
    fn corruption_is_reported_not_panicked_on() {
        let w = busy_world();
        let bytes = encode(&w);
        assert_eq!(decode(b"").unwrap_err(), SaveError::Truncated);
        assert_eq!(decode(b"NOT A SAVE AT ALL").unwrap_err(), SaveError::BadMagic);
        let mut bad_version = bytes.clone();
        bad_version[11] = 0xfe;
        bad_version[12] = 0xff;
        assert_eq!(decode(&bad_version).unwrap_err(), SaveError::BadVersion(0xfffe));
        // Truncations at a spread of offsets must all be handled.
        for cut in [0, 5, 11, 13, 20, 100, bytes.len() / 2, bytes.len() - 1] {
            let mut short = bytes.clone();
            short.truncate(cut);
            assert!(
                decode(&short).is_err(),
                "a {cut}-byte save should not decode"
            );
        }
        // Trailing junk is rejected rather than ignored.
        let mut long = bytes.clone();
        long.push(0);
        assert_eq!(decode(&long).unwrap_err(), SaveError::TrailingBytes(1));
    }

    #[test]
    fn files_round_trip_through_disk() {
        let w = busy_world();
        let mut path = std::env::temp_dir();
        path.push("worldforge_save_roundtrip.wfz");
        let path = path.to_string_lossy().to_string();
        let written = save_to_file(&path, &w).expect("save writes");
        assert!(written > 1000, "a full world save should be sizeable");
        let loaded = load_from_file(&path).expect("save loads");
        assert_eq!(loaded.state_hash(), w.state_hash());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn an_empty_world_round_trips_too() {
        let w = World::empty(16, 12, 7);
        let back = decode(&encode(&w)).unwrap();
        assert_eq!(back.state_hash(), w.state_hash());
        assert_eq!(back.width, 16);
        assert_eq!(back.height, 12);
    }
}
