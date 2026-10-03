//! Villages: where civilization actually happens.
//!
//! A village owns territory, stores resources, builds houses, feeds its people and
//! sends settlers out. Population growth is gated by food *and* housing, which is
//! what makes the WorldBox loop work: no farms, no boom.
//!
//! Building order follows the genre: fireplace, town hall (three tiers), houses,
//! then farms/mines/barracks/docks/walls as resources allow.

use crate::hex::Hex;
use crate::names;
use crate::races::Race;
use crate::units::{Resource, UnitKind, UnitState};
use crate::world::{EventKind, Fnv, World};

/// Hard cap on buildings per village, so saves stay bounded.
pub const MAX_BUILDINGS: usize = 40;
/// Population at which a village is allowed to send out settlers.
pub const SETTLER_POP: u32 = 14;
/// How many people travel with a settler band.
pub const SETTLER_BAND: u32 = 3;
/// Food a village can keep in store.
pub const GRANARY_CAP: i32 = 500;
/// Monsters seeded by `seed_life` start at least this far from any village.
pub const MONSTER_GAP: i32 = 14;
/// Citizens needed before a village can be founded at all.
pub const FOUNDING_POP: u32 = 6;

/// Everything buildable.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BuildingKind {
    Fireplace,
    TownHall,
    House,
    Farm,
    Mine,
    Sawmill,
    Barracks,
    Dock,
    Temple,
    Tower,
    Well,
    Statue,
    Wall,
}

impl BuildingKind {
    pub const ALL: [BuildingKind; 13] = [
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
    ];

    pub fn name(self) -> &'static str {
        match self {
            BuildingKind::Fireplace => "fireplace",
            BuildingKind::TownHall => "town hall",
            BuildingKind::House => "house",
            BuildingKind::Farm => "farm",
            BuildingKind::Mine => "mine",
            BuildingKind::Sawmill => "sawmill",
            BuildingKind::Barracks => "barracks",
            BuildingKind::Dock => "dock",
            BuildingKind::Temple => "temple",
            BuildingKind::Tower => "tower",
            BuildingKind::Well => "well",
            BuildingKind::Statue => "statue",
            BuildingKind::Wall => "wall",
        }
    }

    /// `[wood, stone, ore]` to start construction.
    pub fn cost(self) -> [i32; 3] {
        match self {
            BuildingKind::Fireplace => [2, 0, 0],
            BuildingKind::TownHall => [20, 10, 0],
            BuildingKind::House => [8, 0, 0],
            BuildingKind::Farm => [10, 0, 0],
            BuildingKind::Mine => [5, 10, 0],
            BuildingKind::Sawmill => [12, 4, 0],
            BuildingKind::Barracks => [16, 14, 2],
            BuildingKind::Dock => [18, 6, 0],
            BuildingKind::Temple => [24, 20, 4],
            BuildingKind::Tower => [10, 18, 0],
            BuildingKind::Well => [6, 8, 0],
            BuildingKind::Statue => [14, 12, 2],
            BuildingKind::Wall => [0, 24, 0],
        }
    }

    /// How much builder work the site needs.
    pub fn work(self) -> u16 {
        match self {
            BuildingKind::Fireplace => 8,
            BuildingKind::House => 30,
            BuildingKind::Farm => 40,
            BuildingKind::Mine => 45,
            BuildingKind::Sawmill => 40,
            BuildingKind::Well => 40,
            BuildingKind::Dock => 70,
            BuildingKind::TownHall => 90,
            BuildingKind::Barracks => 90,
            BuildingKind::Temple => 120,
            BuildingKind::Tower => 100,
            BuildingKind::Statue => 80,
            BuildingKind::Wall => 140,
        }
    }

    /// Buildings that produce something every tick.
    pub fn is_production(self) -> bool {
        matches!(self, BuildingKind::Farm | BuildingKind::Mine | BuildingKind::Sawmill)
    }

    /// Cap per village. Houses are capped by housing demand instead.
    pub fn max_count(self) -> u32 {
        match self {
            BuildingKind::Fireplace => 1,
            BuildingKind::TownHall => 1,
            BuildingKind::Farm => 6,
            BuildingKind::Mine => 3,
            BuildingKind::Sawmill => 3,
            BuildingKind::Barracks => 1,
            BuildingKind::Dock => 2,
            BuildingKind::Temple => 1,
            BuildingKind::Tower => 4,
            BuildingKind::Well => 2,
            BuildingKind::Statue => 2,
            BuildingKind::Wall => 2,
            BuildingKind::House => 40,
        }
    }
}

/// A structure in a village.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Building {
    pub kind: BuildingKind,
    pub pos: Hex,
    pub level: u8,
    pub complete: bool,
    pub progress: u16,
}

impl Building {
    pub fn new(kind: BuildingKind, pos: Hex) -> Self {
        Building {
            kind,
            pos,
            level: 1,
            complete: false,
            progress: 0,
        }
    }
}

/// A settlement.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Village {
    pub id: u32,
    pub alive: bool,
    pub name: String,
    pub race: Race,
    pub center: Hex,
    /// Citizens belonging to this village (civilians, soldiers, leader).
    pub pop: u32,
    pub houses: u16,
    pub town_hall_level: u8,
    pub food: i32,
    pub wood: i32,
    pub stone: i32,
    pub ore: i32,
    pub gold: i32,
    pub buildings: Vec<Building>,
    pub build_site: Option<Building>,
    pub kingdom: Option<u32>,
    /// -100 (about to revolt) .. 100 (devoted).
    pub loyalty: i16,
    pub founded_year: u32,
    pub births: u32,
    pub deaths: u32,
    pub soldiers: u16,
    pub leader: Option<u32>,
    pub capital: bool,
    pub walls: u8,
    pub claim_radius: i32,
    /// Village this one was settled from, if any.
    pub parent: Option<u32>,
    pub last_war_year: u32,
    pub boats: u16,
    pub starving: bool,
    /// Siege progress against this village, 0..=100.
    pub siege: i32,
    /// Which kingdom is currently besieging it.
    pub besieged_by: Option<u32>,
}

impl Default for Village {
    fn default() -> Self {
        Village {
            id: 0,
            alive: true,
            name: String::new(),
            race: Race::Human,
            center: Hex::new(0, 0),
            pop: 0,
            houses: 0,
            town_hall_level: 0,
            food: 20,
            wood: 10,
            stone: 5,
            ore: 0,
            gold: 0,
            buildings: Vec::new(),
            build_site: None,
            kingdom: None,
            loyalty: 50,
            founded_year: 0,
            births: 0,
            deaths: 0,
            soldiers: 0,
            leader: None,
            capital: false,
            walls: 0,
            claim_radius: 3,
            parent: None,
            last_war_year: 0,
            boats: 0,
            starving: false,
            siege: 0,
            besieged_by: None,
        }
    }
}

