//! The simulation loop and creature AI.
//!
//! One tick is 1/20th of a year. The order inside [`World::step`] is fixed: ages,
//! environment, disasters, statuses, creatures, villages, kingdoms. Anything that
//! reads the world before another system has written to it would change the
//! simulation, so the order is part of the contract and the determinism tests
//! guard it.

use crate::hex::Hex;
use crate::path::PathMode;
use crate::races::Race;
use crate::terrain::Biome;
use crate::units::{traits, Resource, StatusKind, UnitKind, UnitState};
use crate::world::{EventKind, World, TICKS_PER_YEAR};

/// Wildlife breeding caps: no more than this many of one species, and no more
/// than this many living creatures in total.
pub const WILDLIFE_PER_SPECIES: usize = 24;
pub const WORLD_UNIT_CAP: usize = 400;
/// How far a civilian will chase something before giving up on it.
pub const CIVILIAN_CHASE_RANGE: i32 = 2;
/// Beyond this distance from the village centre, a villager walks home.
pub const HOMESICK_RANGE: i32 = 12;
/// Wild creatures will not attack a villager inside this radius of their town.
pub const HOME_SAFE_RADIUS: i32 = 4;

impl World {
    /// Advance the world by one tick.
    pub fn step(&mut self) {
        self.advance_clock();

        // --- 1. eras -------------------------------------------------------
        if self.tick % TICKS_PER_YEAR == 0 {
            self.age_tick();
            self.rebellion_tick();
        }

        // --- 2. the environment -------------------------------------------
        if self.tick % 5 == 0 {
            self.biome_tick();
        }

        // --- 3. disasters and weather -------------------------------------
        self.disaster_tick();

        // --- 4. creatures --------------------------------------------------
        self.unit_status_tick();
        self.unit_tick();

        // --- 5. civilization ----------------------------------------------
        self.village_tick();

        // --- 6. politics and war ------------------------------------------
        self.sync_unit_kingdoms();
        self.kingdom_tick();

        // --- 7. housekeeping ----------------------------------------------
        if self.tick % 256 == 0 {
            self.compact();
        }
    }

    /// A unit belongs to the kingdom its village belongs to. Villages change
    /// hands through capture and rebellion, so this is re-derived every tick.
    pub fn sync_unit_kingdoms(&mut self) {
        for i in 0..self.units.len() {
            if !self.units[i].alive {
                continue;
            }
            let Some(vid) = self.units[i].village else {
                continue;
            };
            let kingdom = self
                .villages
                .get(vid as usize)
                .filter(|v| v.alive)
                .and_then(|v| v.kingdom);
            self.units[i].kingdom = kingdom;
        }
    }

    /// Advance by `n` ticks.
    pub fn step_n(&mut self, n: u64) {
        for _ in 0..n {
            self.step();
        }
    }

    /// Every creature acts once.
    pub fn unit_tick(&mut self) {
        let ids = self.unit_ids();
        for id in ids {
            if self.unit(id).is_none() {
                continue; // died earlier in this same tick
            }
            self.unit_needs_tick(id);
            if self.unit(id).is_none() {
                continue;
            }
            self.unit_heal_tick(id);
            self.unit_behavior_tick(id);
            if self.unit(id).is_none() {
                continue;
            }
            self.unit_combat_tick(id);
            if self.unit(id).is_none() {
                continue;
            }
            self.unit_move_tick(id);
        }
    }

    /// Wounds close slowly when a creature is fed and not fighting for its life.
    fn unit_heal_tick(&mut self, id: u32) {
        if self.tick % 3 != 0 {
            return;
        }
        let Some(u) = self.unit(id) else { return };
        if u.hp >= u.max_hp || u.hunger > 50 {
            return;
        }
        // Being next to something that wants you dead stops the bleeding from
        // turning into knitting.
        let pos = u.pos;
        let race = u.race;
        let in_danger = self.units.iter().any(|o| {
            o.alive
                && o.id != id
                && o.pos.distance(pos) <= 1
                && (o.race.hates_race(race) || race.hates_race(o.race) || o.has_status(StatusKind::Mad))
        });
        if in_danger {
            return;
        }
        if let Some(u) = self.unit_mut(id) {
            u.hp = (u.hp + 1).min(u.max_hp);
        }
    }

