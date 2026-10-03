//! Kingdoms: diplomacy, wars, rebellions and the fall of empires.
//!
//! A kingdom is a set of villages with a capital and a king. Relations are stored
//! per kingdom as a sorted list of `(other id, opinion)`, so a single kingdom's
//! view of the world can drift (hate after a border dispute, gratitude after a
//! shared war) without a global matrix.
//!
//! `route: reimplementation` — the rules below are our own model of the genre:
//! opinion drift from race tension and border friction, wars declared when
//! opinion collapses, peace when both sides are exhausted, and rebellions when a
//! distant city's loyalty rots.

use crate::hex::Hex;
use crate::names;
use crate::races::Race;
use crate::units::{UnitKind, UnitState};
use crate::village::{BuildingKind, SETTLER_POP};
use crate::world::{EventKind, Fnv, World};

/// Population at which a village with a town hall crowns its own king.
pub const CROWN_POP: u32 = 10;

/// Culture leans, drawn at founding, that nudge a kingdom's behaviour.
pub mod culture {
    pub const EXPANSIONIST: u32 = 1 << 0;
    pub const MILITARIST: u32 = 1 << 1;
    pub const PEACEFUL: u32 = 1 << 2;
    pub const SCHOLARS: u32 = 1 << 3;
    pub const TRADERS: u32 = 1 << 4;

    pub fn names(bits: u32) -> Vec<&'static str> {
        let mut out = Vec::new();
        for (bit, name) in [
            (EXPANSIONIST, "expansionist"),
            (MILITARIST, "militarist"),
            (PEACEFUL, "peaceful"),
            (SCHOLARS, "scholars"),
            (TRADERS, "traders"),
        ] {
            if bits & bit != 0 {
                out.push(name);
            }
        }
        out
    }
}

/// One active war, with the year it started (used for peace negotiations).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct War {
    pub enemy: u32,
    pub start_year: u32,
}

/// A realm.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Kingdom {
    pub id: u32,
    pub alive: bool,
    pub name: String,
    pub race: Race,
    pub color: (u8, u8, u8),
    pub capital: u32,
    pub king: Option<u32>,
    pub king_name: String,
    pub founded_year: u32,
    pub cities: Vec<u32>,
    pub population: u32,
    pub wars: Vec<War>,
    pub allies: Vec<u32>,
    /// Sorted `(kingdom id, opinion -100..100)`.
    pub relations: Vec<(u32, i16)>,
    pub motto: String,
    pub culture: u32,
    /// 0..=100, drifts with culture and wars.
    pub aggression: i16,
    pub wars_won: u32,
    pub wars_lost: u32,
    pub cities_captured: u32,
    pub rebellions_survived: u32,
}

impl Default for Kingdom {
    fn default() -> Self {
        Kingdom {
            id: 0,
            alive: true,
            name: String::new(),
            race: Race::Human,
            color: (200, 200, 200),
            capital: 0,
            king: None,
            king_name: String::new(),
            founded_year: 0,
            cities: Vec::new(),
            population: 0,
            wars: Vec::new(),
            allies: Vec::new(),
            relations: Vec::new(),
            motto: String::new(),
            culture: 0,
            aggression: 30,
            wars_won: 0,
            wars_lost: 0,
            cities_captured: 0,
            rebellions_survived: 0,
        }
    }
}

impl Kingdom {
    pub fn new(id: u32, name: String, race: Race, capital: u32, year: u32) -> Self {
        let color = banner_color(race, id);
        Kingdom {
            id,
            name,
            race,
            color,
            capital,
            founded_year: year,
            relations: Vec::new(),
            ..Default::default()
        }
    }

    pub fn opinion(&self, other: u32) -> i16 {
        self.relations
            .iter()
            .find(|(k, _)| *k == other)
            .map(|(_, o)| *o)
            .unwrap_or(0)
    }

    /// Set the opinion, keeping `relations` sorted by kingdom id.
    pub fn set_opinion(&mut self, other: u32, value: i16) {
        let value = value.clamp(-100, 100);
        match self.relations.binary_search_by_key(&other, |(k, _)| *k) {
            Ok(i) => self.relations[i].1 = value,
            Err(i) => self.relations.insert(i, (other, value)),
        }
    }

    pub fn add_opinion(&mut self, other: u32, delta: i16) {
        let now = self.opinion(other);
        self.set_opinion(other, now + delta);
    }

    pub fn at_war_with(&self, other: u32) -> bool {
        self.wars.iter().any(|w| w.enemy == other)
    }

    pub fn war_start(&self, other: u32) -> Option<u32> {
        self.wars
            .iter()
            .find(|w| w.enemy == other)
            .map(|w| w.start_year)
    }

    pub fn is_allied_with(&self, other: u32) -> bool {
        self.allies.contains(&other)
    }

    pub fn has_culture(&self, bit: u32) -> bool {
        self.culture & bit != 0
    }

    pub fn describe(&self) -> String {
        format!(
            "#{} {} ({}) capital {} pop {} cities {} king {} wars {} allies {} aggression {}",
            self.id,
            self.name,
            self.race.name(),
            self.capital,
            self.population,
            self.cities.len(),
            if self.king_name.is_empty() {
                "none".to_string()
            } else {
                self.king_name.clone()
            },
            self.wars.len(),
            self.allies.len(),
            self.aggression
        )
    }

