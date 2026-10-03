//! The world: tiles, entities, the event chronicle, and the canonical state hash.
//!
//! `World` owns everything mutable in the simulation. Entities live in slot
//! vectors: a dead unit keeps its slot (marked `alive = false`) so ids stay stable
//! and deterministic, and the slot goes on a free list for reuse.
//!
//! Determinism is a hard requirement here: [`World::state_hash`] folds the entire
//! world into a `u64`, and the test suite asserts that two runs of the same seed
//! and script produce the same hash, and that a save/reload round trip preserves
//! it.

use crate::ages::AgeState;
use crate::disaster::Effects;
use crate::hex::Hex;
use crate::races::Race;
use crate::rng::Rng;
use crate::terrain::{Biome, Tile};
use crate::units::{StatusKind, Unit, UnitKind};
use crate::village::Village;
use crate::kingdom::Kingdom;
use crate::worldgen::{generate, GenParams, WorldType};
use std::collections::VecDeque;

/// Ticks in one in-world year. Ages, lifespans and funding dates use years.
pub const TICKS_PER_YEAR: u64 = 20;

/// How many chronicle entries are kept.
pub const CHRONICLE_CAP: usize = 1024;

/// The kind of thing that happened, for filtering and stats.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EventKind {
    Age,
    VillageFounded,
    KingdomFounded,
    War,
    Peace,
    Rebellion,
    CityCaptured,
    KingdomFallen,
    Disaster,
    Power,
    Birth,
    Death,
    Boss,
    Discovery,
    Alliance,
}

impl EventKind {
    pub fn name(self) -> &'static str {
        match self {
            EventKind::Age => "age",
            EventKind::VillageFounded => "village",
            EventKind::KingdomFounded => "kingdom",
            EventKind::War => "war",
            EventKind::Peace => "peace",
            EventKind::Rebellion => "rebellion",
            EventKind::CityCaptured => "capture",
            EventKind::KingdomFallen => "fall",
            EventKind::Disaster => "disaster",
            EventKind::Power => "power",
            EventKind::Birth => "birth",
            EventKind::Death => "death",
            EventKind::Boss => "boss",
            EventKind::Discovery => "discovery",
            EventKind::Alliance => "alliance",
        }
    }

    pub fn parse(s: &str) -> Option<EventKind> {
        let l = s.to_ascii_lowercase();
        [
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
        .into_iter()
        .find(|k| k.name().starts_with(&l))
    }
}

/// One line in the world's chronicle.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Event {
    pub tick: u64,
    pub year: u32,
    pub kind: EventKind,
    pub text: String,
}

/// Counters describing everything that has ever happened in this world.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WorldStats {
    pub units_spawned: u64,
    pub births: u64,
    pub deaths: u64,
    pub kills: u64,
    pub damage_dealt: u64,
    pub villages_founded: u64,
    pub kingdoms_founded: u64,
    pub wars_declared: u64,
    pub wars_ended: u64,
    pub alliances: u64,
    pub cities_captured: u64,
    pub kingdoms_fallen: u64,
    pub rebellions: u64,
    pub buildings_built: u64,
    pub trees_chopped: u64,
    pub disasters: u64,
    pub explosions: u64,
    pub nukes_dropped: u64,
    pub powers_cast: u64,
    pub zombies_raised: u64,
    pub age_changes: u64,
    pub boats_built: u64,
}

/// The simulation state.
#[derive(Clone, Debug)]
pub struct World {
    pub width: u16,
    pub height: u16,
    pub seed: u64,
    pub world_type: WorldType,
    pub land_ratio: u8,
    /// Ticks since the world was created.
    pub tick: u64,
    /// In-world year, `tick / TICKS_PER_YEAR`.
    pub year: u32,
    pub tiles: Vec<Tile>,
    pub rng: Rng,
    pub units: Vec<Unit>,
    pub villages: Vec<Village>,
    pub kingdoms: Vec<Kingdom>,
    pub free_units: Vec<u32>,
    /// Slots freed by deaths during this tick; folded into `free_units` before the
    /// next allocation so a dying unit cannot be replaced mid-tick.
    pub graveyard: Vec<u32>,
    pub free_villages: Vec<u32>,
    pub free_kingdoms: Vec<u32>,
    pub age: AgeState,
    pub effects: Effects,
    pub chronicle: VecDeque<Event>,
    pub stats: WorldStats,
    /// Settler bands walking to a site: `(site, race, kingdom, parent village)`.
    pub pending_settlements: Vec<(Hex, Race, Option<u32>, u32)>,
}