    /// Ageing, hunger and upkeep.
    fn unit_needs_tick(&mut self, id: u32) {
        let (race, village, state, one_year) = match self.unit(id) {
            Some(u) => (
                u.race,
                u.village,
                u.state,
                self.tick % TICKS_PER_YEAR == 0,
            ),
            None => return,
        };
        if one_year {
            if let Some(u) = self.unit_mut(id) {
                u.age = u.age.saturating_add(1);
            }
        }
        // Animals and monsters fend for themselves; villagers eat from the store.
        if race.is_civilized() {
            if let Some(vid) = village {
                let food = self.village(vid).map(|v| v.food).unwrap_or(0);
                // People eat roughly every four ticks; `food` is the village
                // larder, so one meal costs one food.
                let eats_now = (self.tick.wrapping_add(id as u64)) % 4 == 0;
                // Roll the dice before borrowing the unit mutably.
                let hurt = self.rng.chance(0.05);
                let nap = self.rng.chance(0.02);
                let wake = self.rng.chance(0.1);
                if let Some(u) = self.unit_mut(id) {
                    if eats_now {
                        if food > 0 {
                            u.hunger = 0;
                        } else {
                            u.hunger = u.hunger.saturating_add(1);
                        }
                    }
                    if u.hunger > 60 && hurt {
                        u.hp -= 1;
                    }
                    if state == UnitState::Idle {
                        // Nothing to do: nap by the fire.
                        if nap {
                            u.state = UnitState::Sleep;
                        }
                    } else if state == UnitState::Sleep && wake {
                        u.state = UnitState::Idle;
                    }
                }
                if eats_now {
                    if let Some(v) = self.village_mut(vid) {
                        v.food = (v.food - 1).max(0);
                    }
                }
            }
        } else if race.is_animal() {
            let tile_food = self
                .tile(self.unit(id).map(|u| u.pos).unwrap_or_default())
                .map(|t| t.fertile > 30 || t.trees > 0)
                .unwrap_or(false);
            if let Some(u) = self.unit_mut(id) {
                if tile_food {
                    u.hunger = 0;
                } else {
                    u.hunger = u.hunger.saturating_add(2);
                }
                if u.hunger > 90 {
                    u.hp -= 2;
                }
            }
        }
        // Creatures standing in lava or fire suffer here (fire also spreads).
        let pos = match self.unit(id) {
            Some(u) => u.pos,
            None => return,
        };
        if let Some(t) = self.tile(pos) {
            if t.fire > 0 {
                let tick = self.tick;
                let instant = tick % 3 == 0;
                if instant {
                    self.damage_unit(id, 4, None);
                }
            }
        }
    }

    /// What the creature wants to do this tick.
    fn unit_behavior_tick(&mut self, id: u32) {
        let (race, kind, mad) = match self.unit(id) {
            Some(u) => (
                u.race,
                u.kind,
                u.has_trait(traits::MAD) || u.has_status(StatusKind::Mad),
            ),
            None => return,
        };
        if mad {
            self.rampage_tick(id);
            return;
        }
        if race.is_animal() {
            self.animal_tick(id);
        } else if race.is_monster() || kind == UnitKind::Monster {
            self.monster_tick(id);
        } else {
            self.civilian_tick(id);
        }
    }

    /// Mad creatures attack the nearest thing that moves.
    fn rampage_tick(&mut self, id: u32) {
        if let Some(u) = self.unit_mut(id) {
            u.state = UnitState::Rampage;
        }
        let target = self.nearest_enemy_ignoring_allegiance(id, 14);
        self.chase_or_attack(id, target, UnitState::Rampage);
    }

    /// Nearest unit of any kind that is not this unit's own race.
    fn nearest_enemy_ignoring_allegiance(&self, id: u32, radius: i32) -> Option<u32> {
        let me = self.unit(id)?;
        let mut best: Option<(i32, u32)> = None;
        for other in self.units.iter().filter(|u| u.alive && u.id != id) {
            if other.race == me.race && other.village.is_some() == me.village.is_some() {
                continue;
            }
            let d = me.pos.distance(other.pos);
            if d > radius {
                continue;
            }
            if best.is_none() || d < best.unwrap().0 {
                best = Some((d, other.id));
            }
        }
        best.map(|(_, id)| id)
    }

