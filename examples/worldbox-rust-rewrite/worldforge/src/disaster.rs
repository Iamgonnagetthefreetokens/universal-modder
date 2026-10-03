//! Disasters: meteors, nukes, tornadoes, volcanoes, black holes and weather
//! clouds, plus the two slow systems they feed — fire and lava.
//!
//! Everything long lived is an entry in [`Effects`], which is plain data so it can
//! be saved and reloaded. In-flight things are updated once per tick in
//! [`World::disaster_tick`].

use crate::hex::Hex;
use crate::races::Race;
use crate::terrain::Biome;
use crate::units::{traits, StatusKind, UnitKind};
use crate::world::{EventKind, World};

/// Weather/effect clouds dropped by the player's powers.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CloudKind {
    /// Puts out fires, waters crops.
    Rain,
    /// Burns creatures and kills plants.
    AcidRain,
    /// Drives everything in it insane.
    Madness,
    /// Heals and extinguishes burning creatures.
    Blood,
    /// Spreads mushrooms and infection.
    Spores,
    /// Blesses creatures.
    Blessing,
    /// Curses creatures.
    Curse,
    /// Sets the area alight.
    Fire,
    /// Alien mould: infects and corrupts.
    AlienMold,
    /// Encourages breeding.
    Love,
}

impl CloudKind {
    pub const ALL: [CloudKind; 10] = [
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
    ];

    pub fn name(self) -> &'static str {
        match self {
            CloudKind::Rain => "rain cloud",
            CloudKind::AcidRain => "acid rain",
            CloudKind::Madness => "madness rain",
            CloudKind::Blood => "blood rain",
            CloudKind::Spores => "mushroom spores",
            CloudKind::Blessing => "blessing",
            CloudKind::Curse => "curse",
            CloudKind::Fire => "fire rain",
            CloudKind::AlienMold => "alien mould",
            CloudKind::Love => "smooth jazz",
        }
    }

    pub fn parse(s: &str) -> Option<CloudKind> {
        let l = s.to_ascii_lowercase();
        CloudKind::ALL.into_iter().find(|c| {
            c.name().starts_with(&l)
                || c.name().replace(' ', "_") == l
                || c.name().replace(' ', "") == l.replace(' ', "")
        })
    }

    /// Radius of the cloud's effect.
    pub fn radius(self) -> i32 {
        match self {
            CloudKind::Rain | CloudKind::Love => 3,
            CloudKind::Blood => 2,
            _ => 2,
        }
    }

    /// How long the cloud lasts, in ticks.
    pub fn duration(self) -> u16 {
        match self {
            CloudKind::Rain => 160,
            CloudKind::AcidRain => 140,
            CloudKind::Madness => 120,
            CloudKind::Fire => 60,
            CloudKind::Blood | CloudKind::Blessing | CloudKind::Curse => 100,
            _ => 120,
        }
    }
}

/// A cloud sitting over the map.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cloud {
    pub pos: Hex,
    pub kind: CloudKind,
    pub ticks_left: u16,
    pub radius: i32,
}

/// A rock on its way down. `pos` moves toward `target` each tick.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Meteor {
    pub pos: Hex,
    pub target: Hex,
    pub ticks_left: u16,
    pub radius: i32,
    pub power: i32,
    pub crater: bool,
}

/// A wandering funnel cloud.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tornado {
    pub pos: Hex,
    pub dir: usize,
    pub ticks_left: u16,
    pub power: i32,
}

/// An erupting vent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Volcano {
    pub pos: Hex,
    pub ticks_left: u16,
    pub power: i32,
}

/// A hole in the world. Pulls units in, then implodes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlackHole {
    pub pos: Hex,
    pub ticks_left: u16,
    pub radius: i32,
}

/// Everything in flight.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Effects {
    pub clouds: Vec<Cloud>,
    pub meteors: Vec<Meteor>,
    pub tornadoes: Vec<Tornado>,
    pub volcanoes: Vec<Volcano>,
    pub black_holes: Vec<BlackHole>,
}

impl Effects {
    pub fn is_empty(&self) -> bool {
        self.clouds.is_empty()
            && self.meteors.is_empty()
            && self.tornadoes.is_empty()
            && self.volcanoes.is_empty()
            && self.black_holes.is_empty()
    }