    /// Culture line for the CLI.
    pub fn culture_line(&self) -> String {
        let c = culture::names(self.culture);
        if c.is_empty() {
            "no strong culture".to_string()
        } else {
            c.join(", ")
        }
    }

    pub fn hash_into(&self, h: &mut Fnv) {
        h.u64(self.id as u64);
        h.u64(self.alive as u64);
        h.str(&self.name);
        h.u64(self.race as u64);
        h.u64(self.capital as u64);
        h.u64(self.king.map(|k| k as u64 + 1).unwrap_or(0));
        h.str(&self.king_name);
        h.u64(self.founded_year as u64);
        h.u64(self.cities.len() as u64);
        for c in &self.cities {
            h.u64(*c as u64);
        }
        h.u64(self.population as u64);
        h.u64(self.wars.len() as u64);
        for w in &self.wars {
            h.u64(w.enemy as u64);
            h.u64(w.start_year as u64);
        }
        h.u64(self.allies.len() as u64);
        for a in &self.allies {
            h.u64(*a as u64);
        }
        h.u64(self.relations.len() as u64);
        for (k, o) in &self.relations {
            h.u64(*k as u64);
            h.u64(*o as i64 as u64);
        }
        h.u64(self.culture as u64);
        h.u64(self.aggression as i64 as u64);
        h.u64(self.wars_won as u64);
        h.u64(self.wars_lost as u64);
        h.u64(self.cities_captured as u64);
    }
}

/// Banner colour, biased by race so a map reads at a glance.
fn banner_color(race: Race, id: u32) -> (u8, u8, u8) {
    let base: (u8, u8, u8) = match race {
        Race::Human => (70, 120, 220),
        Race::Elf => (90, 200, 120),
        Race::Dwarf => (200, 150, 60),
        Race::Orc => (160, 60, 60),
        Race::Bandit => (140, 120, 100),
        _ => (170, 170, 170),
    };
    // Vary the shade a little per kingdom id, deterministically.
    let shift = ((id as i32 * 37) % 40) - 20;
    let f = |c: u8| ((c as i32 + shift).clamp(30, 245)) as u8;
    (f(base.0), f(base.1), f(base.2))
}

impl World {
    /// Crown a capital: create the kingdom that rules this village.
    pub fn found_kingdom(&mut self, capital_village: u32, race: Race) -> Option<u32> {
        let (capital_name, leader) = {
            let v = self.village(capital_village)?;
            (v.name.clone(), v.leader)
        };
        let name = names::kingdom_name(&mut self.rng, race, &capital_name);
        let motto = names::motto(&mut self.rng).to_string();
        let mut k = Kingdom::new(0, name.clone(), race, capital_village, self.year);
        k.motto = motto;
        k.culture = match race {
            Race::Human => culture::EXPANSIONIST | culture::TRADERS,
            Race::Orc => culture::MILITARIST | culture::EXPANSIONIST,
            Race::Elf => culture::PEACEFUL | culture::SCHOLARS,
            Race::Dwarf => culture::SCHOLARS | culture::TRADERS,
            _ => culture::EXPANSIONIST,
        };
        k.aggression = match race {
            Race::Orc => 70,
            Race::Bandit => 80,
            Race::Human => 45,
            Race::Dwarf => 35,
            Race::Elf => 25,
            _ => 40,
        };
        let kid = self.alloc_kingdom_slot(k);
        if let Some(v) = self.village_mut(capital_village) {
            v.kingdom = Some(kid);
            v.capital = true;
            v.loyalty = v.loyalty.max(60);
        }
        let subjects: Vec<u32> = self
            .units
            .iter()
            .filter(|u| u.alive && u.village == Some(capital_village))
            .map(|u| u.id)
            .collect();
        for uid in subjects {
            if let Some(u) = self.unit_mut(uid) {
                u.kingdom = Some(kid);
            }
        }
        // The village leader becomes king.
        if let Some(lid) = leader {
            let ruler = if let Some(u) = self.unit_mut(lid) {
                u.kind = UnitKind::King;
                if u.name.is_empty() {
                    names::person_name(&mut self.rng)
                } else {
                    format!("King {}", u.name)
                }
            } else {
                names::person_name(&mut self.rng)
            };
            if let Some(k) = self.kingdom_mut(kid) {
                k.king = Some(lid);
                k.king_name = ruler;
                k.cities.push(capital_village);
            }
        } else if let Some(k) = self.kingdom_mut(kid) {
            k.cities.push(capital_village);
        }
        self.stats.kingdoms_founded += 1;
        self.chronicle(
            EventKind::KingdomFounded,
            format!("{name} rises, ruled from {capital_name}"),
        );
        Some(kid)
    }

    /// Opinion of `a` toward `b`, and vice versa.
    pub fn opinions(&self, a: u32, b: u32) -> (i16, i16) {
        let ab = self.kingdom(a).map(|k| k.opinion(b)).unwrap_or(0);
        let ba = self.kingdom(b).map(|k| k.opinion(a)).unwrap_or(0);
        (ab, ba)
    }