    /// Walk toward a target, or fight it when adjacent.
    fn chase_or_attack(&mut self, id: u32, target: Option<u32>, state: UnitState) {
        // Wild things do not follow people into their own villages: a beast that
        // chased a woodcutter home gives up at the border. This is what stops a
        // single bear from eating a hamlet one villager at a time.
        if let Some(t) = target {
            if !self.attack_allowed(id, t) {
                if let Some(u) = self.unit_mut(id) {
                    if u.state == UnitState::Hunt || u.state == UnitState::Attack {
                        u.state = UnitState::Idle;
                        u.enemy = None;
                    }
                }
                return;
            }
        }
        let Some(target) = target else {
            // Nothing to chase: wander somewhere nearby.
            let (pos, wander) = match self.unit(id) {
                Some(u) => (u.pos, u.state == UnitState::Idle),
                None => return,
            };
            if wander {
                let dest = pos.neighbor(self.rng.below(6) as usize);
                if self.tile(dest).map(|t| t.is_walkable()).unwrap_or(false) {
                    if let Some(u) = self.unit_mut(id) {
                        u.state = UnitState::Wander;
                        u.destination = Some(dest);
                    }
                    self.order_move(id, dest);
                }
            }
            return;
        };
        if let Some(u) = self.unit_mut(id) {
            u.enemy = Some(target);
            u.state = state;
        }
        let (us, them, dist) = match (self.unit(id), self.unit(target)) {
            (Some(a), Some(b)) => (a.pos, b.pos, a.pos.distance(b.pos)),
            _ => return,
        };
        if dist <= 1 {
            return; // combat happens in unit_combat_tick
        }
        if self.unit(id).and_then(|u| u.destination) != Some(them)
            && !self.order_move(id, them) {
                // Blocked (water, mountains): try to board a boat, else give up on
                // this target for now.
                if self.board_boat(id) {
                    self.order_move(id, them);
                } else if let Some(u) = self.unit_mut(id) {
                    u.enemy = None;
                }
            }
        let _ = us;
    }

    /// Herbivores graze and flee, predators hunt, everything breeds.
    fn animal_tick(&mut self, id: u32) {
        let (race, pos, hunger) = match self.unit(id) {
            Some(u) => (u.race, u.pos, u.hunger),
            None => return,
        };
        // Predators first: hunt whatever they hate.
        if race.is_predator() {
            let prey = self.nearest_enemy(id, 9);
            if prey.is_some() {
                self.chase_or_attack(id, prey, UnitState::Hunt);
                return;
            }
        }
        // Flee from a predator standing right next to us.
        let threat = self.nearest_predator(id, 3);
        if let Some(t) = threat {
            let threat_pos = self.unit(t).map(|u| u.pos).unwrap_or(pos);
            self.order_flee(id, threat_pos);
            return;
        }
        // Graze: stay on food, otherwise walk to it.
        if hunger > 30 {
            let (c, r) = pos.to_offset();
            let mut best: Option<Hex> = None;
            for dx in -4..=4 {
                for dy in -4..=4 {
                    let h = Hex::from_offset(c + dx, r + dy);
                    match self.tile(h) {
                        Some(t) if t.is_walkable() && (t.fertile > 40 || t.trees > 0) => {
                            let better = match best {
                                None => true,
                                Some(b) => h.distance(pos) < b.distance(pos),
                            };
                            if better {
                                best = Some(h);
                            }
                        }
                        _ => {}
                    }
                }
            }
            if let Some(dest) = best {
                self.chase_or_attack(id, None, UnitState::Graze);
                if let Some(u) = self.unit_mut(id) {
                    u.state = UnitState::Graze;
                    u.destination = Some(dest);
                }
                self.order_move(id, dest);
                return;
            }
        }
        // Breed when two of the same race are close and both are fed. Wildlife is
        // capped so a long game cannot turn into an exponential animal farm.
        if self.tick % 4 == 0 && !race.is_civilized() && !race.is_monster() {
            let mate = self.units.iter().any(|o| {
                o.alive
                    && o.id != id
                    && o.race == race
                    && o.hunger < 40
                    && o.pos.distance(pos) <= 2
            });
            if mate && hunger < 40 {
                let herd = self.units.iter().filter(|u| u.alive && u.race == race).count();
                let all = self.units.iter().filter(|u| u.alive).count();
                if herd < WILDLIFE_PER_SPECIES && all < WORLD_UNIT_CAP {
                    let chance =
                        race.def().breed_chance * self.age.age.fertility_rate() as f32 / 60.0;
                    if self.rng.chance(chance)
                        && self.spawn_unit(race, pos, UnitKind::Animal).is_some()
                    {
                        self.stats.births += 1;
                    }
                }
            }
        }
        // Idle wander.
        if self.unit(id).map(|u| u.state) == Some(UnitState::Idle) {
            let dest = pos.neighbor(self.rng.below(6) as usize);
            if self.tile(dest).map(|t| t.is_walkable()).unwrap_or(false) {
                if let Some(u) = self.unit_mut(id) {
                    u.state = UnitState::Wander;
                }
                self.order_move(id, dest);
            }
        }
    }

    fn nearest_predator(&self, id: u32, radius: i32) -> Option<u32> {
        let me = self.unit(id)?;
        let mut best: Option<(i32, u32)> = None;
        for other in self.units.iter().filter(|u| u.alive && u.id != id) {
            if !other.race.is_predator() || !other.race.hates_race(me.race) {
                continue;
            }
            let d = me.pos.distance(other.pos);
            if d > radius {
                continue;
            }
            if best.is_none() || d < best.unwrap().0 {
                best = Some((d, other.id));
            }
        }
        best.map(|(_, id)| id)
    }