impl World {
    /// Generate a fresh world from generation parameters.
    pub fn generate(params: GenParams) -> World {
        let generated = generate(params);
        World {
            width: generated.width,
            height: generated.height,
            seed: params.seed,
            world_type: params.world_type,
            land_ratio: params.land_ratio,
            tick: 0,
            year: 0,
            tiles: generated.tiles,
            rng: Rng::new(params.seed),
            units: Vec::new(),
            villages: Vec::new(),
            kingdoms: Vec::new(),
            free_units: Vec::new(),
            graveyard: Vec::new(),
            free_villages: Vec::new(),
            free_kingdoms: Vec::new(),
            age: AgeState::new(params.seed),
            effects: Effects::default(),
            chronicle: VecDeque::new(),
            stats: WorldStats::default(),
            pending_settlements: Vec::new(),
        }
    }

    /// Convenience constructor: size in tiles, seed, world type.
    pub fn new(width: u16, height: u16, seed: u64, world_type: WorldType) -> World {
        World::generate(GenParams::new(width, height, seed, world_type))
    }

    /// A fully empty world of the given size (used to rebuild from a save).
    #[allow(clippy::too_many_arguments)]
    pub fn empty(width: u16, height: u16, seed: u64) -> World {
        World {
            width,
            height,
            seed,
            world_type: WorldType::Continents,
            land_ratio: 45,
            tick: 0,
            year: 0,
            tiles: vec![Tile::default(); width as usize * height as usize],
            rng: Rng::new(seed),
            units: Vec::new(),
            villages: Vec::new(),
            kingdoms: Vec::new(),
            free_units: Vec::new(),
            graveyard: Vec::new(),
            free_villages: Vec::new(),
            free_kingdoms: Vec::new(),
            age: AgeState::new(seed),
            effects: Effects::default(),
            chronicle: VecDeque::new(),
            stats: WorldStats::default(),
            pending_settlements: Vec::new(),
        }
    }

    // --- geometry ----------------------------------------------------------

    pub fn in_bounds(&self, h: Hex) -> bool {
        let (c, r) = h.to_offset();
        c >= 0 && r >= 0 && c < self.width as i32 && r < self.height as i32
    }

    /// Flat index of a hex, row-major in offset coordinates.
    pub fn idx(&self, h: Hex) -> Option<usize> {
        let (c, r) = h.to_offset();
        if c < 0 || r < 0 || c >= self.width as i32 || r >= self.height as i32 {
            return None;
        }
        Some((r * self.width as i32 + c) as usize)
    }

    pub fn tile(&self, h: Hex) -> Option<&Tile> {
        self.idx(h).and_then(|i| self.tiles.get(i))
    }

    pub fn tile_mut(&mut self, h: Hex) -> Option<&mut Tile> {
        self.idx(h).and_then(|i| self.tiles.get_mut(i))
    }

    /// Hex of a flat tile index.
    pub fn hex_at_index(&self, index: usize) -> Hex {
        let w = self.width as usize;
        if w == 0 {
            return Hex::new(0, 0);
        }
        Hex::from_offset((index % w) as i32, (index / w) as i32)
    }

    /// In-bounds neighbours of a hex.
    pub fn neighbors_of(&self, h: Hex) -> Vec<Hex> {
        h.neighbors()
            .into_iter()
            .filter(|n| self.in_bounds(*n))
            .collect()
    }

    /// Every hex on the map, in row-major order.
    pub fn iter_hexes(&self) -> Vec<Hex> {
        let mut out = Vec::with_capacity(self.width as usize * self.height as usize);
        for r in 0..self.height as i32 {
            for c in 0..self.width as i32 {
                out.push(Hex::from_offset(c, r));
            }
        }
        out
    }