impl Village {
    pub fn new(name: String, race: Race, center: Hex, year: u32) -> Self {
        Village {
            name,
            race,
            center,
            founded_year: year,
            food: 25,
            wood: 12,
            stone: 6,
            ..Default::default()
        }
    }

    /// Houses the current population needs. Growth stalls until they exist.
    pub fn housing_needed(&self) -> u32 {
        (self.pop / 2 + 1).min(BuildingKind::House.max_count())
    }

    /// Housing capacity from houses and the town hall.
    pub fn housing(&self) -> u32 {
        let houses = self.houses as u32 * 2;
        let hall = self.town_hall_level as u32 * 4;
        // A village always has room for the people who founded it.
        (houses + hall).max(6)
    }

    /// Buildings of a kind, including the one under construction.
    pub fn count(&self, kind: BuildingKind) -> u32 {
        self.buildings.iter().filter(|b| b.kind == kind).count() as u32
    }

    pub fn has(&self, kind: BuildingKind) -> bool {
        // The building list is capped, so the counters are the authority for the
        // kinds that have one (and they survive older saves).
        match kind {
            BuildingKind::TownHall => self.town_hall_level > 0,
            BuildingKind::Wall => self.walls > 0,
            _ => self.buildings.iter().any(|b| b.kind == kind),
        }
    }

    /// Farms, mines and sawmills: what the village produces each tick.
    pub fn production_count(&self, kind: BuildingKind) -> i32 {
        self.buildings
            .iter()
            .filter(|b| b.kind == kind && b.complete)
            .map(|b| b.level as i32)
            .sum()
    }

    /// Soldiers this village can still field, based on population.
    pub fn soldier_cap(&self) -> u16 {
        if self.pop < 6 {
            return 0;
        }
        ((self.pop / 4).min(12)) as u16
    }

    /// Is this village next to water (can build docks and boats)?
    pub fn is_coastal(&self, world: &World) -> bool {
        self.center
            .spiral(3)
            .iter()
            .any(|h| world.tile(*h).map(|t| t.is_sea()).unwrap_or(false))
    }

    /// One-line status for the CLI.
    pub fn describe(&self) -> String {
        format!(
            "#{} {} ({}) pop {} houses {} hall {} loy {} food {} wood {} stone {} ore {} gold {} {}",
            self.id,
            self.name,
            self.race.name(),
            self.pop,
            self.houses,
            self.town_hall_level,
            self.loyalty,
            self.food,
            self.wood,
            self.stone,
            self.ore,
            self.gold,
            match self.kingdom {
                Some(k) => format!("kingdom {k}"),
                None => "independent".to_string(),
            }
        )
    }

    pub fn hash_into(&self, h: &mut Fnv) {
        h.u64(self.id as u64);
        h.u64(self.alive as u64);
        h.str(&self.name);
        h.u64(self.race as u64);
        h.i32(self.center.q);
        h.i32(self.center.r);
        h.u64(self.pop as u64);
        h.u64(self.houses as u64);
        h.u64(self.town_hall_level as u64);
        h.u64(self.food as i64 as u64);
        h.u64(self.wood as i64 as u64);
        h.u64(self.stone as i64 as u64);
        h.u64(self.ore as i64 as u64);
        h.u64(self.gold as i64 as u64);
        h.u64(self.buildings.len() as u64);
        for b in &self.buildings {
            h.u64(b.kind as u64);
            h.i32(b.pos.q);
            h.i32(b.pos.r);
            h.u64(b.level as u64);
            h.u64(b.complete as u64);
        }
        h.u64(self.build_site.map(|b| b.kind as u64 + 1).unwrap_or(0));
        h.u64(self.kingdom.map(|k| k as u64 + 1).unwrap_or(0));
        h.u64(self.loyalty as i64 as u64);
        h.u64(self.founded_year as u64);
        h.u64(self.births as u64);
        h.u64(self.deaths as u64);
        h.u64(self.soldiers as u64);
        h.u64(self.leader.map(|l| l as u64 + 1).unwrap_or(0));
        h.u64(self.capital as u64);
        h.u64(self.walls as u64);
        h.u64(self.claim_radius as i64 as u64);
        h.u64(self.siege as i64 as u64);
        h.u64(self.besieged_by.map(|b| b as u64 + 1).unwrap_or(0));
    }
}

impl World {
    /// Score a tile as a village site: fertility, wood, fresh water, ore, and
    /// distance from other villages. Higher is better.
    pub fn site_score(&self, h: Hex, race: Race, avoid: Option<Hex>) -> i32 {
        let Some(center) = self.tile(h) else {
            return -1000;
        };
        if !center.is_buildable() || center.owner.is_some() {
            return -1000;
        }
        let mut score = 0;
        let mut water = 0;
        let mut trees = 0;
        let mut ore = 0;
        let mut fertility = 0;
        let mut buildable = 0;
        let mut owned_penalty = 0;
        for (i, nb) in h.spiral(4).into_iter().enumerate() {
            let Some(t) = self.tile(nb) else {
                continue;
            };
            let w = if i == 0 { 2 } else { 1 };
            if t.is_sea() {
                water += w;
            }
            trees += t.trees as i32 * w;
            ore += t.ore as i32 * w + t.stone as i32 * w;
            fertility += t.fertile as i32 * w;
            if t.is_buildable() {
                buildable += w;
            }
            if t.owner.is_some() {
                owned_penalty += w;
            }
        }
        score += fertility / 4;
        score += trees * 2;
        score += ore * 3;
        score += water.min(4) * 6; // fresh water and fishing
        score += buildable / 2;
        score += race.habitability(center.biome) / 2;
        score -= owned_penalty * 6;
        if let Some(a) = avoid {
            score -= (h.distance(a) * 2).min(40);
        }
        score - center.scorch as i32 * 3 - center.lava as i32 * 20
    }

    /// Find the best nearby spot for a new village of `race`.
    pub fn best_village_site(&self, from: Hex, race: Race, min_dist: i32, max_dist: i32) -> Option<Hex> {
        let mut best: Option<(i32, Hex)> = None;
        for h in from.spiral(max_dist) {
            let d = h.distance(from);
            if d < min_dist {
                continue;
            }
            let score = self.site_score(h, race, Some(from));
            if score <= 0 {
                continue;
            }
            let better = match best {
                None => true,
                Some((bs, bh)) => score > bs || (score == bs && h < bh),
            };
            if better {
                best = Some((score, h));
            }
        }
        best.map(|(_, h)| h)
    }