    /// Monsters and bosses: hunt, rampage, burn.
    fn monster_tick(&mut self, id: u32) {
        let (race, pos) = match self.unit(id) {
            Some(u) => (u.race, u.pos),
            None => return,
        };
        // Bosses demolish whatever they stand on.
        if race.def().size >= 4 {
            let owner = self.tile(pos).and_then(|t| t.owner);
            if let Some(v) = owner {
                if self.rng.chance(0.08) {
                    self.village_building_damage(v, 1);
                }
            }
            if race == Race::Dragon {
                // Dragons set the ground alight as they pass.
                if self.rng.chance(0.3) {
                    self.ignite(pos, 20);
                }
                let fire_breath = pos.neighbor(self.rng.below(6) as usize);
                if self.rng.chance(0.2) {
                    self.ignite(fire_breath, 24);
                    for victim in self.units_in_radius(fire_breath, 1) {
                        self.damage_unit(victim, 25, Some(id));
                    }
                }
            }
            if race == Race::Ufo {
                // The saucer burns what it hovers over.
                if self.rng.chance(0.15) {
                    self.ignite(pos, 16);
                }
            }
        }
        let target = self
            .nearest_enemy(id, 14)
            .or_else(|| self.nearest_enemy_ignoring_allegiance(id, 8));
        self.chase_or_attack(id, target, UnitState::Rampage);
    }