    pub fn count(&self) -> usize {
        self.clouds.len()
            + self.meteors.len()
            + self.tornadoes.len()
            + self.volcanoes.len()
            + self.black_holes.len()
    }

    pub fn clear(&mut self) {
        self.clouds.clear();
        self.meteors.clear();
        self.tornadoes.clear();
        self.volcanoes.clear();
        self.black_holes.clear();
    }
}

impl World {
    // --- spells that schedule something ------------------------------------

    /// Drop a rain/weather cloud.
    pub fn cast_cloud(&mut self, at: Hex, kind: CloudKind) -> bool {
        if !self.in_bounds(at) {
            return false;
        }
        self.effects.clouds.push(Cloud {
            pos: at,
            kind,
            ticks_left: kind.duration(),
            radius: kind.radius(),
        });
        self.stats.powers_cast += 1;
        true
    }

    /// Call down a meteor. It falls for a few ticks so the player sees it coming.
    pub fn cast_meteor(&mut self, target: Hex, radius: i32, power: i32, crater: bool) -> bool {
        if !self.in_bounds(target) {
            return false;
        }
        let (c, r) = target.to_offset();
        let from = Hex::from_offset((c - 6).max(0), (r - 6).max(0));
        self.effects.meteors.push(Meteor {
            pos: from,
            target,
            ticks_left: 12,
            radius: radius.clamp(1, 8),
            power: power.clamp(10, 900),
            crater,
        });
        self.stats.powers_cast += 1;
        true
    }

    pub fn cast_tornado(&mut self, at: Hex, power: i32) -> bool {
        if !self.in_bounds(at) {
            return false;
        }
        self.effects.tornadoes.push(Tornado {
            pos: at,
            dir: self.rng.below(6) as usize,
            ticks_left: 120,
            power: power.clamp(10, 200),
        });
        self.stats.powers_cast += 1;
        true
    }

    pub fn cast_volcano(&mut self, at: Hex) -> bool {
        if !self.in_bounds(at) {
            return false;
        }
        self.effects.volcanoes.push(Volcano {
            pos: at,
            ticks_left: 240,
            power: 3,
        });
        self.stats.powers_cast += 1;
        true
    }

    pub fn cast_black_hole(&mut self, at: Hex, radius: i32) -> bool {
        if !self.in_bounds(at) {
            return false;
        }
        self.effects.black_holes.push(BlackHole {
            pos: at,
            ticks_left: 60,
            radius: radius.clamp(1, 6),
        });
        self.stats.powers_cast += 1;
        true
    }

    // --- the atomic option -------------------------------------------------

    /// A nuclear detonation: crater, fire ring, scorched earth, radiation.
    pub fn nuke(&mut self, at: Hex, power: i32) -> u32 {
        let power = power.clamp(100, 1200);
        let killed = self.explode(at, 7, power, None);
        // Crater: the ground subsides and turns to scorched rock.
        for h in at.spiral(4) {
            if let Some(t) = self.tile_mut(h) {
                if t.is_water() {
                    continue;
                }
                t.elevation = (t.elevation - 90).max(20);
                t.biome = Biome::Wasteland;
                t.trees = 0;
                t.stone = 0;
                t.ore = 0;
                t.scorch = 6;
                t.fire = 0;
                t.refresh_fertility();
                t.owner = None;
            }
        }
        // Firestorm ring and radiation poisoning for anything left standing.
        for h in at.spiral(6) {
            let d = h.distance(at);
            if d > 4 {
                if let Some(t) = self.tile_mut(h) {
                    if t.biome.can_burn() {
                        t.fire = (30 - d * 3).max(6) as u8;
                    }
                }
            }
        }
        for id in self.units_in_radius(at, 9) {
            if let Some(u) = self.unit_mut(id) {
                u.apply_status(StatusKind::Poisoned, 300);
                u.add_trait(traits::CURSED);
            }
        }
        self.chronicle(
            EventKind::Disaster,
            format!("A nuclear blast tears the ground at {at}"),
        );
        self.stats.nukes_dropped += 1;
        killed
    }

    // --- area damage -------------------------------------------------------