    /// Declare war in both directions. Idempotent.
    pub fn declare_war(&mut self, a: u32, b: u32) {
        if a == b {
            return;
        }
        let year = self.year;
        let (a_name, b_name) = match (self.kingdom(a), self.kingdom(b)) {
            (Some(ka), Some(kb)) => (ka.name.clone(), kb.name.clone()),
            _ => return,
        };
        let mut fresh = false;
        if let Some(ka) = self.kingdom_mut(a) {
            if !ka.at_war_with(b) {
                ka.wars.push(War {
                    enemy: b,
                    start_year: year,
                });
                ka.wars.sort_by_key(|w| w.enemy);
                fresh = true;
            }
            ka.add_opinion(b, -30);
            ka.allies.retain(|x| *x != b);
        }
        if let Some(kb) = self.kingdom_mut(b) {
            if !kb.at_war_with(a) {
                kb.wars.push(War {
                    enemy: a,
                    start_year: year,
                });
                kb.wars.sort_by_key(|w| w.enemy);
                fresh = true;
            }
            kb.add_opinion(a, -30);
            kb.allies.retain(|x| *x != a);
        }
        if fresh {
            self.stats.wars_declared += 1;
            self.chronicle(
                EventKind::War,
                format!("{a_name} declares war on {b_name}"),
            );
        }
    }

    /// End a war. No-op when they are not at war.
    pub fn make_peace(&mut self, a: u32, b: u32) {
        let (a_name, b_name) = match (self.kingdom(a), self.kingdom(b)) {
            (Some(ka), Some(kb)) => (ka.name.clone(), kb.name.clone()),
            _ => return,
        };
        let mut was_at_war = false;
        if let Some(ka) = self.kingdom_mut(a) {
            was_at_war |= ka.at_war_with(b);
            ka.wars.retain(|w| w.enemy != b);
            ka.add_opinion(b, 20);
        }
        if let Some(kb) = self.kingdom_mut(b) {
            was_at_war |= kb.at_war_with(a);
            kb.wars.retain(|w| w.enemy != a);
            kb.add_opinion(a, 20);
        }
        if was_at_war {
            self.stats.wars_ended += 1;
            self.chronicle(
                EventKind::Peace,
                format!("{a_name} and {b_name} make peace"),
            );
        }
    }

    /// Ally two kingdoms.
    pub fn form_alliance(&mut self, a: u32, b: u32) {
        if a == b {
            return;
        }
        let (a_name, b_name) = match (self.kingdom(a), self.kingdom(b)) {
            (Some(ka), Some(kb)) => (ka.name.clone(), kb.name.clone()),
            _ => return,
        };
        if let Some(ka) = self.kingdom_mut(a) {
            if !ka.allies.contains(&b) {
                ka.allies.push(b);
                ka.allies.sort_unstable();
                ka.add_opinion(b, 30);
            }
        }
        if let Some(kb) = self.kingdom_mut(b) {
            if !kb.allies.contains(&a) {
                kb.allies.push(a);
                kb.allies.sort_unstable();
                kb.add_opinion(a, 30);
            }
        }
        self.stats.alliances += 1;
        self.chronicle(
            EventKind::Alliance,
            format!("{a_name} and {b_name} swear an alliance"),
        );
    }

    /// A city changes hands.
    pub fn capture_city(&mut self, attacker: u32, village: u32) {
        let (old_kingdom, city_name) = match self.village(village) {
            Some(v) => (v.kingdom, v.name.clone()),
            None => return,
        };
        let attacker_name = self
            .kingdom(attacker)
            .map(|k| k.name.clone())
            .unwrap_or_else(|| "the horde".to_string());
        // Bandit kingdoms and monsters raze instead of ruling.
        let razes = self
            .kingdom(attacker)
            .map(|k| k.race == Race::Bandit)
            .unwrap_or(true);
        if razes {
            self.destroy_village(village, "razed by raiders");
            return;
        }
        // Some of the population does not survive the sack.
        let victims: Vec<u32> = self
            .units
            .iter()
            .filter(|u| u.alive && u.village == Some(village))
            .map(|u| u.id)
            .collect();
        let kill_count = victims.len() / 3;
        for id in victims.into_iter().take(kill_count) {
            self.kill_unit(id, None);
        }
        if let Some(v) = self.village_mut(village) {
            v.kingdom = Some(attacker);
            v.loyalty = -40;
            v.siege = 0;
            v.besieged_by = None;
            v.capital = false;
        }
        for u in self.units.iter_mut() {
            if u.alive && u.village == Some(village) {
                u.kingdom = Some(attacker);
                u.state = UnitState::Wander;
            }
        }
        if let Some(k) = self.kingdom_mut(attacker) {
            if !k.cities.contains(&village) {
                k.cities.push(village);
                k.cities.sort_unstable();
            }
            k.cities_captured += 1;
        }
        if let Some(old) = old_kingdom {
            let empty = {
                if let Some(k) = self.kingdom_mut(old) {
                    k.cities.retain(|c| *c != village);
                    k.wars_lost += 1;
                    k.cities.is_empty()
                } else {
                    false
                }
            };
            // The losing side picks a new capital, or dies.
            if !empty {
                let new_capital = self
                    .kingdom(old)
                    .and_then(|k| k.cities.first().copied());
                if let Some(cap) = new_capital {
                    if let Some(k) = self.kingdom_mut(old) {
                        k.capital = cap;
                    }
                    if let Some(v) = self.village_mut(cap) {
                        v.capital = true;
                    }
                }
            } else {
                self.destroy_kingdom(old, "no cities remain");
            }
        }
        if let Some(k) = self.kingdom_mut(attacker) {
            k.wars_won += 1;
        }
        self.stats.cities_captured += 1;
        self.chronicle(
            EventKind::CityCaptured,
            format!("{attacker_name} sacks {city_name}"),
        );
    }