    /// Villagers: do whatever their task says.
    fn civilian_tick(&mut self, id: u32) {
        let (state, pos, dest, carry, village, enemy, kind) = match self.unit(id) {
            Some(u) => (
                u.state,
                u.pos,
                u.destination,
                u.carry,
                u.village,
                u.enemy,
                u.kind,
            ),
            None => return,
        };
        match state {
            UnitState::Gather => {
                let Some(dest) = dest else {
                    if let Some(u) = self.unit_mut(id) {
                        u.state = UnitState::Idle;
                    }
                    return;
                };
                if pos == dest || pos.distance(dest) <= 0 {
                    // Harvest from the tile (or the nearest one if the tile is
                    // already stripped).
                    let kind = carry.map(|(r, _)| r).unwrap_or(Resource::Wood);
                    let mut harvested = None;
                    for h in [dest].into_iter().chain(pos.neighbors()) {
                        if let Some(i) = self.idx(h) {
                            let t = self.tiles[i];
                            let (take, res) = match kind {
                                Resource::Wood if t.trees > 0 => (true, Resource::Wood),
                                Resource::Stone if t.stone > 0 || t.is_mountain() => {
                                    (true, Resource::Stone)
                                }
                                Resource::Ore if t.ore > 0 => (true, Resource::Ore),
                                Resource::Food if t.fertile > 30 => (true, Resource::Food),
                                Resource::Wood => (false, Resource::Wood),
                                Resource::Stone => (false, Resource::Stone),
                                Resource::Ore => (false, Resource::Ore),
                                Resource::Food => (false, Resource::Food),
                                _ => (false, kind),
                            };
                            if take {
                                harvested = Some((i, h, res));
                                break;
                            }
                        }
                    }
                    match harvested {
                        Some((i, h, res)) => {
                            match res {
                                Resource::Wood => {
                                    self.tiles[i].trees -= 1;
                                    self.stats.trees_chopped += 1;
                                }
                                Resource::Stone => {
                                    // Mountain rock never runs out; loose stone does.
                                    self.tiles[i].stone = self.tiles[i].stone.saturating_sub(1);
                                }
                                Resource::Ore => self.tiles[i].ore = self.tiles[i].ore.saturating_sub(1),
                                Resource::Food => {}
                                Resource::Gold => {}
                            }
                            if res == Resource::Food {
                                if let Some(vid) = village {
                                    if let Some(v) = self.village_mut(vid) {
                                        v.food += 2;
                                    }
                                }
                                if let Some(u) = self.unit_mut(id) {
                                    u.state = UnitState::Idle;
                                    u.carry = None;
                                }
                            } else {
                                if let Some(u) = self.unit_mut(id) {
                                    u.carry = Some((res, 2));
                                    u.state = UnitState::Deliver;
                                    u.destination = None;
                                }
                                let home = village
                                    .and_then(|v| self.village(v).map(|v| v.center))
                                    .unwrap_or(h);
                                if let Some(u) = self.unit_mut(id) {
                                    u.destination = Some(home);
                                }
                                self.order_move(id, home);
                            }
                        }
                        None => {
                            // Nothing here any more; look again.
                            if let Some(u) = self.unit_mut(id) {
                                u.state = UnitState::Idle;
                                u.carry = None;
                                u.destination = None;
                            }
                        }
                    }
                    return;
                }
                if self.unit(id).map(|u| u.path_i) == Some(self.unit(id).map(|u| u.path.len()).unwrap_or(0)) {
                    // The path ran out (blocked, or the destination moved).
                    if let Some(u) = self.unit_mut(id) {
                        u.path.clear();
                        u.path_i = 0;
                    }
                    self.order_move(id, dest);
                }
            }
            UnitState::Deliver => {
                let home = village
                    .and_then(|v| self.village(v).map(|v| v.center))
                    .unwrap_or(pos);
                if pos.distance(home) <= 1 {
                    if let (Some(vid), Some((res, amount))) = (village, carry) {
                        let gold = if res == Resource::Ore { amount as i32 / 4 } else { 0 };
                        if let Some(v) = self.village_mut(vid) {
                            match res {
                                Resource::Wood => v.wood += amount as i32,
                                Resource::Stone => v.stone += amount as i32,
                                Resource::Ore => v.ore += amount as i32,
                                Resource::Gold => v.gold += amount as i32,
                                Resource::Food => v.food += amount as i32,
                            }
                            v.gold += gold;
                        }
                    }
                    if let Some(u) = self.unit_mut(id) {
                        u.carry = None;
                        u.state = UnitState::Idle;
                        u.destination = None;
                    }
                } else if self.unit(id).and_then(|u| u.destination) != Some(home) {
                    if let Some(u) = self.unit_mut(id) {
                        u.destination = Some(home);
                    }
                    self.order_move(id, home);
                }
            }
            UnitState::Build => {
                let site = self
                    .village(village.unwrap_or(u32::MAX))
                    .and_then(|v| v.build_site)
                    .map(|b| b.pos);
                match site {
                    Some(site) if pos.distance(site) <= 1 => {
                        if let Some(v) = village {
                            if let Some(v) = self.village_mut(v) {
                                if let Some(b) = v.build_site.as_mut() {
                                    b.progress = b.progress.saturating_add(3);
                                }
                            }
                        }
                        if let Some(u) = self.unit_mut(id) {
                            u.state = UnitState::Build;
                        }
                    }
                    Some(site) => {
                        if self.unit(id).and_then(|u| u.destination) != Some(site) {
                            if let Some(u) = self.unit_mut(id) {
                                u.destination = Some(site);
                            }
                            self.order_move(id, site);
                        }
                    }
                    None => {
                        if let Some(u) = self.unit_mut(id) {
                            u.state = UnitState::Idle;
                        }
                    }
                }
            }
            UnitState::March | UnitState::Attack | UnitState::Patrol => {
                // Soldiers are steered by the war system; make sure they are not
                // stranded when their destination is gone.
                if let Some(d) = dest {
                    let valid = self.in_bounds(d);
                    if !valid {
                        if let Some(u) = self.unit_mut(id) {
                            u.destination = None;
                            u.state = UnitState::Idle;
                        }
                    } else if self.unit(id).map(|u| u.path.is_empty()).unwrap_or(false) {
                        self.order_move(id, d);
                    }
                } else if state == UnitState::Attack {
                    // Nothing left to fight (the target died, or fled out of any
                    // reasonable range): go back to normal life. Without this a
                    // villager who once swung at a passing wolf walks at it forever.
                    let target = enemy.and_then(|e| self.unit(e).map(|e| e.pos));
                    let quitting = match target {
                        None => true,
                        Some(p) => p.distance(pos) > CIVILIAN_CHASE_RANGE && kind != UnitKind::Soldier,
                    };
                    // A villager runs for the village instead of trading blows
                    // with something that outclasses them. Soldiers hold the line.
                    let fleeing = match self.unit(id) {
                        Some(u) => {
                            if !u.race.is_civilized()
                                || matches!(kind, UnitKind::Soldier | UnitKind::Monster)
                            {
                                false
                            } else {
                                let wounded = u.hp * 3 < u.max_hp;
                                let outclassed = enemy
                                    .and_then(|e| self.unit(e))
                                    .map(|e| e.race.is_monster() || e.race.is_predator())
                                    .unwrap_or(false);
                                wounded || outclassed
                            }
                        }
                        None => false,
                    };
                    if fleeing {
                        let home = village
                            .and_then(|v| self.village(v).map(|v| v.center))
                            .or_else(|| self.unit(id).map(|u| u.home))
                            .unwrap_or(pos);
                        if let Some(u) = self.unit_mut(id) {
                            u.state = UnitState::Flee;
                            u.enemy = None;
                            u.destination = Some(home);
                        }
                        self.order_move(id, home);
                    } else if quitting {
                        if let Some(u) = self.unit_mut(id) {
                            u.state = UnitState::Idle;
                            u.destination = None;
                            u.enemy = None;
                            u.path.clear();
                            u.path_i = 0;
                        }
                    } else if let Some(p) = target {
                        if pos.distance(p) > 1
                            && self.unit(id).map(|u| u.path.is_empty()).unwrap_or(false)
                        {
                            self.order_move(id, p);
                        }
                    }
                } else if state == UnitState::Patrol {
                    let home = village
                        .and_then(|v| self.village(v).map(|v| v.center))
                        .unwrap_or(pos);
                    if pos.distance(home) > 4 {
                        if let Some(u) = self.unit_mut(id) {
                            u.destination = Some(home);
                        }
                        self.order_move(id, home);
                    }
                }
            }
            UnitState::Flee => {
                let home = village
                    .and_then(|v| self.village(v).map(|v| v.center))
                    .or_else(|| self.unit(id).map(|u| u.home))
                    .unwrap_or(pos);
                if pos.distance(home) <= 1 {
                    if let Some(u) = self.unit_mut(id) {
                        u.state = UnitState::Idle;
                        u.destination = None;
                    }
                } else if dest != Some(home) {
                    if let Some(u) = self.unit_mut(id) {
                        u.destination = Some(home);
                    }
                    self.order_move(id, home);
                }
            }
            UnitState::Idle | UnitState::Wander | UnitState::Sleep => {
                // A wanderer whose path ran out somewhere odd simply heads home
                // (this also reels in anyone who chased something too far).
                if let (Some(d), true) = (dest, self.unit(id).map(|u| u.path.is_empty()).unwrap_or(false))
                {
                    if pos.distance(d) > 1 {
                        let reachable = village
                            .and_then(|v| self.village(v).map(|v| v.center))
                            .or_else(|| self.unit(id).map(|u| u.home))
                            .unwrap_or(pos);
                        if pos.distance(reachable) > HOMESICK_RANGE {
                            if let Some(u) = self.unit_mut(id) {
                                u.destination = Some(reachable);
                                u.state = UnitState::Wander;
                            }
                            self.order_move(id, reachable);
                        }
                    }
                }
            }
            UnitState::Graze | UnitState::Hunt | UnitState::Rampage => {}
            UnitState::Follow => {
                // Escort the leader/king.
                let leader = village
                    .and_then(|v| self.village(v).and_then(|v| v.leader))
                    .and_then(|l| self.unit(l).map(|u| u.pos));
                if let Some(lp) = leader {
                    if pos.distance(lp) > 2 {
                        if let Some(u) = self.unit_mut(id) {
                            u.destination = Some(lp);
                        }
                        self.order_move(id, lp);
                    }
                }
            }
            UnitState::Sail => {
                // Try to get back onto land.
                self.disembark(id);
            }
        }
    }