    /// Set a tile's biome and refresh its derived fertility.
    pub fn set_biome(&mut self, h: Hex, biome: Biome) -> bool {
        match self.idx(h) {
            Some(i) => {
                self.tiles[i].biome = biome;
                let mut t = self.tiles[i];
                t.refresh_fertility();
                self.tiles[i] = t;
                true
            }
            None => false,
        }
    }

    /// Tiles owned by a village.
    pub fn village_tiles(&self, village: u32) -> Vec<Hex> {
        self.tiles
            .iter()
            .enumerate()
            .filter(|(_, t)| t.owner == Some(village))
            .map(|(i, _)| self.hex_at_index(i))
            .collect()
    }

    // --- entity slots ------------------------------------------------------

    /// Move dead slots onto the free list. Called before allocating.
    pub fn flush_graveyard(&mut self) {
        if !self.graveyard.is_empty() {
            self.free_units.append(&mut self.graveyard);
            self.free_units.sort_unstable();
        }
    }

    pub fn alloc_unit_slot(&mut self) -> u32 {
        self.flush_graveyard();
        if let Some(id) = self.free_units.pop() {
            id
        } else {
            let id = self.units.len() as u32;
            self.units
                .push(Unit::new(id, Race::Human, Hex::new(0, 0), UnitKind::Civilian));
            id
        }
    }

    pub fn free_unit_slot(&mut self, id: u32) {
        if let Some(u) = self.units.get_mut(id as usize) {
            u.alive = false;
        }
        self.graveyard.push(id);
    }

    pub fn alloc_village_slot(&mut self, v: Village) -> u32 {
        if let Some(id) = self.free_villages.pop() {
            let mut v = v;
            v.id = id;
            self.villages[id as usize] = v;
            id
        } else {
            let id = self.villages.len() as u32;
            let mut v = v;
            v.id = id;
            self.villages.push(v);
            id
        }
    }

    pub fn free_village_slot(&mut self, id: u32) {
        if let Some(v) = self.villages.get_mut(id as usize) {
            v.alive = false;
        }
        self.free_villages.push(id);
    }

    pub fn alloc_kingdom_slot(&mut self, k: Kingdom) -> u32 {
        if let Some(id) = self.free_kingdoms.pop() {
            let mut k = k;
            k.id = id;
            self.kingdoms[id as usize] = k;
            id
        } else {
            let id = self.kingdoms.len() as u32;
            let mut k = k;
            k.id = id;
            self.kingdoms.push(k);
            id
        }
    }

    pub fn free_kingdom_slot(&mut self, id: u32) {
        if let Some(k) = self.kingdoms.get_mut(id as usize) {
            k.alive = false;
        }
        self.free_kingdoms.push(id);
    }

    /// Alive villages, in id order.
    pub fn village_ids(&self) -> Vec<u32> {
        self.villages
            .iter()
            .filter(|v| v.alive)
            .map(|v| v.id)
            .collect()
    }

    /// Alive kingdoms, in id order.
    pub fn kingdom_ids(&self) -> Vec<u32> {
        self.kingdoms
            .iter()
            .filter(|k| k.alive)
            .map(|k| k.id)
            .collect()
    }

    pub fn village(&self, id: u32) -> Option<&Village> {
        self.villages.get(id as usize).filter(|v| v.alive)
    }

    pub fn village_mut(&mut self, id: u32) -> Option<&mut Village> {
        self.villages.get_mut(id as usize).filter(|v| v.alive)
    }

    pub fn kingdom(&self, id: u32) -> Option<&Kingdom> {
        self.kingdoms.get(id as usize).filter(|k| k.alive)
    }

    pub fn kingdom_mut(&mut self, id: u32) -> Option<&mut Kingdom> {
        self.kingdoms.get_mut(id as usize).filter(|k| k.alive)
    }

    // --- chronicle and stats ----------------------------------------------

    /// Record an event. The chronicle is a bounded ring buffer.
    pub fn chronicle(&mut self, kind: EventKind, text: String) {
        if self.chronicle.len() >= CHRONICLE_CAP {
            self.chronicle.pop_front();
        }
        self.chronicle.push_back(Event {
            tick: self.tick,
            year: self.year,
            kind,
            text,
        });
    }