    /// Generic explosion: damages units by distance, burns and scuffs the ground.
    /// Returns the number of units killed outright.
    pub fn explode(&mut self, center: Hex, radius: i32, power: i32, source: Option<u32>) -> u32 {
        if !self.in_bounds(center) {
            return 0;
        }
        let radius = radius.clamp(0, 12);
        let before = self.unit_count();
        let ids = self.units_in_radius(center, radius);
        for id in ids {
            let d = self
                .unit(id)
                .map(|u| u.pos.distance(center))
                .unwrap_or(radius + 1);
            // Energy falls off with the square of the distance, so the rim of a
            // blast hurts but rarely kills.
            let falloff = (radius + 1 - d).max(0);
            let span = (radius + 1) * (radius + 1);
            let dmg = power * falloff * falloff / span.max(1);
            self.damage_unit(id, dmg, source);
        }
        for h in center.spiral(radius) {
            let d = h.distance(center);
            // Roll first: `tile_mut` borrows the world.
            let fire = self.rng.chance(0.5);
            let thin_trees = self.rng.chance(0.5);
            let scorch = 1 + (radius - d).max(0) as u8 / 2;
            let trees_gone = d <= radius / 2;
            if let Some(t) = self.tile_mut(h) {
                if t.is_water() {
                    continue;
                }
                t.scorch = (t.scorch + scorch).min(9);
                if trees_gone {
                    t.trees = 0;
                } else if t.trees > 0 && thin_trees {
                    t.trees -= 1;
                }
                if fire && t.biome.can_burn() {
                    t.fire = t.fire.max(20);
                }
                if power >= 400 && d <= radius / 2 {
                    t.biome = if t.biome == Biome::Infernal {
                        Biome::Infernal
                    } else {
                        Biome::Wasteland
                    };
                    t.refresh_fertility();
                }
            }
        }
        let after = self.unit_count();
        let killed = before.saturating_sub(after) as u32;
        self.stats.explosions += 1;
        killed
    }

    /// Set a tile alight if it can burn.
    pub fn ignite(&mut self, h: Hex, strength: u8) -> bool {
        let Some(t) = self.tile_mut(h) else {
            return false;
        };
        if !t.biome.can_burn() && t.trees == 0 {
            return false;
        }
        if t.fire == 0 || strength > 0 {
            t.fire = t.fire.max(strength.max(12));
            return true;
        }
        false
    }

    // --- per-tick systems --------------------------------------------------

    /// Everything in flight, then fire and lava.
    pub fn disaster_tick(&mut self) {
        self.meteor_tick();
        self.tornado_tick();
        self.cloud_tick();
        self.volcano_tick();
        self.black_hole_tick();
        self.fire_tick();
        self.lava_tick();
    }

    fn meteor_tick(&mut self) {
        if self.effects.meteors.is_empty() {
            return;
        }
        let mut impacts: Vec<Meteor> = Vec::new();
        for m in self.effects.meteors.iter_mut() {
            // Fall toward the target: one step closer each tick.
            if m.pos != m.target {
                let line = m.pos.line_to(m.target);
                if line.len() > 1 {
                    m.pos = line[1];
                } else {
                    m.pos = m.target;
                }
            }
            if m.ticks_left > 0 {
                m.ticks_left -= 1;
            }
            if m.ticks_left == 0 {
                impacts.push(*m);
            }
        }
        self.effects.meteors.retain(|m| m.ticks_left > 0);
        for m in impacts {
            if m.crater {
                self.nuke(m.target, m.power);
                // A meteor also scatters lava around the crater.
                for h in m.target.ring(2) {
                    if let Some(t) = self.tile_mut(h) {
                        if !t.is_water() {
                            t.lava = t.lava.max(1);
                        }
                    }
                }
            } else {
                let killed = self.explode(m.target, m.radius, m.power, None);
                if killed > 0 {
                    self.chronicle(
                        EventKind::Disaster,
                        format!("A meteor strike at {} kills {}", m.target, killed),
                    );
                }
            }
        }
    }