    /// A village breaks away and forms its own kingdom, usually dragging
    /// neighbours with it.
    pub fn rebel(&mut self, village: u32) {
        let (old_kingdom, center, race, name) = match self.village(village) {
            Some(v) => (v.kingdom, v.center, v.race, v.name.clone()),
            None => return,
        };
        let Some(old) = old_kingdom else { return };
        // The rebel city becomes independent first, then gets a crown.
        if let Some(v) = self.village_mut(village) {
            v.kingdom = None;
            v.loyalty = 20;
        }
        let Some(new_kingdom) = self.found_kingdom(village, race) else {
            return;
        };
        if let Some(k) = self.kingdom_mut(new_kingdom) {
            k.aggression = (k.aggression + 25).min(100);
        }
        // Neighbours with poor loyalty may join the revolt.
        let mut joined = 0;
        let neighbours: Vec<u32> = self
            .villages
            .iter()
            .filter(|v| {
                v.alive
                    && v.kingdom == Some(old)
                    && v.id != village
                    && v.center.distance(center) <= 6
                    && v.loyalty < 10
            })
            .map(|v| v.id)
            .collect();
        for vid in neighbours {
            if joined >= 3 {
                break;
            }
            if self.rng.chance(0.6) {
                self.transfer_village(vid, new_kingdom);
                joined += 1;
            }
        }
        if let Some(k) = self.kingdom_mut(old) {
            k.cities.retain(|c| c != &village);
            k.rebellions_survived += 1;
        }
        self.declare_war(old, new_kingdom);
        self.stats.rebellions += 1;
        self.chronicle(
            EventKind::Rebellion,
            format!("{name} rises in rebellion, founding a new realm"),
        );
    }

    /// Move a village (and its people) to another kingdom.
    pub fn transfer_village(&mut self, village: u32, kingdom: u32) {
        let (name, old) = match self.village(village) {
            Some(v) => (v.name.clone(), v.kingdom),
            None => return,
        };
        if let Some(v) = self.village_mut(village) {
            v.kingdom = Some(kingdom);
            v.loyalty = 10;
        }
        for u in self.units.iter_mut() {
            if u.alive && u.village == Some(village) {
                u.kingdom = Some(kingdom);
            }
        }
        if let Some(k) = self.kingdom_mut(kingdom) {
            if !k.cities.contains(&village) {
                k.cities.push(village);
                k.cities.sort_unstable();
            }
        }
        if let Some(old) = old {
            if let Some(k) = self.kingdom_mut(old) {
                k.cities.retain(|c| *c != village);
            }
        }
        self.chronicle(
            EventKind::Rebellion,
            format!("{name} joins another realm"),
        );
    }

    /// Remove a kingdom.
    pub fn destroy_kingdom(&mut self, id: u32, reason: &str) {
        let name = match self.kingdom(id) {
            Some(k) => k.name.clone(),
            None => return,
        };
        let cities: Vec<u32> = self
            .kingdom(id)
            .map(|k| k.cities.clone())
            .unwrap_or_default();
        for c in cities {
            self.destroy_village(c, reason);
        }
        let king = self.kingdom(id).and_then(|k| k.king);
        if let Some(kid) = king {
            if let Some(u) = self.unit_mut(kid) {
                u.kind = UnitKind::Civilian;
            }
        }
        // Everyone at war with it is no longer at war with it.
        let others = self.kingdom_ids();
        for o in others {
            if o == id {
                continue;
            }
            if let Some(k) = self.kingdom_mut(o) {
                k.wars.retain(|w| w.enemy != id);
                k.allies.retain(|a| *a != id);
                k.relations.retain(|(r, _)| *r != id);
            }
        }
        self.free_kingdom_slot(id);
        self.stats.kingdoms_fallen += 1;
        self.chronicle(EventKind::KingdomFallen, format!("{name} falls: {reason}"));
    }

    // --- per-tick systems --------------------------------------------------