    /// Create a village and hand it its founders.
    pub fn found_village(
        &mut self,
        center: Hex,
        race: Race,
        founders: &[u32],
        kingdom: Option<u32>,
        parent: Option<u32>,
    ) -> Option<u32> {
        if self.tile(center).is_none() || self.tile(center)?.owner.is_some() {
            return None;
        }
        let name = names::village_name(&mut self.rng, race);
        let mut v = Village::new(name.clone(), race, center, self.year);
        v.kingdom = kingdom;
        v.parent = parent;
        let vid = self.alloc_village_slot(v);
        // Claim the founding tiles.
        let tiles = center.spiral(2);
        for h in tiles {
            if let Some(t) = self.tile_mut(h) {
                if t.is_land() && t.owner.is_none() {
                    t.owner = Some(vid);
                }
            }
        }
        // The founders join up; the first one leads.
        let leader_name = if founders.is_empty() {
            String::new()
        } else {
            names::person_name(&mut self.rng)
        };
        for (i, u) in founders.iter().enumerate() {
            if let Some(unit) = self.unit_mut(*u) {
                unit.village = Some(vid);
                unit.kingdom = kingdom;
                unit.home = center;
                if i == 0 {
                    unit.kind = UnitKind::Leader;
                    unit.name = leader_name.clone();
                    let leader = unit.id;
                    if let Some(v) = self.village_mut(vid) {
                        v.leader = Some(leader);
                    }
                }
            }
            if let Some(v) = self.village_mut(vid) {
                v.pop += 1;
            }
            if let Some(k) = kingdom {
                if let Some(kd) = self.kingdom_mut(k) {
                    kd.population += 1;
                }
            }
        }
        // Fireplace first, always.
        if let Some(v) = self.village_mut(vid) {
            v.buildings.push(Building {
                kind: BuildingKind::Fireplace,
                pos: center,
                level: 1,
                complete: true,
                progress: 0,
            });
        }
        self.stats.villages_founded += 1;
        self.chronicle(
            EventKind::VillageFounded,
            format!("{} is founded by the {} at {center}", name, race.name()),
        );
        Some(vid)
    }

    /// Populate a freshly generated world.
    ///
    /// Places `civs` starting villages (spread out, one civilized race each in
    /// turn), `animals` head of wildlife and `monsters` horrors. Returns the ids
    /// of the villages that were founded. Every choice is made through the
    /// world's RNG, so the same seed always seeds the same life.
    pub fn seed_life(&mut self, civs: u32, animals: u32, monsters: u32) -> Vec<u32> {
        const RACES: [Race; 4] = [Race::Human, Race::Elf, Race::Dwarf, Race::Orc];
        const MIN_VILLAGE_GAP: i32 = 12;
        let mut sites: Vec<Hex> = Vec::new();
        for i in 0..civs {
            let race = RACES[(i as usize) % RACES.len()];
            // Sample the map and keep the best site that is far from the others.
            let mut best: Option<(i32, Hex)> = None;
            for _ in 0..64 {
                let Some(h) = self.random_land_tile() else { break };
                if sites.iter().any(|s| h.distance(*s) < MIN_VILLAGE_GAP) {
                    continue;
                }
                let avoid = sites.iter().copied().min_by_key(|s| h.distance(*s));
                let score = self.site_score(h, race, avoid);
                if score <= 0 {
                    continue;
                }
                let better = match best {
                    None => true,
                    Some((bs, bh)) => score > bs || (score == bs && h < bh),
                };
                if better {
                    best = Some((score, h));
                }
            }
            if let Some((_, site)) = best {
                sites.push(site);
            }
        }
        self.seed_life_at(&sites, animals, monsters)
    }

    /// Settle at exactly these sites, then scatter wildlife and monsters.
    ///
    /// [`World::seed_life`] picks its own sites; this is the same settlement and
    /// the same wildlife rules with the sites handed in, which is what an
    /// imported map needs (the reader knows where the land is, not where the
    /// villages of the original save were).
    pub fn seed_life_at(&mut self, sites: &[Hex], animals: u32, monsters: u32) -> Vec<u32> {
        const RACES: [Race; 4] = [Race::Human, Race::Elf, Race::Dwarf, Race::Orc];
        let mut founded = Vec::new();
        for (i, site) in sites.iter().enumerate() {
            let race = RACES[i % RACES.len()];
            let mut founders = Vec::new();
            for _ in 0..FOUNDING_POP {
                if let Some(id) = self.spawn_unit(race, *site, UnitKind::Civilian) {
                    founders.push(id);
                }
            }
            if let Some(vid) = self.found_village(*site, race, &founders, None, None) {
                founded.push(vid);
            }
        }
        for _ in 0..animals {
            let race = self.rng.pick_copy(&Race::SPAWNABLE_ANIMALS);
            let Some(h) = self.random_land_tile() else { break };
            self.spawn_unit(race, h, UnitKind::Animal);
        }
        // Monsters are put down well away from the villages: a horror camped on a
        // hamlet's doorstep ends the game before it starts.
        for _ in 0..monsters {
            let race = self.rng.pick_copy(&Race::SPAWNABLE_MONSTERS);
            let mut spot = None;
            for _ in 0..48 {
                let Some(h) = self.random_land_tile() else { break };
                if sites.iter().all(|s| h.distance(*s) >= MONSTER_GAP) {
                    spot = Some(h);
                    break;
                }
            }
            let Some(h) = spot.or_else(|| self.random_land_tile()) else {
                break;
            };
            self.spawn_unit(race, h, UnitKind::Monster);
        }
        founded
    }

    /// Spawn a band of settlers from `vid` and send them looking for a new home.
    pub fn send_settlers(&mut self, vid: u32) -> Option<Hex> {
        let (center, race, kingdom) = {
            let v = self.village(vid)?;
            (v.center, v.race, v.kingdom)
        };
        // Never strip the village below a viable population.
        let pop = self.village(vid).map(|v| v.pop).unwrap_or(0);
        if pop < FOUNDING_POP + 2 {
            return None;
        }
        let band = ((pop - 2) / 2).min(SETTLER_BAND) as usize;
        let candidates: Vec<u32> = self
            .units
            .iter()
            .filter(|u| {
                u.alive
                    && u.village == Some(vid)
                    && u.kind != UnitKind::Soldier
                    && u.kind != UnitKind::Leader
            })
            .map(|u| u.id)
            .take(band)
            .collect();
        if candidates.len() < 2 {
            return None;
        }
        // Pick a destination: the best site in a ring around the village.
        let site = self.best_village_site(center, race, 6, 16)?;
        if let Some(v) = self.village_mut(vid) {
            v.pop = v.pop.saturating_sub(candidates.len() as u32);
            if let Some(k) = kingdom {
                if let Some(kd) = self.kingdom_mut(k) {
                    kd.population = kd.population.saturating_sub(candidates.len() as u32);
                }
            }
        }
        // Walk them there. If the sea is in the way, they take a boat.
        for u in &candidates {
            let mode_ok = self.order_move(*u, site);
            if !mode_ok
                && self.board_boat(*u) {
                    self.order_move(*u, site);
                    if let Some(v) = self.village_mut(vid) {
                        v.boats = v.boats.saturating_add(1);
                    }
                    self.stats.boats_built += 1;
                }
            if let Some(unit) = self.unit_mut(*u) {
                unit.state = UnitState::March;
                unit.destination = Some(site);
                unit.village = Some(vid);
            }
        }
        // The band founds the village when it arrives; park the plan on the RNG-free
        // side by storing the intent on the units' destinations and letting
        // `settler_tick` finish the job.
        self.pending_settlements.push((site, race, kingdom, vid));
        Some(site)
    }