    /// The last `n` chronicle entries, newest last.
    pub fn chronicle_tail(&self, n: usize) -> Vec<Event> {
        let len = self.chronicle.len();
        let start = len.saturating_sub(n);
        self.chronicle.iter().skip(start).cloned().collect()
    }

    /// Total living population.
    pub fn population(&self) -> u32 {
        self.units
            .iter()
            .filter(|u| u.alive && u.race.is_civilized())
            .count() as u32
    }

    /// Living civilized population belonging to each race.
    pub fn population_by_race(&self) -> Vec<(Race, u32)> {
        let mut out = Vec::new();
        for race in Race::SPAWNABLE_CIVILIZED {
            let n = self
                .units
                .iter()
                .filter(|u| u.alive && u.race == race)
                .count() as u32;
            if n > 0 {
                out.push((race, n));
            }
        }
        out
    }

    /// Total monsters and animals alive.
    pub fn wildlife_count(&self) -> (u32, u32) {
        let mut animals = 0;
        let mut monsters = 0;
        for u in self.units.iter().filter(|u| u.alive) {
            if u.race.is_animal() {
                animals += 1;
            } else if u.race.is_monster() {
                monsters += 1;
            }
        }
        (animals as u32, monsters as u32)
    }

    /// Advance the clock by one tick. Called at the top of `step`.
    pub fn advance_clock(&mut self) {
        self.tick += 1;
        self.year = (self.tick / TICKS_PER_YEAR) as u32;
    }

    /// Drop dead slots and renumber everything, remapping every reference that
    /// points at a unit, village or kingdom. Only safe between steps (never while
    /// a system is iterating).
    pub fn compact(&mut self) {
        // Deaths queued this tick become free slots first, so the maps below see
        // them and the free lists are rebuilt consistently.
        self.flush_graveyard();
        if self.free_units.is_empty() && self.free_villages.is_empty() && self.free_kingdoms.is_empty()
        {
            return;
        }
        // Old index -> new index.
        let unit_map: Vec<Option<u32>> = build_map(self.units.iter().map(|u| u.alive).collect());
        let village_map: Vec<Option<u32>> =
            build_map(self.villages.iter().map(|v| v.alive).collect());
        let kingdom_map: Vec<Option<u32>> =
            build_map(self.kingdoms.iter().map(|k| k.alive).collect());
        let remap = |map: &[Option<u32>], id: u32| -> Option<u32> {
            map.get(id as usize).copied().flatten()
        };

        self.units.retain(|u| u.alive);
        for (i, u) in self.units.iter_mut().enumerate() {
            u.id = i as u32;
            u.village = u.village.and_then(|v| remap(&village_map, v));
            u.kingdom = u.kingdom.and_then(|k| remap(&kingdom_map, k));
            u.enemy = u.enemy.and_then(|e| remap(&unit_map, e));
        }
        self.villages.retain(|v| v.alive);
        for (i, v) in self.villages.iter_mut().enumerate() {
            v.id = i as u32;
            v.kingdom = v.kingdom.and_then(|k| remap(&kingdom_map, k));
            v.leader = v.leader.and_then(|l| remap(&unit_map, l));
            v.parent = v.parent.and_then(|p| remap(&village_map, p));
            v.besieged_by = v.besieged_by.and_then(|k| remap(&kingdom_map, k));
        }
        self.kingdoms.retain(|k| k.alive);
        for (i, k) in self.kingdoms.iter_mut().enumerate() {
            k.id = i as u32;
            k.capital = remap(&village_map, k.capital).unwrap_or(0);
            k.cities = k
                .cities
                .iter()
                .filter_map(|c| remap(&village_map, *c))
                .collect();
            k.cities.sort_unstable();
            k.cities.dedup();
            k.king = k.king.and_then(|u| remap(&unit_map, u));
            k.wars = k
                .wars
                .iter()
                .filter_map(|w| {
                    remap(&kingdom_map, w.enemy).map(|e| crate::kingdom::War {
                        enemy: e,
                        start_year: w.start_year,
                    })
                })
                .collect();
            k.wars.sort_by_key(|w| w.enemy);
            k.allies = k
                .allies
                .iter()
                .filter_map(|a| remap(&kingdom_map, *a))
                .collect();
            k.allies.sort_unstable();
            k.relations = k
                .relations
                .iter()
                .filter_map(|(r, o)| remap(&kingdom_map, *r).map(|r| (r, *o)))
                .collect();
            k.relations.sort_by_key(|(r, _)| *r);
        }
        for t in self.tiles.iter_mut() {
            t.owner = t.owner.and_then(|v| remap(&village_map, v));
        }
        self.pending_settlements = self
            .pending_settlements
            .iter()
            .filter_map(|(site, race, kingdom, parent)| {
                Some((
                    *site,
                    *race,
                    kingdom.and_then(|k| remap(&kingdom_map, k)),
                    remap(&village_map, *parent)?,
                ))
            })
            .collect();
        self.free_units.clear();
        self.graveyard.clear();
        self.free_villages.clear();
        self.free_kingdoms.clear();
    }