    /// Melee: attack an adjacent enemy when the cooldown allows.
    fn unit_combat_tick(&mut self, id: u32) {
        let (cooldown, pos, kind, race, state) = match self.unit(id) {
            Some(u) => (u.cooldown, u.pos, u.kind, u.race, u.state),
            None => return,
        };
        // Someone running for their life does not stop to swing.
        if state == UnitState::Flee && !matches!(kind, UnitKind::Soldier | UnitKind::Monster) {
            return;
        }
        // Find an adjacent enemy: the current target if it is in reach, otherwise
        // whoever is closest.
        let mut target: Option<u32> = None;
        if let Some(e) = self.unit(id).and_then(|u| u.enemy) {
            if let Some(other) = self.unit(e) {
                if other.pos.distance(pos) <= 1 {
                    target = Some(e);
                }
            }
        }
        if target.is_none() {
            let mut best: Option<(i32, u32)> = None;
            for other in self.units.iter().filter(|u| u.alive && u.id != id) {
                let d = other.pos.distance(pos);
                if d > 1 {
                    continue;
                }
                let wants = self
                    .unit(id)
                    .map(|me| me.wants_to_fight(other))
                    .unwrap_or(false);
                if wants && (best.is_none() || d < best.unwrap().0) {
                    best = Some((d, other.id));
                }
            }
            target = best.map(|(_, t)| t);
        }
        let Some(target) = target else {
            if cooldown > 0 {
                if let Some(u) = self.unit_mut(id) {
                    u.cooldown -= 1;
                }
            }
            return;
        };
        if cooldown > 0 {
            if let Some(u) = self.unit_mut(id) {
                u.cooldown -= 1;
            }
            return;
        }
        self.resolve_attack(id, target);
        let wait = race.def().attack_cooldown;
        if let Some(u) = self.unit_mut(id) {
            u.cooldown = wait;
            u.state = UnitState::Attack;
            u.enemy = Some(target);
            if kind == UnitKind::Soldier {
                u.siege_progress = u.siege_progress.saturating_add(1);
            }
        }
    }