    /// Finish settlements whose founders have arrived.
    pub fn settler_tick(&mut self) {
        if self.pending_settlements.is_empty() {
            return;
        }
        let mut done: Vec<usize> = Vec::new();
        let pending = self.pending_settlements.clone();
        for (i, (site, race, kingdom, parent)) in pending.iter().enumerate() {
            let here: Vec<u32> = self
                .units
                .iter()
                .filter(|u| {
                    u.alive
                        && u.village == Some(*parent)
                        && u.pos.distance(*site) <= 1
                        && u.kind != UnitKind::Soldier
                })
                .map(|u| u.id)
                .collect();
            if here.len() >= 2 {
                self.found_village(*site, *race, &here, *kingdom, Some(*parent));
                done.push(i);
            } else if self.year > 0 && self.tick % 400 == 0 {
                // Give up after a long walk; the band simply stays home.
                done.push(i);
            }
        }
        for i in done.into_iter().rev() {
            self.pending_settlements.remove(i);
        }
    }

    /// One tick of every village's economy, construction and growth.
    pub fn village_tick(&mut self) {
        let ids = self.village_ids();
        for vid in ids {
            self.village_tick_one(vid);
        }
        self.settler_tick();
    }

    fn village_tick_one(&mut self, vid: u32) {
        // --- presence check -------------------------------------------------
        let Some(v) = self.village(vid) else { return };
        let center = v.center;
        let race = v.race;
        let kingdom = v.kingdom;
        // Village dies when everyone is dead.
        let citizens = self
            .units
            .iter()
            .filter(|u| u.alive && u.village == Some(vid))
            .count() as u32;
        let leader_alive = self
            .village(vid)
            .and_then(|v| v.leader)
            .map(|l| self.unit(l).is_some())
            .unwrap_or(false);
        {
            let Some(v) = self.village_mut(vid) else { return };
            v.pop = citizens;
        }
        if citizens == 0 && self.village(vid).map(|v| v.founded_year).unwrap_or(0) + 2 < self.year {
            self.destroy_village(vid, "its people are gone");
            return;
        }

        // --- food -----------------------------------------------------------
        self.village_food_tick(vid);

        // --- construction ---------------------------------------------------
        self.village_build_tick(vid);

        // --- tasks for the idle --------------------------------------------
        self.assign_tasks(vid);

        // --- soldiers -------------------------------------------------------
        self.village_soldier_tick(vid);

        // --- territory ------------------------------------------------------
        if self.tick % 6 == 0 {
            self.village_claim_tick(vid);
        }

        // --- growth ---------------------------------------------------------
        self.village_growth_tick(vid);

        // --- loyalty -------------------------------------------------------
        if self.tick % 10 == 0 {
            self.village_loyalty_tick(vid);
        }

        // --- leadership -----------------------------------------------------
        if !leader_alive {
            let new_leader = self
                .units
                .iter()
                .filter(|u| u.alive && u.village == Some(vid) && u.kind != UnitKind::Soldier)
                .map(|u| u.id)
                .next();
            if let Some(id) = new_leader {
                let person = names::person_name(&mut self.rng);
                if let Some(u) = self.unit_mut(id) {
                    u.kind = UnitKind::Leader;
                    u.name = person;
                }
                if let Some(v) = self.village_mut(vid) {
                    v.leader = Some(id);
                }
                if let Some(k) = kingdom {
                    if let Some(kd) = self.kingdom_mut(k) {
                        if kd.capital == vid {
                            kd.king = Some(id);
                        }
                    }
                }
            }
        }

        // --- culture: elves replant, dwarves dig, everyone else keeps what they
        // took
        if race == Race::Elf && self.tick % 50 == 0 {
            for h in center.spiral(3) {
                if let Some(i) = self.idx(h) {
                    let cap = self.tiles[i].biome.tree_capacity();
                    if cap > 0 && self.tiles[i].trees < cap {
                        self.tiles[i].trees += 1;
                    }
                }
            }
        }
    }

    /// Harvest and hunger.
    ///
    /// The village reaps every tick; its people eat out of the larder as they go
    /// (`unit_needs_tick`), about one food per villager per four ticks. A village
    /// whose larder was empty when it woke up is `starving`: it loses loyalty and,
    /// now and then, a life.
    fn village_food_tick(&mut self, vid: u32) {
        let (pop, farms, wells, docks) = match self.village(vid) {
            Some(v) => (
                v.pop as i32,
                v.production_count(BuildingKind::Farm),
                v.count(BuildingKind::Well) as i32,
                v.production_count(BuildingKind::Dock),
            ),
            None => return,
        };
        // The land itself feeds people too: good ground inside the borders.
        let land_food = (self
            .tiles
            .iter()
            .filter(|t| t.owner == Some(vid) && t.fertile > 40)
            .count() as i32
            / 4)
            .max(2);
        let produced = farms * 3 + land_food + wells + docks * 2;
        // Roll before borrowing the village.
        let hungry_now = self.rng.chance(0.03);
        let fatal = self.rng.chance(0.5);
        let starved = {
            let Some(v) = self.village_mut(vid) else { return };
            let larder = v.food;
            v.food = (v.food + produced).min(GRANARY_CAP);
            v.starving = pop > 0 && larder <= 0;
            v.starving
        };
        if pop <= 0 || !starved || !hungry_now || !fatal {
            return;
        }
        // Somebody does not make it through the night.
        let victim = self
            .units
            .iter()
            .filter(|u| u.alive && u.village == Some(vid) && u.kind != UnitKind::Soldier)
            .map(|u| u.id)
            .next();
        if let Some(id) = victim {
            if pop > 1 {
                self.kill_unit(id, None);
                let name = self
                    .village(vid)
                    .map(|v| v.name.clone())
                    .unwrap_or_default();
                self.chronicle(EventKind::Death, format!("Starvation in {name}"));
            }
        }
    }