    // --- canonical state hash ---------------------------------------------

    /// Fold the entire world into a `u64`. Two worlds with the same hash are in
    /// the same state as far as the simulation is concerned; tests use this as the
    /// determinism oracle and as the save round-trip oracle.
    pub fn state_hash(&self) -> u64 {
        let mut h = Fnv::new();
        h.u64(self.seed);
        h.u64(self.tick);
        h.u64(self.year as u64);
        h.u64(self.width as u64);
        h.u64(self.height as u64);
        h.u64(self.world_type as u64);
        h.u64(self.land_ratio as u64);
        for t in &self.tiles {
            h.i32(t.elevation as i32);
            h.u64(t.biome.id() as u64);
            h.u64(t.trees as u64);
            h.u64(t.ore as u64);
            h.u64(t.stone as u64);
            h.u64(t.fertile as u64);
            h.u64(t.fire as u64);
            h.u64(t.lava as u64);
            h.u64(t.scorch as u64);
            h.u64(t.river as u64);
            h.u64(t.road as u64);
            h.u64(t.owner.map(|o| o as u64 + 1).unwrap_or(0));
        }
        for u in &self.units {
            h.u64(u.id as u64);
            h.u64(u.alive as u64);
            h.u64(u.race as u64);
            h.u64(u.kind as u64);
            h.str(&u.name);
            h.i32(u.pos.q);
            h.i32(u.pos.r);
            h.i32(u.hp);
            h.i32(u.max_hp);
            h.i32(u.damage);
            h.i32(u.armor);
            h.i32(u.speed);
            h.u64(u.traits);
            for s in u.statuses {
                h.u64(s as u64);
            }
            h.u64(u.age as u64);
            h.u64(u.level as u64);
            h.u64(u.xp as u64);
            h.u64(u.kills as u64);
            h.i32(u.move_points);
            h.u64(u.state as u64);
            h.u64(u.village.map(|v| v as u64 + 1).unwrap_or(0));
            h.u64(u.kingdom.map(|k| k as u64 + 1).unwrap_or(0));
            h.u64(u.enemy.map(|e| e as u64 + 1).unwrap_or(0));
            h.u64(u.destination.is_some() as u64);
            h.i32(u.home.q);
            h.i32(u.home.r);
            h.u64(u.path.len() as u64);
            for p in &u.path {
                h.i32(p.q);
                h.i32(p.r);
            }
            h.u64(u.carry.map(|(r, a)| (r as u64) * 256 + a as u64).unwrap_or(0));
            h.u64(u.item.kind as u64);
            h.u64(u.item.tier as u64);
            h.u64(u.hunger as u64);
            h.u64(u.sailing as u64);
        }
        for v in &self.villages {
            v.hash_into(&mut h);
        }
        for k in &self.kingdoms {
            k.hash_into(&mut h);
        }
        h.u64(self.age.age as u64);
        h.u64(self.age.years_in_age as u64);
        h.u64(self.age.duration_years as u64);
        h.u64(self.age.history.len() as u64);
        h.u64(self.effects.clouds.len() as u64);
        h.u64(self.effects.meteors.len() as u64);
        h.u64(self.effects.tornadoes.len() as u64);
        h.u64(self.effects.volcanoes.len() as u64);
        h.u64(self.effects.black_holes.len() as u64);
        h.u64(self.chronicle.len() as u64);
        h.u64(self.rng.state());
        h.u64(self.units.len() as u64);
        h.u64(self.villages.len() as u64);
        h.u64(self.kingdoms.len() as u64);
        h.u64(self.free_units.len() as u64);
        h.u64(self.free_villages.len() as u64);
        h.u64(self.free_kingdoms.len() as u64);
        h.finish()
    }