    /// Whether `attacker` may strike `target`.
    ///
    /// The only rule here: a wild animal or monster will not attack a civilized
    /// creature that is standing inside its own village's territory. People are
    /// safe at home, which is what makes fleeing work.
    pub fn attack_allowed(&self, attacker: u32, target: u32) -> bool {
        let (wild, a_kind) = match self.unit(attacker) {
            Some(u) => (!u.race.is_civilized(), u.kind),
            None => return true,
        };
        if !wild || a_kind == UnitKind::Soldier {
            return true;
        }
        let Some(victim) = self.unit(target) else {
            return true;
        };
        if !victim.race.is_civilized() {
            return true;
        }
        let Some(vid) = victim.village else {
            return true;
        };
        // "Home" is the village centre and its claimed ring: a beast that chased
        // someone back to the edge of town turns around there.
        match self.village(vid).map(|v| v.center) {
            Some(center) => victim.pos.distance(center) > HOME_SAFE_RADIUS,
            None => true,
        }
    }

    /// Movement: spend accumulated movement points walking the path.
    fn unit_move_tick(&mut self, id: u32) {
        let (speed, sailing, pos) = match self.unit(id) {
            Some(u) => (u.move_speed(), u.sailing, u.pos),
            None => return,
        };
        if speed > 0 {
            if let Some(u) = self.unit_mut(id) {
                u.move_points = (u.move_points + speed).min(600);
            }
        }
        // Step at most four tiles per tick, and disembark when we hit dry land.
        for _ in 0..4 {
            let stepped = self.unit_step_along_path(id);
            if !stepped {
                break;
            }
        }
        if sailing {
            let dry = self
                .tile(self.unit(id).map(|u| u.pos).unwrap_or(pos))
                .map(|t| !t.is_water())
                .unwrap_or(false);
            if dry {
                self.disembark(id);
            }
        }
    }

    /// Give every monster-free creature a small chance to appear from the wild
    /// (keeps an empty world alive before the player does anything).
    pub fn wildlife_spawn_tick(&mut self) {
        if self.tick % 200 != 0 {
            return;
        }
        let animals = self.units.iter().filter(|u| u.alive && u.race.is_animal()).count();
        if animals > 60 {
            return;
        }
        let race = self.rng.pick_copy(&Race::SPAWNABLE_ANIMALS);
        if let Some(h) = self.random_land_tile() {
            if self.rng.chance(0.4) {
                self.spawn_unit(race, h, UnitKind::Animal);
            }
        }
    }

    /// Convenience: kill everything of a race, or everything at all (used by the
    /// CLI's panic button and by tests).
    pub fn purge(&mut self, race: Option<Race>) -> u32 {
        let ids: Vec<u32> = self
            .units
            .iter()
            .filter(|u| u.alive && race.map(|r| u.race == r).unwrap_or(true))
            .map(|u| u.id)
            .collect();
        let n = ids.len() as u32;
        for id in ids {
            self.kill_unit(id, None);
        }
        self.chronicle(
            EventKind::Discovery,
            match race {
                Some(r) => format!("The gods erase every {} from the world", r.name()),
                None => "The gods erase everything that lives".to_string(),
            },
        );
        n
    }

    /// Burn the whole map (a test and demo helper, and the `wildfire` script
    /// command).
    pub fn ignite_world(&mut self, strength: u8) -> u32 {
        let mut lit = 0;
        for i in 0..self.tiles.len() {
            if self.tiles[i].biome.can_burn() {
                self.tiles[i].fire = strength.max(12);
                lit += 1;
            }
        }
        lit
    }

    /// Freeze the whole map over (Age of Ice as a power).
    pub fn freeze_world(&mut self) -> u32 {
        let mut frozen = 0;
        for i in 0..self.tiles.len() {
            match self.tiles[i].biome {
                Biome::Shallow | Biome::Ocean => {
                    self.tiles[i].biome = Biome::Ice;
                    frozen += 1;
                }
                Biome::Grass | Biome::Forest | Biome::Swamp => {
                    self.tiles[i].biome = Biome::Permafrost;
                    frozen += 1;
                }
                _ => {}
            }
        }
        frozen
    }

    /// Every unit on the map, described: used by the CLI's `units` command.
    pub fn unit_report(&self) -> Vec<String> {
        self.units
            .iter()
            .filter(|u| u.alive)
            .map(|u| u.describe())
            .collect()
    }

    /// Sailing units use the water path mode.
    pub fn set_sailing(&mut self, id: u32, sail: bool) {
        if let Some(u) = self.unit_mut(id) {
            u.sailing = sail;
        }
    }