    /// Pick the village's next building and advance the current site.
    fn village_build_tick(&mut self, vid: u32) {
        // Advance work on the current site.
        let mut completed: Option<BuildingKind> = None;
        {
            let Some(v) = self.village_mut(vid) else { return };
            if let Some(site) = v.build_site.as_mut() {
                site.progress = site.progress.saturating_add(2);
                if site.progress >= site.kind.work() {
                    site.complete = true;
                    completed = Some(site.kind);
                }
            }
        }
        if let Some(kind) = completed {
            let (site, name) = {
                let Some(v) = self.village_mut(vid) else { return };
                let site = v.build_site.take().unwrap();
                if kind == BuildingKind::House {
                    v.houses += 1;
                }
                if kind == BuildingKind::TownHall {
                    v.town_hall_level = (v.town_hall_level + 1).min(3);
                }
                if kind == BuildingKind::Wall {
                    v.walls = (v.walls + 1).min(3);
                }
                (site, v.name.clone())
            };
            if let Some(v) = self.village_mut(vid) {
                if v.buildings.len() < MAX_BUILDINGS {
                    v.buildings.push(site);
                }
                if kind == BuildingKind::Dock {
                    v.boats = v.boats.saturating_add(1);
                }
            }
            self.stats.buildings_built += 1;
            if matches!(
                kind,
                BuildingKind::Barracks | BuildingKind::Temple | BuildingKind::TownHall
            ) {
                self.chronicle(
                    EventKind::Discovery,
                    format!("{} completes a {}", name, kind.name()),
                );
            }
        }
        // Choose the next thing to build.
        let next = {
            let Some(v) = self.village(vid) else { return };
            if v.build_site.is_some() {
                return;
            }
            // The first thing on the list the village can actually pay for.
            self.build_priorities(v)
                .into_iter()
                .find(|kind| {
                    let c = kind.cost();
                    v.wood >= c[0] && v.stone >= c[1] && v.ore >= c[2]
                })
        };
        let Some(kind) = next else { return };
        let cost = kind.cost();
        let pos = self.find_build_spot(vid);
        let Some(pos) = pos else { return };
        {
            let Some(v) = self.village_mut(vid) else { return };
            if v.wood < cost[0] || v.stone < cost[1] || v.ore < cost[2] {
                return;
            }
            v.wood -= cost[0];
            v.stone -= cost[1];
            v.ore -= cost[2];
            v.build_site = Some(Building::new(kind, pos));
        }
        // Put someone on it.
        let builder = self
            .units
            .iter()
            .filter(|u| {
                u.alive
                    && u.village == Some(vid)
                    && u.kind != UnitKind::Soldier
                    && matches!(u.state, UnitState::Idle | UnitState::Wander | UnitState::Gather)
            })
            .map(|u| u.id)
            .next();
        if let Some(b) = builder {
            self.assign_build(b, vid, pos);
        }
    }

    /// What this village would like to build next, most important first.
    ///
    /// The caller walks the list and takes the first entry it can pay for, so a
    /// village with no stone nearby keeps making progress on what it can afford
    /// instead of stalling on a quarry it cannot reach.
    fn build_priorities(&self, v: &Village) -> Vec<BuildingKind> {
        let mut out = Vec::new();
        if !v.has(BuildingKind::Fireplace) {
            out.push(BuildingKind::Fireplace);
        }
        // Houses first: population is capped by housing.
        if (v.houses as u32) < v.housing_needed() {
            out.push(BuildingKind::House);
        }
        // Then food: a village that cannot feed itself never grows.
        if v.production_count(BuildingKind::Farm) < (v.pop as i32 / 8).max(1) {
            out.push(BuildingKind::Farm);
        }
        // The hall comes before the village gets fancy: it is what makes a
        // kingdom, and what lets houses grow past their first tier.
        if v.town_hall_level < 2 {
            out.push(BuildingKind::TownHall);
        }
        if v.production_count(BuildingKind::Farm) < (v.pop as i32 / 4).clamp(1, 6) {
            out.push(BuildingKind::Farm);
        }
        if v.stone > 10 && v.count(BuildingKind::Mine) < 2 {
            out.push(BuildingKind::Mine);
        }
        if v.wood > 12 && v.count(BuildingKind::Sawmill) < 2 {
            out.push(BuildingKind::Sawmill);
        }
        if v.pop > 8 && v.count(BuildingKind::Well) < 1 {
            out.push(BuildingKind::Well);
        }
        if v.town_hall_level < 3 && v.pop > 10 {
            out.push(BuildingKind::TownHall);
        }
        if v.pop > 12 && v.count(BuildingKind::Barracks) < 1 {
            out.push(BuildingKind::Barracks);
        }
        if v.pop > 16 && v.count(BuildingKind::Tower) < 2 {
            out.push(BuildingKind::Tower);
        }
        if v.pop > 20 && v.count(BuildingKind::Temple) < 1 {
            out.push(BuildingKind::Temple);
        }
        if v.pop > 24 && v.count(BuildingKind::Statue) < 1 {
            out.push(BuildingKind::Statue);
        }
        if v.pop > 18 && v.walls < 2 {
            out.push(BuildingKind::Wall);
        }
        out
    }

    /// A free tile next to the village for a new building.
    fn find_build_spot(&self, vid: u32) -> Option<Hex> {
        let v = self.village(vid)?;
        let mut used: Vec<Hex> = v.buildings.iter().map(|b| b.pos).collect();
        if let Some(s) = v.build_site {
            used.push(s.pos);
        }
        for h in v.center.spiral(4) {
            if used.contains(&h) {
                continue;
            }
            let Some(t) = self.tile(h) else { continue };
            if t.is_buildable() && t.lava == 0 {
                return Some(h);
            }
        }
        Some(v.center)
    }

    /// Hand out jobs to idle citizens.
    fn assign_tasks(&mut self, vid: u32) {
        let (center, race, _kingdom) = match self.village(vid) {
            Some(v) => (v.center, v.race, v.kingdom),
            None => return,
        };
        let idle: Vec<u32> = self
            .units
            .iter()
            .filter(|u| {
                u.alive
                    && u.village == Some(vid)
                    && u.kind != UnitKind::Soldier
                    && matches!(u.state, UnitState::Idle | UnitState::Wander)
            })
            .map(|u| u.id)
            .take(4)
            .collect();
        let (wood, stone, ore, food, pop) = {
            let v = match self.village(vid) {
                Some(v) => v,
                None => return,
            };
            (v.wood, v.stone, v.ore, v.food, v.pop)
        };
        let thin_larder = food < pop as i32 * 2;
        for uid in idle {
            // Prefer farming when the larder is thin.
            if thin_larder {
                if let Some(farm) = self
                    .village(vid)
                    .and_then(|v| v.buildings.iter().find(|b| b.kind == BuildingKind::Farm).map(|b| b.pos))
                {
                    self.assign_move_and_state(uid, farm, UnitState::Gather);
                    continue;
                }
            }
            // Otherwise gather whatever the village is shortest of, measured as a
            // fraction of what it wants to have in store.
            let fill = |have: i32, target: i32| have as f32 / target as f32;
            let mut want = Resource::Wood;
            let mut lowest = fill(wood, 40);
            if fill(stone, 30) < lowest {
                lowest = fill(stone, 30);
                want = Resource::Stone;
            }
            if fill(ore, 20) < lowest {
                want = Resource::Ore;
            }
            let target = self
                .find_resource_tile(center, want, 10)
                .or_else(|| self.find_resource_tile(center, want, 18));
            match target {
                Some(h) => {
                    if let Some(u) = self.unit_mut(uid) {
                        u.state = UnitState::Gather;
                        u.destination = Some(h);
                        u.carry = Some((want, 0));
                    }
                    self.order_move(uid, h);
                }
                None => {
                    // Nothing to gather: wander near home.
                    let spot = center.neighbor(self.rng.below(6) as usize);
                    if self.tile(spot).map(|t| t.is_walkable()).unwrap_or(false) {
                        self.assign_move_and_state(uid, spot, UnitState::Wander);
                    }
                }
            }
            let _ = race;
        }
    }