    fn tornado_tick(&mut self) {
        if self.effects.tornadoes.is_empty() {
            return;
        }
        let mut finished = Vec::new();
        for i in 0..self.effects.tornadoes.len() {
            // Copy the funnel's state out, mutate locally, then write it back:
            // the world itself is borrowed below.
            let (mut pos, mut dir, mut ticks_left, power) = {
                let t = &self.effects.tornadoes[i];
                (t.pos, t.dir, t.ticks_left, t.power)
            };
            ticks_left = ticks_left.saturating_sub(1);
            if self.rng.chance(0.2) {
                dir = (dir + 1 + self.rng.below(2) as usize) % 6;
            }
            let next = pos.neighbor(dir);
            if self.in_bounds(next) {
                pos = next;
            } else {
                dir = (dir + 3) % 6; // bounce off the map edge
            }
            {
                let t = &mut self.effects.tornadoes[i];
                t.pos = pos;
                t.dir = dir;
                t.ticks_left = ticks_left;
            }
            // Damage and fling everything nearby.
            for id in self.units_in_radius(pos, 1) {
                self.damage_unit(id, power, None);
                if !self.rng.chance(0.5) {
                    continue;
                }
                let mut target = pos;
                for _ in 0..3 {
                    target = target.neighbor(self.rng.below(6) as usize);
                }
                if self.in_bounds(target) {
                    if let Some(u) = self.unit_mut(id) {
                        u.pos = target;
                        u.path.clear();
                        u.path_i = 0;
                    }
                }
            }
            // Wreck buildings and scatter trees.
            if let Some(t) = self.tile_mut(pos) {
                t.trees = 0;
                t.fire = t.fire.saturating_sub(1);
            }
            if let Some(v) = self.tile(pos).and_then(|t| t.owner) {
                self.village_building_damage(v, 1);
            }
            if self.effects.tornadoes[i].ticks_left == 0 {
                finished.push(i);
            }
        }
        for i in finished.into_iter().rev() {
            self.effects.tornadoes.remove(i);
        }
    }

    fn cloud_tick(&mut self) {
        if self.effects.clouds.is_empty() {
            return;
        }
        let mut expired = Vec::new();
        for i in 0..self.effects.clouds.len() {
            let (pos, kind, radius) = {
                let c = &self.effects.clouds[i];
                (c.pos, c.kind, c.radius)
            };
            let cells = pos.spiral(radius);
            for h in cells {
                match kind {
                    CloudKind::Rain => {
                        let soak = self.rng.chance(0.05);
                        if let Some(t) = self.tile_mut(h) {
                            t.fire = 0;
                            if soak {
                                t.fertile = (t.fertile + 5).min(100);
                            }
                        }
                    }
                    CloudKind::AcidRain => {
                        let kill_plant = self.rng.chance(0.1);
                        if let Some(t) = self.tile_mut(h) {
                            if t.trees > 0 && kill_plant {
                                t.trees -= 1;
                            }
                            t.scorch = (t.scorch + 1).min(9);
                            t.fertile = t.fertile.saturating_sub(4);
                        }
                        for id in self.units_in_radius(h, 0) {
                            self.damage_unit(id, 6, None);
                        }
                    }
                    CloudKind::Madness => {
                        for id in self.units_in_radius(h, 0) {
                            if let Some(u) = self.unit_mut(id) {
                                if !u.has_trait(traits::IMMORTAL) {
                                    u.apply_status(StatusKind::Mad, 90);
                                }
                            }
                        }
                    }
                    CloudKind::Blood => {
                        for id in self.units_in_radius(h, 0) {
                            self.heal_unit(id, 12);
                            if let Some(u) = self.unit_mut(id) {
                                u.clear_status(StatusKind::Burning);
                                u.clear_status(StatusKind::Bleeding);
                            }
                        }
                        if let Some(t) = self.tile_mut(h) {
                            t.fire = 0;
                        }
                    }
                    CloudKind::Spores => {
                        let sprout = self.rng.chance(0.06);
                        if let Some(t) = self.tile_mut(h) {
                            if t.biome.is_land() && sprout {
                                t.biome = Biome::Mushroom;
                                t.refresh_fertility();
                            }
                        }
                        for id in self.units_in_radius(h, 0) {
                            if self.rng.chance(0.05) {
                                if let Some(u) = self.unit_mut(id) {
                                    u.add_trait(traits::INFECTED);
                                }
                            }
                        }
                    }
                    CloudKind::Blessing => {
                        for id in self.units_in_radius(h, 0) {
                            if let Some(u) = self.unit_mut(id) {
                                u.add_trait(traits::BLESSED);
                                u.remove_trait(traits::CURSED);
                            }
                        }
                    }
                    CloudKind::Curse => {
                        for id in self.units_in_radius(h, 0) {
                            if let Some(u) = self.unit_mut(id) {
                                u.add_trait(traits::CURSED);
                                u.remove_trait(traits::BLESSED);
                            }
                        }
                    }
                    CloudKind::Fire => {
                        if self.rng.chance(0.25) {
                            self.ignite(h, 20);
                        }
                    }
                    CloudKind::AlienMold => {
                        let creep = self.rng.chance(0.05);
                        if let Some(t) = self.tile_mut(h) {
                            if t.biome.is_land() && creep {
                                t.biome = Biome::Corrupted;
                                t.refresh_fertility();
                            }
                        }
                        for id in self.units_in_radius(h, 0) {
                            if let Some(u) = self.unit_mut(id) {
                                if u.race.is_civilized() {
                                    u.add_trait(traits::INFECTED);
                                }
                            }
                        }
                    }
                    CloudKind::Love => {
                        for id in self.units_in_radius(h, 0) {
                            if let Some(u) = self.unit_mut(id) {
                                u.hunger = 0;
                                u.apply_status(StatusKind::Caffeinated, 40);
                            }
                        }
                    }
                }
            }
            if self.effects.clouds[i].ticks_left > 0 {
                self.effects.clouds[i].ticks_left -= 1;
            }
            if self.effects.clouds[i].ticks_left == 0 {
                expired.push(i);
            }
        }
        for i in expired.into_iter().rev() {
            self.effects.clouds.remove(i);
        }
    }