    /// One tick of kingdoms: census, kings, diplomacy, wars and expansions.
    pub fn kingdom_tick(&mut self) {
        let ids = self.kingdom_ids();
        // --- census and succession ---
        for kid in &ids {
            let cities: Vec<u32> = self
                .kingdom(*kid)
                .map(|k| k.cities.clone())
                .unwrap_or_default();
            let mut live = Vec::new();
            let mut pop = 0u32;
            for c in cities {
                if let Some(v) = self.village(c) {
                    live.push(c);
                    pop += v.pop;
                }
            }
            let mut empty = false;
            if let Some(k) = self.kingdom_mut(*kid) {
                k.cities = live;
                k.population = pop;
                empty = k.cities.is_empty();
            }
            if empty {
                self.destroy_kingdom(*kid, "its last city was lost");
                continue;
            }
            // Succession: a dead king is replaced by the capital's leader, then by
            // the most experienced soldier.
            let king_alive = self
                .kingdom(*kid)
                .and_then(|k| k.king)
                .map(|k| self.unit(k).is_some())
                .unwrap_or(false);
            if !king_alive {
                let (capital, cities) = self
                    .kingdom(*kid)
                    .map(|k| (k.capital, k.cities.clone()))
                    .unwrap_or((0, Vec::new()));
                // The throne passes to the capital's leader first; failing that, to
                // the most seasoned soldier in any of the kingdom's towns.
                let heir = self
                    .village(capital)
                    .and_then(|v| v.leader)
                    .filter(|l| self.unit(*l).is_some())
                    .or_else(|| {
                        self.units
                            .iter()
                            .filter(|u| {
                                u.alive
                                    && u.race.is_civilized()
                                    && (u.kingdom == Some(*kid)
                                        || u.village
                                            .map(|v| cities.contains(&v))
                                            .unwrap_or(false))
                            })
                            .max_by_key(|u| (u.level, u.kills, u.id))
                            .map(|u| u.id)
                    });
                if let Some(h) = heir {
                    let ruler = names::person_name(&mut self.rng);
                    if let Some(u) = self.unit_mut(h) {
                        u.kind = UnitKind::King;
                        u.name = ruler.clone();
                    }
                    if let Some(k) = self.kingdom_mut(*kid) {
                        k.king = Some(h);
                        k.king_name = ruler;
                    }
                } else if let Some(k) = self.kingdom_mut(*kid) {
                    // No one is left to take the crown.
                    k.king = None;
                }
            }
        }
        // --- coronations ---
        // A village that has grown past a town hall's worth of people crowns its
        // own king. This is how kingdoms appear without a god's finger.
        if self.tick % 20 == 0 {
            for vid in self.village_ids() {
                let crown = match self.village(vid) {
                    Some(v) => {
                        v.kingdom.is_none()
                            && v.pop >= CROWN_POP
                            && v.has(BuildingKind::TownHall)
                    }
                    None => false,
                };
                if !crown {
                    continue;
                }
                let race = self.village(vid).map(|v| v.race).unwrap_or(Race::Human);
                self.found_kingdom(vid, race);
            }
        }
        // --- diplomacy ---
        let ids = self.kingdom_ids();
        for (i, a) in ids.iter().enumerate() {
            for b in ids.iter().skip(i + 1) {
                self.diplomacy_pair(*a, *b);
            }
        }
        // --- expansion ---
        self.expansion_tick();
        // --- wars ---
        self.war_tick();
    }

    /// Relations between two kingdoms drift, and wars and alliances follow.
    fn diplomacy_pair(&mut self, a: u32, b: u32) {
        let (ka, kb) = match (self.kingdom(a), self.kingdom(b)) {
            (Some(ka), Some(kb)) => (ka.clone(), kb.clone()),
            _ => return,
        };
        if ka.race == kb.race && ka.at_war_with(b) {
            // Same-race wars are unpopular at home.
        }
        let mut delta: i16 = 0;
        delta -= (ka.race.tension(kb.race) / 8) as i16;
        delta += match (ka.is_allied_with(b), kb.is_allied_with(a)) {
            (true, true) => 3,
            _ => 0,
        };
        if ka.at_war_with(b) {
            delta -= 3;
        }
        // Shared enemies bring kingdoms together.
        let shared_enemy = ka.wars.iter().any(|w| kb.at_war_with(w.enemy));
        if shared_enemy {
            delta += 2;
        }
        // Border friction: close capitals with big territories rub along badly.
        let (ca, cb) = (
            self.village(ka.capital).map(|v| v.center),
            self.village(kb.capital).map(|v| v.center),
        );
        if let (Some(ca), Some(cb)) = (ca, cb) {
            let d = ca.distance(cb);
            if d < 14 {
                delta -= 1;
            } else if d > 40 {
                delta += 1;
            }
        }
        delta += (self.age.age.war_bonus() / 5) as i16;
        delta += self.rng.range(-1, 1) as i16;
        let (ab, ba) = (ka.opinion(b) + delta, kb.opinion(a) + delta);
        if let Some(k) = self.kingdom_mut(a) {
            k.set_opinion(b, ab);
        }
        if let Some(k) = self.kingdom_mut(b) {
            k.set_opinion(a, ba);
        }
        // Peace when both sides are tired or relations recovered.
        if ka.at_war_with(b) {
            let start = ka.war_start(b).unwrap_or(self.year);
            let years = self.year.saturating_sub(start);
            let tired = years > 40 || ka.population < 6 || kb.population < 6;
            let recovered = ab > -10 && ba > -10;
            let peaceful = ka.has_culture(culture::PEACEFUL) || kb.has_culture(culture::PEACEFUL);
            let chance = if tired {
                0.20
            } else if recovered {
                0.12
            } else if peaceful {
                0.06
            } else {
                0.015
            };
            if self.rng.chance(chance) {
                self.make_peace(a, b);
            }
            return;
        }
        // Alliance when they like each other and share an enemy.
        if !ka.is_allied_with(b)
            && ab > 55
            && ba > 55
            && shared_enemy
            && self.rng.chance(0.05)
        {
            self.form_alliance(a, b);
            return;
        }
        // War when opinion collapses. Hope-era kings are calmer.
        let aggression =
            (ka.aggression as i32 + kb.aggression as i32 + self.age.age.war_bonus()) as f32 / 3.0;
        let hate = (-ab).max(-ba) as f32;
        if hate > 45.0 {
            let chance = (hate - 45.0) / 900.0 * (1.0 + aggression / 50.0);
            if self.rng.chance(chance) {
                self.declare_war(a, b);
            }
        }
        // A kingdom that hates another can also join its enemy's war.
        if ab < -60 && self.rng.chance(0.02) {
            if let Some(target) = self.kingdom(b).and_then(|k| k.wars.first().map(|w| w.enemy)) {
                if target != a && self.kingdom(target).is_some() {
                    self.declare_war(a, target);
                }
            }
        }
    }