    fn assign_build(&mut self, uid: u32, _vid: u32, pos: Hex) {
        if let Some(u) = self.unit_mut(uid) {
            u.state = UnitState::Build;
            u.destination = Some(pos);
        }
        self.order_move(uid, pos);
    }

    fn assign_move_and_state(&mut self, uid: u32, dest: Hex, state: UnitState) {
        if let Some(u) = self.unit_mut(uid) {
            u.state = state;
            u.destination = Some(dest);
        }
        self.order_move(uid, dest);
    }

    /// Nearest tile with the wanted resource, reachable on foot-ish.
    pub fn find_resource_tile(&self, from: Hex, want: Resource, radius: i32) -> Option<Hex> {
        let mut best: Option<(i32, Hex)> = None;
        for h in from.spiral(radius) {
            let Some(t) = self.tile(h) else { continue };
            let ok = match want {
                Resource::Wood => t.trees > 0,
                Resource::Stone => t.stone > 0 || t.is_mountain(),
                Resource::Ore => t.ore > 0,
                Resource::Gold => t.ore > 1,
                Resource::Food => t.fertile > 40 && !t.is_water(),
            };
            if !ok {
                continue;
            }
            // Most resources are picked up where they lie; mountains are not
            // walkable, so a quarry digger stands on the tile beside one.
            let dest = if t.is_walkable() {
                h
            } else {
                match h
                    .neighbors()
                    .into_iter()
                    .find(|n| self.tile(*n).map(|t| t.is_walkable()).unwrap_or(false))
                {
                    Some(n) => n,
                    None => continue,
                }
            };
            // Prefer owned land, then closeness.
            let owned = t.owner.is_some() as i32;
            let score = dest.distance(from) - owned * 3;
            let better = match best {
                None => true,
                Some((bs, bh)) => score < bs || (score == bs && dest < bh),
            };
            if better {
                best = Some((score, dest));
            }
        }
        best.map(|(_, h)| h)
    }

    /// Claim neighbouring tiles as the village grows.
    fn village_claim_tick(&mut self, vid: u32) {
        let owned = self.tiles.iter().filter(|t| t.owner == Some(vid)).count() as i32;
        let (center, pop) = match self.village(vid) {
            Some(v) => (v.center, v.pop),
            None => return,
        };
        // Territory scales with population, with a floor so hamlets have borders.
        let target = (6 + pop as i32).min(90);
        if owned >= target {
            return;
        }
        let mut claimed = 0;
        for h in center.spiral(6) {
            if claimed >= 3 {
                break;
            }
            let Some(t) = self.tile(h).copied() else { continue };
            if !t.is_land() || t.owner.is_some() || t.lava > 0 {
                continue;
            }
            // Must touch territory we already own.
            let touches = self
                .neighbors_of(h)
                .iter()
                .any(|nb| self.tile(*nb).map(|t| t.owner == Some(vid)).unwrap_or(false));
            if !touches {
                continue;
            }
            if let Some(tile) = self.tile_mut(h) {
                tile.owner = Some(vid);
                claimed += 1;
            }
        }
        // Villages upgrade their claim radius as they grow.
        if claimed > 0 {
            if let Some(v) = self.village_mut(vid) {
                v.claim_radius = v.claim_radius.max((pop as i32 / 6) + 3).min(9);
            }
        }
    }

    /// The village makes babies when it has food and room.
    fn village_growth_tick(&mut self, vid: u32) {
        let (pop, food, housing, race, center, _kingdom) = match self.village(vid) {
            Some(v) => (
                v.pop,
                v.food,
                v.housing(),
                v.race,
                v.center,
                v.kingdom,
            ),
            None => return,
        };
        if pop == 0 || pop >= housing {
            return;
        }
        if food <= pop as i32 / 2 {
            return; // no surplus, no children
        }
        let def = race.def();
        let mut chance = def.breed_chance * self.age.age.fertility_rate() as f32 / 100.0;
        if self.age.age == crate::ages::Age::Hope {
            chance *= 1.2;
        }
        if !self.rng.chance(chance) {
            return;
        }
        // A baby appears at the village centre.
        let Some(id) = self.spawn_unit_at(race, center, UnitKind::Civilian, Some(vid)) else {
            return;
        };
        if let Some(u) = self.unit_mut(id) {
            u.age = 0;
        }
        if let Some(v) = self.village_mut(vid) {
            v.pop += 1;
            v.births += 1;
            v.food -= 2;
        }
        if let Some(k) = self.village(vid).and_then(|v| v.kingdom) {
            if let Some(kd) = self.kingdom_mut(k) {
                kd.population += 1;
            }
        }
        self.stats.births += 1;
    }

    /// Loyalty drifts with distance from the capital, war, starvation and the age.
    fn village_loyalty_tick(&mut self, vid: u32) {
        let (kingdom, food, _pop, starving) = match self.village(vid) {
            Some(v) => (v.kingdom, v.food, v.pop, v.starving),
            None => return,
        };
        let age_bonus = self.age.age.loyalty_bonus();
        let mut delta = age_bonus / 4;
        if starving {
            // An empty larder sours a village faster than any age can sweeten it.
            delta -= 8;
        } else if food > 20 {
            delta += 1;
        }
        if let Some(kid) = kingdom {
            if let Some(k) = self.kingdom(kid) {
                if k.capital == vid {
                    delta += 3;
                } else if let Some(cap) = self.village(k.capital) {
                    let d = cap.center.distance(self.village(vid).map(|v| v.center).unwrap_or(cap.center));
                    // Distant cities feel neglected.
                    delta -= (d / 6).min(4);
                }
                if k.wars.len() > 1 {
                    delta -= 2; // too many wars
                }
                if k.king.is_none() {
                    delta -= 2; // interregnum
                }
                if k.motto.is_empty() {
                    delta += 0;
                }
            }
        }
        if self.age.age == crate::ages::Age::Chaos {
            delta -= 1;
        }
        if let Some(v) = self.village_mut(vid) {
            v.loyalty = (v.loyalty + delta as i16).clamp(-100, 100);
        }
    }