    fn volcano_tick(&mut self) {
        if self.effects.volcanoes.is_empty() {
            return;
        }
        let mut finished = Vec::new();
        for i in 0..self.effects.volcanoes.len() {
            let (pos, power) = {
                let v = &self.effects.volcanoes[i];
                (v.pos, v.power)
            };
            // Build the cone and spew lava.
            for h in pos.spiral(1) {
                if let Some(t) = self.tile_mut(h) {
                    if t.is_water() {
                        continue;
                    }
                    if h == pos {
                        t.biome = Biome::Mountain;
                        t.elevation = t.elevation.saturating_add(24).min(900);
                    }
                    t.lava = t.lava.max(1).min(power as u8);
                    t.scorch = (t.scorch + 1).min(9);
                    t.refresh_fertility();
                }
            }
            for h in pos.ring(3) {
                if self.rng.chance(0.25) {
                    if let Some(t) = self.tile_mut(h) {
                        if !t.is_water() {
                            t.lava = t.lava.max(1);
                        }
                    }
                }
                if self.rng.chance(0.2) {
                    self.ignite(h, 20);
                }
            }
            for id in self.units_in_radius(pos, 2) {
                self.damage_unit(id, 12, None);
            }
            if self.effects.volcanoes[i].ticks_left > 0 {
                self.effects.volcanoes[i].ticks_left -= 1;
            }
            if self.effects.volcanoes[i].ticks_left == 0 {
                finished.push(i);
            }
        }
        for i in finished.into_iter().rev() {
            self.effects.volcanoes.remove(i);
        }
    }