    /// A short, human-readable summary line for logs.
    pub fn summary(&self) -> String {
        let (animals, monsters) = self.wildlife_count();
        format!(
            "year {} t{} | pop {} | villages {} | kingdoms {} | animals {} | monsters {} | {}",
            self.year,
            self.tick,
            self.population(),
            self.village_ids().len(),
            self.kingdom_ids().len(),
            animals,
            monsters,
            self.age.age.name()
        )
    }

    /// A compact census used by the CLI's `stats` command.
    /// One line per living village: the table the CLI and scripts print.
    pub fn village_table(&self) -> Vec<String> {
        let mut out = vec![format!(
            "{:<16} {:<6} {:>4} {:>4} {:>4} {:>4} {:>4} {:>5} {:>4} {:>4} {:>6} {:>8} {}",
            "village", "race", "pop", "hous", "hall", "farm", "food", "wood", "stone", "ore", "loyal", "kingdom", "centre"
        )];
        for vid in self.village_ids() {
            let Some(v) = self.village(vid) else { continue };
            let kingdom = v
                .kingdom
                .and_then(|k| self.kingdom(k))
                .map(|k| {
                    if k.capital == vid {
                        format!("{} (capital)", k.name)
                    } else {
                        k.name.clone()
                    }
                })
                .unwrap_or_else(|| "-".to_string());
            out.push(format!(
                "{:<16} {:<6} {:>4} {:>4} {:>4} {:>4} {:>4} {:>5} {:>4} {:>4} {:>6} {:>8} {}",
                v.name,
                v.race.name(),
                v.pop,
                v.houses,
                v.town_hall_level,
                v.production_count(crate::village::BuildingKind::Farm),
                v.food,
                v.wood,
                v.stone,
                v.ore,
                v.loyalty,
                kingdom,
                v.center
            ));
        }
        out
    }

    pub fn census(&self) -> Vec<String> {
        let mut out = Vec::new();
        for (race, n) in self.population_by_race() {
            out.push(format!("{:>8}: {}", race.name(), n));
        }
        let (animals, monsters) = self.wildlife_count();
        out.push(format!("{:>8}: {}", "animals", animals));
        out.push(format!("{:>8}: {}", "monsters", monsters));
        out
    }

    /// Units with a status, counted by status (for `stats`).
    pub fn status_census(&self) -> Vec<(StatusKind, u32)> {
        let mut out = Vec::new();
        for s in StatusKind::ALL {
            let n = self
                .units
                .iter()
                .filter(|u| u.alive && u.has_status(s))
                .count() as u32;
            if n > 0 {
                out.push((s, n));
            }
        }
        out
    }

    /// Append a chronicle line about a power being cast (used by the CLI).
    pub fn log_power(&mut self, text: String) {
        self.chronicle(EventKind::Power, text);
    }
}

/// Old index -> new index for a compaction pass.
fn build_map(alive: Vec<bool>) -> Vec<Option<u32>> {
    let mut out = Vec::with_capacity(alive.len());
    let mut next = 0u32;
    for a in alive {
        if a {
            out.push(Some(next));
            next += 1;
        } else {
            out.push(None);
        }
    }
    out
}

/// FNV-1a 64 hashing helper used for the canonical state hash and the save
/// checksum. Not cryptographic — it only has to be stable and cheap.
pub struct Fnv(u64);

impl Fnv {
    pub fn new() -> Self {
        Fnv(0xcbf2_9ce4_8422_2325)
    }