    /// Villages send settlers when they are crowded; kingdoms push their borders.
    fn expansion_tick(&mut self) {
        if self.tick % 30 != 0 {
            return;
        }
        let ids = self.village_ids();
        for vid in ids {
            let (pop, food, kingdom, race, starving) = match self.village(vid) {
                Some(v) => (v.pop, v.food, v.kingdom, v.race, v.starving),
                None => continue,
            };
            if starving || pop < SETTLER_POP || (food as u32) < pop {
                continue;
            }
            // Expansionist and crowded villages settle most eagerly.
            let mut chance = if pop >= SETTLER_POP + 6 { 0.05 } else { 0.015 };
            if let Some(k) = kingdom.and_then(|k| self.kingdom(k)) {
                if k.has_culture(culture::EXPANSIONIST) {
                    chance *= 2.0;
                }
                if k.cities.len() as u32 >= 12 {
                    chance *= 0.5;
                }
            }
            if !self.rng.chance(chance) {
                continue;
            }
            let _ = race;
            self.send_settlers(vid);
        }
    }

    /// Soldiers march, besiege and storm cities.
    fn war_tick(&mut self) {
        let ids = self.kingdom_ids();
        for kid in ids {
            let wars: Vec<u32> = match self.kingdom(kid) {
                Some(k) => k.wars.iter().map(|w| w.enemy).collect(),
                None => continue,
            };
            if wars.is_empty() {
                continue;
            }
            // Where this kingdom is fighting: the nearest enemy city.
            let home = self
                .kingdom(kid)
                .and_then(|k| self.village(k.capital).map(|v| v.center))
                .or_else(|| {
                    self.kingdom(kid)
                        .and_then(|k| k.cities.first().and_then(|c| self.village(*c).map(|v| v.center)))
                });
            let mut target: Option<(i32, u32)> = None;
            for enemy in &wars {
                let cities: Vec<(Hex, u32)> = self
                    .kingdom(*enemy)
                    .map(|k| k.cities.clone())
                    .unwrap_or_default()
                    .into_iter()
                    .filter_map(|c| self.village(c).map(|v| (v.center, c)))
                    .collect();
                for (center, cid) in cities {
                    let d = match home {
                        Some(h) => h.distance(center),
                        None => 0,
                    };
                    if target.is_none() || d < target.unwrap().0 {
                        target = Some((d, cid));
                    }
                }
            }
            let Some((_, city)) = target else { continue };
            // Send the army.
            let soldiers: Vec<u32> = self
                .units
                .iter()
                .filter(|u| {
                    u.alive
                        && u.kingdom == Some(kid)
                        && u.kind == UnitKind::Soldier
                        && matches!(u.state, UnitState::Patrol | UnitState::Idle)
                })
                .map(|u| u.id)
                .collect();
            let city_center = match self.village(city).map(|v| v.center) {
                Some(c) => c,
                None => continue,
            };
            for s in soldiers {
                // Already heading there?
                let heading = self
                    .unit(s)
                    .map(|u| u.destination == Some(city_center))
                    .unwrap_or(false);
                if heading {
                    continue;
                }
                let reached = self
                    .unit(s)
                    .map(|u| u.pos.distance(city_center) <= 2)
                    .unwrap_or(false);
                if !self.order_move(s, city_center) {
                    // No land route: try the sea.
                    if self.board_boat(s) {
                        self.order_move(s, city_center);
                    }
                }
                if let Some(u) = self.unit_mut(s) {
                    u.state = if reached {
                        UnitState::Attack
                    } else {
                        UnitState::March
                    };
                    u.destination = Some(city_center);
                }
            }
            // Siege progress: soldiers next to the city grind it down.
            let besiegers: Vec<u32> = self
                .units
                .iter()
                .filter(|u| {
                    u.alive
                        && u.kingdom == Some(kid)
                        && u.kind == UnitKind::Soldier
                        && self
                            .village(city)
                            .map(|v| u.pos.distance(v.center) <= 2)
                            .unwrap_or(false)
                })
                .map(|u| u.id)
                .collect();
            if !besiegers.is_empty() {
                let defenders = self
                    .units
                    .iter()
                    .filter(|u| {
                        u.alive
                            && u.village == Some(city)
                            && self.village(city).map(|v| u.pos.distance(v.center) <= 3).unwrap_or(false)
                    })
                    .count();
                let power = besiegers.len() as i32 * 3;
                let walls = self.village(city).map(|v| v.walls as i32).unwrap_or(0);
                if let Some(v) = self.village_mut(city) {
                    v.siege += (power - walls * 2).max(1);
                    v.besieged_by = Some(kid);
                    if v.siege % 10 == 0 {
                        v.loyalty -= 3;
                    }
                }
                let siege = self.village(city).map(|v| v.siege).unwrap_or(0);
                if siege >= 100 && defenders <= besiegers.len() {
                    self.capture_city(kid, city);
                }
            }
        }
    }