    fn black_hole_tick(&mut self) {
        if self.effects.black_holes.is_empty() {
            return;
        }
        let mut finished = Vec::new();
        for i in 0..self.effects.black_holes.len() {
            let (pos, radius) = {
                let b = &self.effects.black_holes[i];
                (b.pos, b.radius)
            };
            let collapsed = self.effects.black_holes[i].ticks_left == 0;
            let ids = self.units_in_radius(pos, radius);
            for id in ids {
                if collapsed {
                    self.kill_unit(id, None);
                    continue;
                }
                // Pull one step toward the centre, hurting all the way in.
                let (cur, dist) = match self.unit(id) {
                    Some(u) => (u.pos, u.pos.distance(pos)),
                    None => continue,
                };
                self.damage_unit(id, 2 + (radius - dist).max(0) * 3, None);
                if dist > 1 {
                    let line = cur.line_to(pos);
                    let next = line[1.min(line.len() - 1)];
                    if let Some(u) = self.unit_mut(id) {
                        u.pos = next;
                        u.path.clear();
                        u.path_i = 0;
                    }
                }
            }
            if collapsed {
                // Collapse: rubble, ash and a scar in the ground.
                for h in pos.spiral(radius) {
                    if let Some(t) = self.tile_mut(h) {
                        if t.is_water() {
                            continue;
                        }
                        t.biome = Biome::Wasteland;
                        t.trees = 0;
                        t.scorch = 9;
                        t.refresh_fertility();
                        t.owner = None;
                    }
                }
                self.chronicle(
                    EventKind::Disaster,
                    format!("A black hole collapses at {pos}"),
                );
                finished.push(i);
            } else if self.effects.black_holes[i].ticks_left > 0 {
                self.effects.black_holes[i].ticks_left -= 1;
            }
        }
        for i in finished.into_iter().rev() {
            self.effects.black_holes.remove(i);
        }
    }

    /// Fire burns, spreads to flammable neighbours, hurts whoever stands in it
    /// and leaves ash behind.
    pub fn fire_tick(&mut self) {
        let rate = self.age.age.fire_rate() as f32 / 100.0;
        let len = self.tiles.len();
        let mut ignitions: Vec<usize> = Vec::new();
        for i in 0..len {
            if self.tiles[i].fire == 0 {
                continue;
            }
            // Burn down.
            self.tiles[i].fire -= 1;
            if self.tiles[i].fire == 0 {
                // Burnt out: blackened ground and no trees.
                self.tiles[i].trees = 0;
                self.tiles[i].scorch = (self.tiles[i].scorch + 1).min(9);
                if self.tiles[i].biome.can_burn() && self.rng.chance(0.35) {
                    self.tiles[i].biome = if self.rng.chance(0.4) {
                        Biome::Ash
                    } else {
                        Biome::Wasteland
                    };
                    let mut t = self.tiles[i];
                    t.refresh_fertility();
                    self.tiles[i] = t;
                }
                continue;
            }
            // Spread.
            let (c, r) = ((i % self.width as usize) as i32, (i / self.width as usize) as i32);
            let h = Hex::from_offset(c, r);
            let fuel = 1.0 + self.tiles[i].trees as f32 * 0.4;
            // Every flammable neighbour is rolled separately: a campfire in a dry
            // forest spreads in whichever direction the wind-blown sparks land.
            let chance = (0.12 * rate * fuel).clamp(0.0, 0.6);
            for dir in 0..6usize {
                let roll = self.rng.chance(chance);
                if !roll {
                    continue;
                }
                let nb = h.neighbor(dir);
                if let Some(ni) = self.idx(nb) {
                    if self.tiles[ni].fire == 0 && self.tiles[ni].biome.can_burn() {
                        ignitions.push(ni);
                    }
                }
            }
            // Nobody likes standing in a bonfire.
            let ids = self.units_in_radius(h, 0);
            for id in ids {
                if let Some(u) = self.unit_mut(id) {
                    if !u.has_trait(traits::FIREPROOF) {
                        u.apply_status(StatusKind::Burning, 30);
                    }
                }
            }
            // Fire damages the village that owns the tile.
            if let Some(v) = self.tiles[i].owner {
                if self.rng.chance(0.02) {
                    self.village_building_damage(v, 1);
                }
            }
        }
        for ni in ignitions {
            self.tiles[ni].fire = 24;
        }
    }