    pub fn u64(&mut self, v: u64) {
        for b in v.to_le_bytes() {
            self.0 ^= b as u64;
            self.0 = self.0.wrapping_mul(0x100_0000_01b3);
        }
    }

    pub fn i32(&mut self, v: i32) {
        self.u64(v as i64 as u64);
    }

    pub fn u8(&mut self, v: u8) {
        self.u64(v as u64);
    }

    pub fn str(&mut self, s: &str) {
        for b in s.as_bytes() {
            self.u64(*b as u64);
        }
        self.u64(s.len() as u64);
    }

    pub fn finish(&self) -> u64 {
        self.0
    }
}

impl Default for Fnv {
    fn default() -> Self {
        Fnv::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worldgen::WorldType;

    fn world() -> World {
        World::generate(GenParams::new(32, 24, 11, WorldType::Continents))
    }

    #[test]
    fn hex_index_roundtrip() {
        let w = world();
        for h in w.iter_hexes() {
            let i = w.idx(h).unwrap();
            assert_eq!(w.hex_at_index(i), h);
            assert_eq!(w.tile(h).unwrap(), &w.tiles[i]);
        }
        assert!(w.idx(Hex::from_offset(-1, 0)).is_none());
        assert!(w.idx(Hex::from_offset(0, 24)).is_none());
    }

    #[test]
    fn state_hash_is_stable_and_sensitive() {
        let a = world();
        let b = world();
        assert_eq!(a.state_hash(), b.state_hash());
        let mut c = world();
        c.tick += 1;
        assert_ne!(a.state_hash(), c.state_hash());
        let mut d = world();
        d.tiles[0].trees = 3;
        assert_ne!(a.state_hash(), d.state_hash());
    }

    #[test]
    fn slots_are_reused_without_reordering_live_entities() {
        let mut w = world();
        let at = w.random_land_tile().unwrap();
        let a = w.spawn_unit(Race::Human, at, UnitKind::Civilian).unwrap();
        let b = w.spawn_unit(Race::Elf, at, UnitKind::Civilian).unwrap();
        assert_ne!(a, b);
        w.kill_unit(a, None);
        assert!(w.unit(a).is_none());
        let c = w.spawn_unit(Race::Orc, at, UnitKind::Civilian).unwrap();
        assert_eq!(c, a, "the freed slot should be reused");
        assert!(w.unit(b).is_some(), "the other unit must be untouched");
    }

    #[test]
    fn compaction_keeps_entities_and_renumbers() {
        let mut w = world();
        let at = w.random_land_tile().unwrap();
        let a = w.spawn_unit(Race::Human, at, UnitKind::Civilian).unwrap();
        let b = w.spawn_unit(Race::Human, at, UnitKind::Civilian).unwrap();
        w.kill_unit(a, None);
        w.compact();
        assert_eq!(w.unit_count(), 1);
        assert_eq!(w.units[0].id, 0);
        assert!(w.unit(b).is_some() || w.units[0].alive);
    }

    #[test]
    fn chronicle_is_bounded_and_ordered() {
        let mut w = world();
        for i in 0..(CHRONICLE_CAP + 50) {
            w.chronicle(EventKind::Discovery, format!("event {i}"));
        }
        assert_eq!(w.chronicle.len(), CHRONICLE_CAP);
        let tail = w.chronicle_tail(3);
        assert_eq!(tail.len(), 3);
        assert!(tail[2].text.ends_with(&format!("{}", CHRONICLE_CAP + 49)));
    }

    #[test]
    fn summary_and_census_do_not_panic_on_empty_worlds() {
        let w = world();
        assert!(w.summary().contains("year 0"));
        assert!(w.census().len() >= 2, "census always lists animals and monsters");
        assert!(w.status_census().is_empty());
    }

    #[test]
    fn set_biome_refreshes_fertility() {
        let mut w = world();
        let h = w.iter_hexes()[0];
        w.set_biome(h, Biome::Grass);
        assert!(w.tile(h).unwrap().fertile > 50);
        w.set_biome(h, Biome::Lava);
        assert_eq!(w.tile(h).unwrap().fertile, 0);
    }
}