    /// Rebellions and loyalty crises, run once a year per village.
    pub fn rebellion_tick(&mut self) {
        let ids = self.village_ids();
        for vid in ids {
            let (kingdom, loyalty, pop) = match self.village(vid) {
                Some(v) => (v.kingdom, v.loyalty, v.pop),
                None => continue,
            };
            let Some(_k) = kingdom else { continue };
            if loyalty > -20 || pop < 4 {
                continue;
            }
            let chance = ((-loyalty - 20) as f32 / 200.0).max(0.01);
            if self.rng.chance(chance) {
                self.rebel(vid);
            }
        }
    }

    /// Relations view for the CLI: `(kingdom name, opinion, at war, allied)`.
    pub fn relations_view(&self, kid: u32) -> Vec<(String, i16, bool, bool)> {
        let mut out = Vec::new();
        for other in self.kingdom_ids() {
            if other == kid {
                continue;
            }
            let name = self
                .kingdom(other)
                .map(|k| k.name.clone())
                .unwrap_or_default();
            let opinion = self.kingdom(kid).map(|k| k.opinion(other)).unwrap_or(0);
            let at_war = self
                .kingdom(kid)
                .map(|k| k.at_war_with(other))
                .unwrap_or(false);
            let allied = self
                .kingdom(kid)
                .map(|k| k.is_allied_with(other))
                .unwrap_or(false);
            out.push((name, opinion, at_war, allied));
        }
        out
    }