    /// Lava spreads downhill, cools into infernal rock and finally into ash.
    pub fn lava_tick(&mut self) {
        let len = self.tiles.len();
        let mut new_lava: Vec<usize> = Vec::new();
        for i in 0..len {
            if self.tiles[i].lava == 0 {
                continue;
            }
            // Cool down over time. Keyed on a chance rather than the tick counter
            // so it behaves the same in scripted bursts as in a running world.
            if self.rng.chance(0.03) {
                self.tiles[i].lava -= 1;
                if self.tiles[i].lava == 0 {
                    self.tiles[i].biome = Biome::Infernal;
                    self.tiles[i].scorch = (self.tiles[i].scorch + 2).min(9);
                    let mut t = self.tiles[i];
                    t.refresh_fertility();
                    self.tiles[i] = t;
                }
            }
            let (c, r) = ((i % self.width as usize) as i32, (i / self.width as usize) as i32);
            let h = Hex::from_offset(c, r);
            if self.rng.chance(0.22) {
                let nb = h.neighbor(self.rng.below(6) as usize);
                if let Some(ni) = self.idx(nb) {
                    if self.tiles[ni].biome.is_land()
                        && self.tiles[ni].biome != Biome::Mountain
                        && self.tiles[ni].lava == 0
                    {
                        new_lava.push(ni);
                    }
                }
            }
            // Anything standing in lava dies fast (fireproof things survive).
            for id in self.units_in_radius(h, 0) {
                self.damage_unit(id, 30, None);
            }
        }
        for ni in new_lava {
            self.tiles[ni].lava = self.tiles[ni].lava.max(1);
            if self.tiles[ni].trees > 0 {
                self.tiles[ni].trees = 0;
            }
        }
    }

    /// Spawn the creatures a disaster leaves behind (used by the CLI's `ruin`).
    pub fn scatter_monsters(&mut self, at: Hex, count: i32) -> u32 {
        let mut spawned = 0;
        for _ in 0..count {
            let h = Hex::from_offset(
                at.to_offset().0 + self.rng.range(-2, 2),
                at.to_offset().1 + self.rng.range(-2, 2),
            );
            if let Some(t) = self.tile(h) {
                if t.is_walkable()
                    && self
                        .spawn_unit(Race::Zombie, h, UnitKind::Monster)
                        .is_some()
                    {
                        spawned += 1;
                    }
            }
        }
        spawned
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worldgen::{GenParams, WorldType};

    fn world() -> World {
        World::generate(GenParams::new(40, 30, 7, WorldType::Continents))
    }

    #[test]
    fn explosion_kills_units_at_the_centre_and_spares_the_edge() {
        let mut w = world();
        let at = w.random_land_tile().unwrap();
        let near = w.spawn_unit(Race::Human, at, crate::units::UnitKind::Civilian).unwrap();
        let far_h = at.neighbor(0).neighbor(0).neighbor(0);
        let far = w
            .spawn_unit(Race::Human, far_h, crate::units::UnitKind::Civilian)
            .unwrap();
        let killed = w.explode(at, 3, 500, None);
        assert!(killed >= 1, "the unit at the centre should die");
        assert!(w.unit(near).is_none());
        assert!(w.unit(far).is_some(), "3 tiles away should survive");
    }

    #[test]
    fn nuke_scorches_the_ground_and_poisons_survivors() {
        let mut w = world();
        let at = w.random_land_tile().unwrap();
        for _ in 0..6 {
            w.spawn_unit(Race::Human, at, crate::units::UnitKind::Civilian);
        }
        let at2 = Hex::from_offset(at.to_offset().0 + 5, at.to_offset().1);
        let survivor = w.spawn_unit(Race::Human, at2, crate::units::UnitKind::Civilian);
        w.nuke(at, 600);
        let center = w.tile(at).unwrap();
        assert_eq!(center.scorch, 6);
        assert!(center.trees == 0);
        assert!(w.stats.nukes_dropped == 1);
        if let Some(id) = survivor {
            if let Some(u) = w.unit(id) {
                assert!(u.has_status(StatusKind::Poisoned));
            }
        }
    }

    #[test]
    fn fire_spreads_burns_out_and_leaves_scars() {
        let mut w = world();
        // Find a flammable tile that has at least one flammable neighbour.
        let mut spot = None;
        'outer: for i in 0..w.tiles.len() {
            if !w.tiles[i].biome.can_burn() {
                continue;
            }
            let (c, r) = ((i % w.width as usize) as i32, (i / w.width as usize) as i32);
            let h = Hex::from_offset(c, r);
            for nb in h.neighbors() {
                if let Some(t) = w.tile(nb) {
                    if t.biome.can_burn() {
                        spot = Some(h);
                        break 'outer;
                    }
                }
            }
        }
        let h = spot.expect("a flammable tile");
        let idx = w.idx(h).unwrap();
        w.tiles[idx].fire = 10;
        let mut saw_fire_elsewhere = false;
        for _ in 0..200 {
            w.fire_tick();
            if w.tiles.iter().filter(|t| t.fire > 0).count() > 1 {
                saw_fire_elsewhere = true;
            }
        }
        assert!(saw_fire_elsewhere, "fire should spread to neighbours");
        assert!(
            w.tile(h).unwrap().scorch > 0,
            "burnt ground should be scarred"
        );
    }