    /// Villages train soldiers when they can afford it and their kingdom is at war.
    fn village_soldier_tick(&mut self, vid: u32) {
        let (kingdom, pop, soldiers, barracked) = match self.village(vid) {
            Some(v) => (
                v.kingdom,
                v.pop,
                v.soldiers,
                v.has(BuildingKind::Barracks),
            ),
            None => return,
        };
        let at_war = kingdom
            .map(|k| self.kingdom(k).map(|k| !k.wars.is_empty()).unwrap_or(false))
            .unwrap_or(false);
        if !at_war || !barracked || self.tick % 8 != 0 {
            return;
        }
        let cap = self
            .village(vid)
            .map(|v| v.soldier_cap())
            .unwrap_or(0)
            .min((pop as u16).saturating_sub(2));
        if soldiers >= cap {
            return;
        }
        // Promote a civilian into a soldier.
        let candidate = self
            .units
            .iter()
            .filter(|u| u.alive && u.village == Some(vid) && u.kind == UnitKind::Civilian)
            .map(|u| u.id)
            .next();
        let Some(cid) = candidate else { return };
        let home = self
            .village(vid)
            .map(|v| v.center)
            .or_else(|| self.unit(cid).map(|u| u.pos));
        if let Some(u) = self.unit_mut(cid) {
            u.kind = UnitKind::Soldier;
            u.state = UnitState::Patrol;
            if let Some(h) = home {
                u.home = h;
            }
        }
        if let Some(v) = self.village_mut(vid) {
            v.soldiers = v.soldiers.saturating_add(1);
        }
    }

    /// Apply damage from fire, tornadoes or bombardment: buildings break.
    pub fn village_building_damage(&mut self, vid: u32, amount: u8) {
        if amount == 0 {
            return;
        }
        let len = self.village(vid).map(|v| v.buildings.len()).unwrap_or(0);
        if len == 0 {
            return;
        }
        let idx = self.rng.below(len as u32) as usize;
        let destroyed = {
            let Some(v) = self.village_mut(vid) else { return };
            {
                let b = v.buildings.remove(idx);
                if b.kind == BuildingKind::House && v.houses > 0 {
                    v.houses -= 1;
                }
                if b.kind == BuildingKind::TownHall {
                    v.town_hall_level = v.town_hall_level.saturating_sub(1);
                }
                if b.kind == BuildingKind::Wall {
                    v.walls = v.walls.saturating_sub(1);
                }
                true
            }
        };
        if destroyed {
            self.stats.disasters += 1;
        }
    }

    /// Remove a village: free its territory, kill the stragglers, tell the world.
    pub fn destroy_village(&mut self, vid: u32, reason: &str) {
        let (name, kingdom) = match self.village(vid) {
            Some(v) => (v.name.clone(), v.kingdom),
            None => return,
        };
        // Survivors become stateless wanderers (or die, if the reason says so).
        let residents = self
            .units
            .iter()
            .filter(|u| u.alive && u.village == Some(vid))
            .map(|u| u.id)
            .collect::<Vec<u32>>();
        for u in residents {
            if reason.contains("razed") || reason.contains("burned") {
                self.kill_unit(u, None);
            } else if let Some(unit) = self.unit_mut(u) {
                unit.village = None;
                unit.kingdom = None;
                unit.state = UnitState::Wander;
            }
        }
        for t in self.tiles.iter_mut() {
            if t.owner == Some(vid) {
                t.owner = None;
            }
        }
        self.free_village_slot(vid);
        if let Some(k) = kingdom {
            let empty = {
                if let Some(kd) = self.kingdom_mut(k) {
                    kd.cities.retain(|c| *c != vid);
                    kd.cities.is_empty()
                } else {
                    false
                }
            };
            if empty {
                self.destroy_kingdom(k, &format!("{name} was lost"));
            }
        }
        self.chronicle(
            EventKind::Death,
            format!("{name} is no more: {reason}"),
        );
    }