    /// Path mode for a unit, exposed for the CLI's `path` command.
    pub fn mode_for(&self, id: u32) -> PathMode {
        self.path_mode_for(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::village::FOUNDING_POP;
    use crate::worldgen::{GenParams, WorldType};

    fn world() -> World {
        World::generate(GenParams::new(36, 28, 5, WorldType::Continents))
    }

    #[test]
    fn stepping_advances_the_clock() {
        let mut w = world();
        w.step_n(25);
        assert_eq!(w.tick, 25);
        assert_eq!(w.year, 1);
    }

    #[test]
    fn simulation_is_deterministic() {
        let mut a = world();
        let mut b = world();
        // Populate both identically, then run.
        for w in [&mut a, &mut b] {
            for _ in 0..5 {
                let h = w.random_land_tile().unwrap();
                w.spawn_unit(Race::Human, h, UnitKind::Civilian);
                w.spawn_unit(Race::Wolf, h, UnitKind::Animal);
            }
        }
        a.step_n(300);
        b.step_n(300);
        assert_eq!(a.state_hash(), b.state_hash(), "same seed must give the same world");
    }

    #[test]
    fn animals_wander_and_stay_on_the_map() {
        let mut w = world();
        let h = w.random_land_tile().unwrap();
        let id = w.spawn_unit(Race::Sheep, h, UnitKind::Animal).unwrap();
        w.step_n(200);
        if let Some(u) = w.unit(id) {
            assert!(w.in_bounds(u.pos));
        }
    }

    #[test]
    fn wolves_hunt_sheep() {
        let mut w = world();
        let h = w.random_land_tile().unwrap();
        let sheep = w.spawn_unit(Race::Sheep, h, UnitKind::Animal).unwrap();
        let wolf = w.spawn_unit(Race::Wolf, h.neighbor(0), UnitKind::Animal).unwrap();
        let mut attacked = false;
        for _ in 0..200 {
            w.step();
            if w.unit(sheep).is_none() {
                attacked = true;
                break;
            }
            if let Some(u) = w.unit(wolf) {
                if u.enemy.is_some() {
                    attacked = true;
                    break;
                }
            }
        }
        assert!(attacked, "a wolf next to a sheep should hunt it");
    }

    #[test]
    fn monsters_attack_villagers() {
        let mut w = world();
        let spot = w.random_land_tile().unwrap();
        let mut founders = Vec::new();
        for _ in 0..FOUNDING_POP {
            if let Some(id) = w.spawn_unit(Race::Human, spot, UnitKind::Civilian) {
                founders.push(id);
            }
        }
        let vid = w.found_village(spot, Race::Human, &founders, None, None).unwrap();
        let zombie = w.spawn_unit(Race::Zombie, spot.neighbor(1), UnitKind::Monster).unwrap();
        for _ in 0..300 {
            w.step();
            if w.unit(zombie).is_none() {
                break;
            }
        }
        assert!(
            w.village(vid).map(|v| v.pop).unwrap_or(0) < FOUNDING_POP
                || w.unit(zombie).is_none(),
            "the zombie should have hurt somebody or died trying"
        );
    }

    #[test]
    fn villagers_gather_and_the_store_grows() {
        let mut w = world();
        let spot = w.random_land_tile().unwrap();
        let mut founders = Vec::new();
        for _ in 0..FOUNDING_POP {
            if let Some(id) = w.spawn_unit(Race::Human, spot, UnitKind::Civilian) {
                founders.push(id);
            }
        }
        let vid = w.found_village(spot, Race::Human, &founders, None, None).unwrap();
        {
            let v = w.village_mut(vid).unwrap();
            v.food = 200;
            v.wood = 0;
            v.stone = 0;
        }
        for _ in 0..600 {
            w.step();
        }
        let v = w.village(vid).unwrap();
        assert!(
            v.wood > 0 || v.stone > 0 || v.ore > 0,
            "gatherers should have delivered something: {}",
            v.describe()
        );
    }

    #[test]
    fn compaction_does_not_break_references() {
        let mut w = world();
        let spot = w.random_land_tile().unwrap();
        let mut founders = Vec::new();
        for _ in 0..FOUNDING_POP {
            if let Some(id) = w.spawn_unit(Race::Human, spot, UnitKind::Civilian) {
                founders.push(id);
            }
        }
        let vid = w.found_village(spot, Race::Human, &founders, None, None).unwrap();
        let kid = w.found_kingdom(vid, Race::Human).unwrap();
        // Kill someone so compaction has work to do.
        let victim = founders[0];
        w.kill_unit(victim, None);
        w.step(); // triggers housekeeping
        w.compact();
        let v = w.village(vid).expect("village survives");
        assert_eq!(v.kingdom, Some(kid));
        assert!(w.kingdom(kid).unwrap().cities.contains(&v.id));
        for t in w.tiles.iter() {
            if let Some(owner) = t.owner {
                assert!(w.village(owner).is_some(), "tile owner {owner} vanished");
            }
        }
        for u in w.units.iter() {
            if let Some(v) = u.village {
                assert!(w.village(v).is_some(), "unit {0} village {v} vanished", u.id);
            }
        }
    }
}