    /// The kingdom's banner colour, for the renderer.
    pub fn kingdom_color(&self, kid: u32) -> (u8, u8, u8) {
        self.kingdom(kid).map(|k| k.color).unwrap_or((120, 120, 120))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::units::UnitKind;
    use crate::village::FOUNDING_POP;
    use crate::worldgen::{GenParams, WorldType};

    fn world() -> World {
        World::generate(GenParams::new(44, 34, 2024, WorldType::Continents))
    }

    fn village_with_kingdom(w: &mut World, race: Race, kingdom: Option<u32>) -> u32 {
        let spot = w.random_land_tile().unwrap();
        let mut founders = Vec::new();
        for _ in 0..FOUNDING_POP {
            if let Some(id) = w.spawn_unit(race, spot, UnitKind::Civilian) {
                founders.push(id);
            }
        }
        w.found_village(spot, race, &founders, kingdom, None).unwrap()
    }

    #[test]
    fn founding_a_kingdom_crowns_a_king() {
        let mut w = world();
        let vid = village_with_kingdom(&mut w, Race::Human, None);
        let kid = w.found_kingdom(vid, Race::Human).unwrap();
        let k = w.kingdom(kid).unwrap();
        assert!(k.king.is_some());
        assert!(!k.king_name.is_empty());
        assert!(k.cities.contains(&vid));
        assert!(w.village(vid).unwrap().capital);
        assert!(w.village(vid).unwrap().loyalty >= 60);
        assert_eq!(w.stats.kingdoms_founded, 1);
        let king = w.unit(k.king.unwrap()).unwrap();
        assert_eq!(king.kind, UnitKind::King);
    }

    #[test]
    fn wars_and_peace_are_symmetric_and_chronicled() {
        let mut w = world();
        let a = village_with_kingdom(&mut w, Race::Human, None);
        let b = village_with_kingdom(&mut w, Race::Orc, None);
        let ka = w.found_kingdom(a, Race::Human).unwrap();
        let kb = w.found_kingdom(b, Race::Orc).unwrap();
        w.declare_war(ka, kb);
        assert!(w.kingdom(ka).unwrap().at_war_with(kb));
        assert!(w.kingdom(kb).unwrap().at_war_with(ka));
        assert_eq!(w.stats.wars_declared, 1);
        // Declaring twice does not double-count.
        w.declare_war(ka, kb);
        assert_eq!(w.stats.wars_declared, 1);
        w.make_peace(ka, kb);
        assert!(!w.kingdom(ka).unwrap().at_war_with(kb));
        assert_eq!(w.stats.wars_ended, 1);
        assert!(w.chronicle.iter().any(|e| e.kind == EventKind::War));
        assert!(w.chronicle.iter().any(|e| e.kind == EventKind::Peace));
    }

    #[test]
    fn opinion_moves_when_kingdoms_fight_and_when_they_ally() {
        let mut w = world();
        let a = village_with_kingdom(&mut w, Race::Human, None);
        let b = village_with_kingdom(&mut w, Race::Orc, None);
        let ka = w.found_kingdom(a, Race::Human).unwrap();
        let kb = w.found_kingdom(b, Race::Orc).unwrap();
        w.declare_war(ka, kb);
        assert!(w.kingdom(ka).unwrap().opinion(kb) < 0);
        w.make_peace(ka, kb);
        let after_peace = w.kingdom(ka).unwrap().opinion(kb);
        w.form_alliance(ka, kb);
        assert!(w.kingdom(ka).unwrap().opinion(kb) > after_peace);
        assert!(w.kingdom(kb).unwrap().is_allied_with(ka));
        assert_eq!(w.stats.alliances, 1);
    }

    #[test]
    fn orc_and_human_relations_drift_apart_over_time() {
        let mut w = world();
        let a = village_with_kingdom(&mut w, Race::Human, None);
        let b = village_with_kingdom(&mut w, Race::Orc, None);
        let ka = w.found_kingdom(a, Race::Human).unwrap();
        let kb = w.found_kingdom(b, Race::Orc).unwrap();
        let before = w.kingdom(ka).unwrap().opinion(kb);
        for _ in 0..200 {
            w.diplomacy_pair(ka, kb);
        }
        let after = w.kingdom(ka).unwrap().opinion(kb);
        assert!(after < before, "orc/human relations should sour: {before} -> {after}");
    }

    #[test]
    fn capturing_a_city_transfers_it_and_can_destroy_a_kingdom() {
        let mut w = world();
        let a = village_with_kingdom(&mut w, Race::Human, None);
        let b = village_with_kingdom(&mut w, Race::Human, None);
        let ka = w.found_kingdom(a, Race::Human).unwrap();
        let kb = w.found_kingdom(b, Race::Human).unwrap();
        w.declare_war(ka, kb);
        w.capture_city(ka, b);
        assert_eq!(w.village(b).unwrap().kingdom, Some(ka));
        assert!(w.kingdom(kb).is_none(), "a kingdom with no cities must fall");
        assert_eq!(w.stats.cities_captured, 1);
        assert!(w.stats.kingdoms_fallen >= 1);
    }

    #[test]
    fn rebellion_creates_a_new_kingdom_at_war_with_its_parent() {
        let mut w = world();
        let a = village_with_kingdom(&mut w, Race::Human, None);
        let b = village_with_kingdom(&mut w, Race::Human, None);
        let ka = w.found_kingdom(a, Race::Human).unwrap();
        w.transfer_village(b, ka);
        {
            let v = w.village_mut(b).unwrap();
            v.loyalty = -80;
        }
        w.rebel(b);
        let new_k = w.village(b).unwrap().kingdom.unwrap();
        assert_ne!(new_k, ka);
        assert!(w.kingdom(ka).unwrap().at_war_with(new_k));
        assert_eq!(w.stats.rebellions, 1);
    }

    #[test]
    fn bandits_raze_instead_of_ruling() {
        let mut w = world();
        let victim = village_with_kingdom(&mut w, Race::Human, None);
        let camp = village_with_kingdom(&mut w, Race::Bandit, None);
        let bandits = w.found_kingdom(camp, Race::Bandit).unwrap();
        w.capture_city(bandits, victim);
        assert!(w.village(victim).is_none(), "bandits should burn the place");
    }

    #[test]
    fn war_tick_sends_soldiers_and_besieges() {
        let mut w = world();
        let a = village_with_kingdom(&mut w, Race::Human, None);
        let b = village_with_kingdom(&mut w, Race::Human, None);
        let ka = w.found_kingdom(a, Race::Human).unwrap();
        let kb = w.found_kingdom(b, Race::Human).unwrap();
        // Give both sides soldiers and put them at war.
        for _ in 0..6 {
            if let Some(id) = w.spawn_unit(Race::Human, w.village(a).unwrap().center, UnitKind::Soldier) {
                if let Some(u) = w.unit_mut(id) {
                    u.village = Some(a);
                    u.kingdom = Some(ka);
                    u.state = UnitState::Patrol;
                }
            }
        }
        w.declare_war(ka, kb);
        for _ in 0..40 {
            w.war_tick();
        }
        let marching = w
            .units
            .iter()
            .filter(|u| u.alive && u.kingdom == Some(ka) && u.state == UnitState::March)
            .count();
        assert!(marching > 0 || w.village(b).map(|v| v.siege).unwrap_or(0) > 0);
    }

    #[test]
    fn a_grown_village_crowns_itself() {
        let mut w = world();
        let vid = village_with_kingdom(&mut w, Race::Human, None);
        // Not yet: too few people and no town hall.
        w.kingdom_tick();
        assert!(w.village(vid).unwrap().kingdom.is_none(), "too eager");
        // Grow the village and give it a hall.
        for _ in 0..8 {
            if let Some(id) = w.spawn_unit(Race::Human, w.village(vid).unwrap().center, UnitKind::Civilian) {
                w.village_mut(vid).unwrap().pop += 1;
                if let Some(u) = w.unit_mut(id) {
                    u.village = Some(vid);
                }
            }
        }
        if let Some(v) = w.village_mut(vid) {
            v.buildings.push(crate::village::Building::new(
                BuildingKind::TownHall,
                v.center,
            ));
            v.town_hall_level = 1;
        }
        w.tick = 20; // coronations happen on the year-ish cadence
        w.kingdom_tick();
        let kingdom = w.village(vid).unwrap().kingdom.expect("a crown");
        assert!(w.kingdom(kingdom).unwrap().king.is_some());
        assert_eq!(w.stats.kingdoms_founded, 1);
    }

    #[test]
    fn succession_replaces_a_dead_king() {
        let mut w = world();
        let a = village_with_kingdom(&mut w, Race::Human, None);
        let ka = w.found_kingdom(a, Race::Human).unwrap();
        let king = w.kingdom(ka).unwrap().king.unwrap();
        w.kill_unit(king, None);
        w.kingdom_tick();
        let new_king = w.kingdom(ka).and_then(|k| k.king);
        assert!(new_king.is_some(), "a kingdom needs a king");
        assert_ne!(new_king, Some(king));
    }
}