    /// Every building kind with its count, for the CLI inspector.
    pub fn building_census(&self, vid: u32) -> Vec<(BuildingKind, u32)> {
        let mut out = Vec::new();
        let Some(v) = self.village(vid) else {
            return out;
        };
        for kind in BuildingKind::ALL {
            let n = v.count(kind);
            if n > 0 {
                out.push((kind, n));
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worldgen::{GenParams, WorldType};

    fn world() -> World {
        World::generate(GenParams::new(40, 30, 99, WorldType::Continents))
    }

    /// Spawn a handful of founders and found a village where they stand.
    fn seed_village(w: &mut World, race: Race) -> u32 {
        let mut spot = None;
        for _ in 0..400 {
            let h = w.random_land_tile().unwrap();
            if w.site_score(h, race, None) > 50 {
                spot = Some(h);
                break;
            }
        }
        let spot = spot.unwrap_or_else(|| w.random_land_tile().unwrap());
        let mut founders = Vec::new();
        for _ in 0..FOUNDING_POP {
            if let Some(id) = w.spawn_unit(race, spot, UnitKind::Civilian) {
                founders.push(id);
            }
        }
        w.found_village(spot, race, &founders, None, None).unwrap()
    }

    #[test]
    fn villages_start_with_a_fireplace_and_claim_land() {
        let mut w = world();
        let vid = seed_village(&mut w, Race::Human);
        let v = w.village(vid).unwrap();
        assert!(v.has(BuildingKind::Fireplace));
        assert!(v.pop >= 6);
        let owned = w.tiles.iter().filter(|t| t.owner == Some(vid)).count();
        assert!(owned >= 5, "a new village should claim its surroundings");
        assert!(w.chronicle.iter().any(|e| e.kind == EventKind::VillageFounded));
    }

    #[test]
    fn villages_do_not_overlap_when_founded_far_apart() {
        let mut w = world();
        let a = seed_village(&mut w, Race::Human);
        let b_spot = w
            .best_village_site(w.village(a).unwrap().center, Race::Human, 8, 14)
            .unwrap();
        let mut founders = Vec::new();
        for _ in 0..FOUNDING_POP {
            if let Some(id) = w.spawn_unit(Race::Human, b_spot, UnitKind::Civilian) {
                founders.push(id);
            }
        }
        let b = w.found_village(b_spot, Race::Human, &founders, None, None).unwrap();
        assert_ne!(a, b);
        assert!(w.village(a).unwrap().center.distance(w.village(b).unwrap().center) >= 8);
    }

    #[test]
    fn founding_on_owned_land_fails() {
        let mut w = world();
        let vid = seed_village(&mut w, Race::Human);
        let center = w.village(vid).unwrap().center;
        let mut founders = Vec::new();
        for _ in 0..FOUNDING_POP {
            if let Some(id) = w.spawn_unit(Race::Human, center, UnitKind::Civilian) {
                founders.push(id);
            }
        }
        assert!(w.found_village(center, Race::Human, &founders, None, None).is_none());
    }

    #[test]
    fn a_village_with_food_and_housing_grows() {
        let mut w = world();
        let vid = seed_village(&mut w, Race::Human);
        // Give it a larder and housing so growth is not gated.
        {
            let v = w.village_mut(vid).unwrap();
            v.food = 500;
            v.houses = 10;
        }
        let before = w.village(vid).unwrap().pop;
        for _ in 0..400 {
            w.village_growth_tick(vid);
        }
        let after = w.village(vid).unwrap().pop;
        assert!(after > before, "population should grow: {before} -> {after}");
        assert!(w.stats.births > 0);
    }

    #[test]
    fn housing_caps_population() {
        let mut w = world();
        let vid = seed_village(&mut w, Race::Human);
        {
            let v = w.village_mut(vid).unwrap();
            v.food = 1000;
            v.houses = 0;
            v.town_hall_level = 0;
        }
        let cap = w.village(vid).unwrap().housing();
        for _ in 0..600 {
            w.village_growth_tick(vid);
        }
        assert!(
            w.village(vid).unwrap().pop <= cap,
            "population {} exceeded housing {cap}",
            w.village(vid).unwrap().pop
        );
    }

    #[test]
    fn building_progress_pays_for_itself_and_completes() {
        let mut w = world();
        let vid = seed_village(&mut w, Race::Human);
        {
            let v = w.village_mut(vid).unwrap();
            v.wood = 200;
            v.stone = 200;
            v.ore = 50;
            v.food = 200;
            v.houses = 2;
        }
        for _ in 0..600 {
            w.village_build_tick(vid);
        }
        let v = w.village(vid).unwrap();
        assert!(v.buildings.len() > 1, "the village should have built by now");
        assert!(w.stats.buildings_built > 0);
    }

    #[test]
    fn territory_grows_with_population() {
        let mut w = world();
        let vid = seed_village(&mut w, Race::Human);
        for _ in 0..20 {
            w.village_claim_tick(vid);
        }
        let owned_small = w.tiles.iter().filter(|t| t.owner == Some(vid)).count();
        for _ in 0..60 {
            w.village_claim_tick(vid);
        }
        let owned_big = w.tiles.iter().filter(|t| t.owner == Some(vid)).count();
        assert!(owned_big >= owned_small);
        assert!(
            owned_big <= 6 + w.village(vid).unwrap().pop as usize + 12,
            "claims should stay proportional to population"
        );
    }

    #[test]
    fn seed_life_founds_spread_out_villages_and_wildlife() {
        let mut w = world();
        let vids = w.seed_life(4, 30, 2);
        assert!(
            vids.len() >= 3,
            "expected several villages, founded {}",
            vids.len()
        );
        for vid in &vids {
            let v = w.village(*vid).unwrap();
            assert!(v.alive);
            assert!(v.pop >= FOUNDING_POP, "village {vid} has only {} people", v.pop);
            assert!(w.tile(v.center).map(|t| t.is_land()).unwrap_or(false));
        }
        for (i, a) in vids.iter().enumerate() {
            for b in vids.iter().skip(i + 1) {
                let (ca, cb) = (
                    w.village(*a).unwrap().center,
                    w.village(*b).unwrap().center,
                );
                assert!(
                    ca.distance(cb) >= 12,
                    "villages {a} and {b} were planted on top of each other"
                );
            }
        }
        let animals = w
            .units
            .iter()
            .filter(|u| u.alive && u.race.is_animal())
            .count();
        let monsters = w
            .units
            .iter()
            .filter(|u| u.alive && !u.race.is_animal() && u.race.is_monster())
            .count();
        assert!(animals >= 20, "only {animals} animals appeared");
        assert_eq!(monsters, 2);
        // Same seed, same life.
        let mut again = world();
        again.seed_life(4, 30, 2);
        assert_eq!(again.state_hash(), w.state_hash());
    }

    #[test]
    fn a_seeded_world_grows_on_its_own() {
        let mut w = world();
        let vids = w.seed_life(4, 24, 0);
        let start = w.population();
        for _ in 0..1200 {
            w.step();
        }
        assert!(
            w.population() > start,
            "population went from {start} to {}",
            w.population()
        );
        assert!(
            vids.iter().any(|v| w.village(*v).map(|v| v.pop > FOUNDING_POP).unwrap_or(false)),
            "no village grew past its founding band"
        );
        assert!(!w.village_ids().is_empty(), "every village died out");
    }

    #[test]
    fn starving_villages_lose_people_and_loyalty() {
        let mut w = world();
        let vid = seed_village(&mut w, Race::Human);
        {
            let v = w.village_mut(vid).unwrap();
            v.food = 0;
        }
        // A village whose land produces nothing at all: the larder is empty
        // every morning, however much anyone scrounges.
        let pop_before = w.units.iter().filter(|u| u.alive).count();
        for _ in 0..600 {
            if let Some(v) = w.village_mut(vid) {
                v.food = 0;
                v.buildings.retain(|b| b.kind != BuildingKind::Farm);
            }
            w.village_food_tick(vid);
            w.village_loyalty_tick(vid);
            assert!(
                w.village(vid).map(|v| v.starving).unwrap_or(false),
                "an empty larder means hunger"
            );
        }
        let v = w.village(vid).unwrap();
        assert!(v.loyalty < 50, "starvation should cost loyalty");
        let pop_now = w.units.iter().filter(|u| u.alive).count();
        assert!(
            pop_now < pop_before,
            "people should die of hunger: {pop_before} -> {pop_now}"
        );
    }

    #[test]
    fn destroying_a_village_frees_its_land() {
        let mut w = world();
        let vid = seed_village(&mut w, Race::Human);
        assert!(w.tiles.iter().any(|t| t.owner == Some(vid)));
        w.destroy_village(vid, "test");
        assert!(w.village(vid).is_none());
        assert!(!w.tiles.iter().any(|t| t.owner == Some(vid)));
    }

    #[test]
    fn site_scoring_prefers_good_land_over_lava() {
        let mut w = world();
        let good = w.random_land_tile().unwrap();
        let lava = w.random_land_tile().unwrap();
        w.set_biome(lava, crate::terrain::Biome::Lava);
        assert!(w.site_score(good, Race::Human, None) > w.site_score(lava, Race::Human, None));
    }
}