    #[test]
    fn meteors_fall_over_several_ticks_then_impact() {
        let mut w = world();
        let at = w.random_land_tile().unwrap();
        w.cast_meteor(at, 3, 300, false);
        assert_eq!(w.effects.meteors.len(), 1);
        let start = w.effects.meteors[0].pos;
        w.meteor_tick();
        assert_ne!(w.effects.meteors[0].pos, start, "the rock should move");
        for _ in 0..20 {
            w.disaster_tick();
        }
        assert!(w.effects.meteors.is_empty(), "meteor should have landed");
        assert!(w.stats.explosions >= 1);
    }

    #[test]
    fn volcano_builds_a_mountain_and_spills_lava() {
        let mut w = world();
        let at = w.random_land_tile().unwrap();
        w.cast_volcano(at);
        for _ in 0..120 {
            w.step();
        }
        assert_eq!(w.tile(at).unwrap().biome, Biome::Mountain);
        let lava = w.tiles.iter().filter(|t| t.lava > 0).count();
        assert!(lava > 1, "lava should spread around the vent, saw {lava}");
    }

    #[test]
    fn lava_cools_into_infernal_ground() {
        let mut w = world();
        let at = w.random_land_tile().unwrap();
        if let Some(t) = w.tile_mut(at) {
            t.lava = 1;
        }
        for _ in 0..200 {
            w.lava_tick();
        }
        assert_eq!(w.tile(at).unwrap().biome, Biome::Infernal);
        assert_eq!(w.tile(at).unwrap().lava, 0);
    }

    #[test]
    fn rain_puts_out_fires_and_acid_rain_hurts() {
        let mut w = world();
        let at = w.random_land_tile().unwrap();
        if let Some(t) = w.tile_mut(at) {
            t.biome = Biome::Forest;
            t.fire = 30;
        }
        w.cast_cloud(at, CloudKind::Rain);
        for _ in 0..3 {
            w.cloud_tick();
        }
        assert_eq!(w.tile(at).unwrap().fire, 0, "rain should extinguish fire");

        let u = w.spawn_unit(Race::Human, at, crate::units::UnitKind::Civilian).unwrap();
        let hp = w.unit(u).unwrap().hp;
        w.cast_cloud(at, CloudKind::AcidRain);
        for _ in 0..20 {
            w.cloud_tick();
        }
        assert!(w.unit(u).map(|u| u.hp).unwrap_or(0) < hp, "acid rain should hurt");
    }

    #[test]
    fn black_hole_pulls_and_then_implodes() {
        let mut w = world();
        let at = w.random_land_tile().unwrap();
        let far = Hex::from_offset(at.to_offset().0 + 3, at.to_offset().1);
        let id = w.spawn_unit(Race::Human, far, crate::units::UnitKind::Civilian).unwrap();
        w.cast_black_hole(at, 4);
        let d0 = w.unit(id).unwrap().pos.distance(at);
        w.black_hole_tick();
        let d1 = w.unit(id).map(|u| u.pos.distance(at)).unwrap_or(0);
        assert!(d1 <= d0, "the hole should pull units inward");
        for _ in 0..70 {
            w.disaster_tick();
        }
        assert!(w.unit(id).is_none(), "the collapse should kill what is left");
        assert!(w.effects.black_holes.is_empty());
    }

    #[test]
    fn effects_clear_resets_everything() {
        let mut w = world();
        let at = w.random_land_tile().unwrap();
        w.cast_meteor(at, 2, 100, false);
        w.cast_tornado(at, 20);
        w.cast_cloud(at, CloudKind::Rain);
        assert!(w.effects.count() >= 3);
        w.effects.clear();
        assert!(w.effects.is_empty());
    }
}

