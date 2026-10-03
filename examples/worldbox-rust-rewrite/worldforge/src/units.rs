//! Units: the creatures that walk the world.
//!
//! A [`Unit`] is a race plus a pile of mutable state: health, levels, permanent
//! trait bits, timed status effects, an inventory item, a task, a path. Combat
//! maths, status ticking and the per-tick AI all live here.

use crate::hex::Hex;
use crate::path::{find_path, PathMode};
use crate::races::{Category, Race};
use crate::terrain::{Biome, Tile};
use crate::village::Village;
use crate::world::World;

/// Permanent, boolean traits. Stored as bit flags in a `u64` so a trait-less unit
/// costs nothing and saves as 8 bytes.
pub mod traits {
    pub const IMMORTAL: u64 = 1 << 0;
    pub const BLESSED: u64 = 1 << 1;
    pub const CURSED: u64 = 1 << 2;
    pub const MAD: u64 = 1 << 3;
    pub const ZOMBIE: u64 = 1 << 4;
    pub const INFECTED: u64 = 1 << 5;
    pub const SHIELDED: u64 = 1 << 6;
    pub const CAFFEINATED: u64 = 1 << 7;
    pub const POWERED: u64 = 1 << 8;
    pub const ENRAGED: u64 = 1 << 9;
    pub const MOUNTAIN_WALKER: u64 = 1 << 10;
    pub const MINER: u64 = 1 << 11;
    pub const SWIMMER: u64 = 1 << 12;
    pub const FLYING: u64 = 1 << 13;
    pub const CANNIBAL: u64 = 1 << 14;
    pub const FAST_BREEDER: u64 = 1 << 15;
    pub const NATURE_LOVER: u64 = 1 << 16;
    pub const SMART: u64 = 1 << 17;
    pub const WARRIOR: u64 = 1 << 18;
    pub const BOSS: u64 = 1 << 19;
    pub const INFECTIOUS: u64 = 1 << 20;
    pub const FIREPROOF: u64 = 1 << 21;
    pub const GIANT: u64 = 1 << 22;
    pub const BLESSED_ARMOR: u64 = 1 << 23;

    /// Human-readable name, for logs and the CLI inspector.
    pub fn name(bit: u64) -> &'static str {
        match bit {
            IMMORTAL => "immortal",
            BLESSED => "blessed",
            CURSED => "cursed",
            MAD => "mad",
            ZOMBIE => "zombie",
            INFECTED => "infected",
            SHIELDED => "shielded",
            CAFFEINATED => "caffeinated",
            POWERED => "powered",
            ENRAGED => "enraged",
            MOUNTAIN_WALKER => "mountain walker",
            MINER => "miner",
            SWIMMER => "swimmer",
            FLYING => "flying",
            CANNIBAL => "cannibal",
            FAST_BREEDER => "fast breeder",
            NATURE_LOVER => "nature lover",
            SMART => "smart",
            WARRIOR => "warrior",
            BOSS => "boss",
            INFECTIOUS => "infectious",
            FIREPROOF => "fireproof",
            GIANT => "giant",
            BLESSED_ARMOR => "blessed armor",
            _ => "unknown",
        }
    }

    /// Every named bit, for iteration in the inspector.
    pub const ALL: [u64; 24] = [
        IMMORTAL,
        BLESSED,
        CURSED,
        MAD,
        ZOMBIE,
        INFECTED,
        SHIELDED,
        CAFFEINATED,
        POWERED,
        ENRAGED,
        MOUNTAIN_WALKER,
        MINER,
        SWIMMER,
        FLYING,
        CANNIBAL,
        FAST_BREEDER,
        NATURE_LOVER,
        SMART,
        WARRIOR,
        BOSS,
        INFECTIOUS,
        FIREPROOF,
        GIANT,
        BLESSED_ARMOR,
    ];
}

/// Timed effects. Each stores a remaining tick count in `Unit::statuses`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StatusKind {
    Burning,
    Poisoned,
    Bleeding,
    Plague,
    Frozen,
    Mad,
    Shielded,
    Caffeinated,
    Sleeping,
    Webbed,
    Enraged,
    Wet,
}

impl StatusKind {
    pub const COUNT: usize = 12;
    pub const ALL: [StatusKind; 12] = [
        StatusKind::Burning,
        StatusKind::Poisoned,
        StatusKind::Bleeding,
        StatusKind::Plague,
        StatusKind::Frozen,
        StatusKind::Mad,
        StatusKind::Shielded,
        StatusKind::Caffeinated,
        StatusKind::Sleeping,
        StatusKind::Webbed,
        StatusKind::Enraged,
        StatusKind::Wet,
    ];

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn name(self) -> &'static str {
        match self {
            StatusKind::Burning => "burning",
            StatusKind::Poisoned => "poisoned",
            StatusKind::Bleeding => "bleeding",
            StatusKind::Plague => "plague",
            StatusKind::Frozen => "frozen",
            StatusKind::Mad => "mad",
            StatusKind::Shielded => "shielded",
            StatusKind::Caffeinated => "caffeinated",
            StatusKind::Sleeping => "sleeping",
            StatusKind::Webbed => "webbed",
            StatusKind::Enraged => "enraged",
            StatusKind::Wet => "wet",
        }
    }

    /// Default duration in ticks when a power or a bite applies the status.
    pub fn default_ticks(self) -> u16 {
        match self {
            StatusKind::Burning => 30,
            StatusKind::Poisoned => 40,
            StatusKind::Bleeding => 20,
            StatusKind::Plague => 140,
            StatusKind::Frozen => 30,
            StatusKind::Mad => 80,
            StatusKind::Shielded => 60,
            StatusKind::Caffeinated => 100,
            StatusKind::Sleeping => 60,
            StatusKind::Webbed => 25,
            StatusKind::Enraged => 70,
            StatusKind::Wet => 20,
        }
    }
}

/// Carried resources.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Resource {
    Wood,
    Stone,
    Ore,
    Gold,
    Food,
}

impl Resource {
    pub fn name(self) -> &'static str {
        match self {
            Resource::Wood => "wood",
            Resource::Stone => "stone",
            Resource::Ore => "ore",
            Resource::Gold => "gold",
            Resource::Food => "food",
        }
    }
}

/// Inventory: one item slot, like the base game's units.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[derive(Default)]
pub enum ItemKind {
    #[default]
    None,
    Sword,
    Bow,
    Spear,
    Axe,
    Hammer,
    Staff,
    Scythe,
    Armor,
    Shield,
    HealthPotion,
    Bomb,
    FireBomb,
    AcidBomb,
    TeleportScroll,
}

impl ItemKind {
    pub const ALL: [ItemKind; 15] = [
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
    ];

    pub fn name(self) -> &'static str {
        match self {
            ItemKind::None => "nothing",
            ItemKind::Sword => "sword",
            ItemKind::Bow => "bow",
            ItemKind::Spear => "spear",
            ItemKind::Axe => "axe",
            ItemKind::Hammer => "hammer",
            ItemKind::Staff => "staff",
            ItemKind::Scythe => "scythe",
            ItemKind::Armor => "armor",
            ItemKind::Shield => "shield",
            ItemKind::HealthPotion => "health potion",
            ItemKind::Bomb => "bomb",
            ItemKind::FireBomb => "fire bomb",
            ItemKind::AcidBomb => "acid bomb",
            ItemKind::TeleportScroll => "teleport scroll",
        }
    }

    pub fn is_weapon(self) -> bool {
        matches!(
            self,
            ItemKind::Sword
                | ItemKind::Bow
                | ItemKind::Spear
                | ItemKind::Axe
                | ItemKind::Hammer
                | ItemKind::Staff
                | ItemKind::Scythe
        )
    }

    pub fn is_bomb(self) -> bool {
        matches!(self, ItemKind::Bomb | ItemKind::FireBomb | ItemKind::AcidBomb)
    }

    /// Damage added by this item at `tier` (0..=3).
    pub fn damage_bonus(self, tier: u8) -> i32 {
        let t = tier.min(3) as i32;
        match self {
            ItemKind::Sword => 4 + 3 * t,
            ItemKind::Bow => 3 + 2 * t,
            ItemKind::Spear => 5 + 3 * t,
            ItemKind::Axe => 6 + 4 * t,
            ItemKind::Hammer => 7 + 5 * t,
            ItemKind::Staff => 6 + 4 * t,
            ItemKind::Scythe => 4 + 3 * t,
            _ => 0,
        }
    }

    pub fn armor_bonus(self, tier: u8) -> i32 {
        let t = tier.min(3) as i32;
        match self {
            ItemKind::Armor => 2 + 2 * t,
            ItemKind::Shield => 1 + t,
            _ => 0,
        }
    }
}

/// One inventory slot.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Item {
    pub kind: ItemKind,
    pub tier: u8,
}


impl Item {
    pub fn new(kind: ItemKind, tier: u8) -> Self {
        Item {
            kind,
            tier: tier.min(3),
        }
    }

    pub fn none() -> Self {
        Item {
            kind: ItemKind::None,
            tier: 0,
        }
    }

    pub fn is_none(&self) -> bool {
        self.kind == ItemKind::None
    }

    pub fn name(&self) -> String {
        if self.is_none() {
            "nothing".to_string()
        } else {
            format!("{} (t{})", self.kind.name(), self.tier)
        }
    }
}

/// What a unit is currently doing. The AI sets this; renderers and the CLI show
/// it; tests assert on it.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum UnitState {
    #[default]
    Idle,
    Wander,
    Sleep,
    Graze,
    Hunt,
    Flee,
    /// Walking to a resource, then harvesting it.
    Gather,
    /// Walking home with resources in hand.
    Deliver,
    /// Adding work to a building site.
    Build,
    /// Walking a patrol route around the village.
    Patrol,
    /// Part of an army marching to a target village.
    March,
    /// In melee with a specific enemy.
    Attack,
    /// Killing everything nearby (madness, monsters).
    Rampage,
    /// The king's guard, following the leader.
    Follow,
    /// Crossing water in a boat.
    Sail,
}

impl UnitState {
    pub fn name(self) -> &'static str {
        match self {
            UnitState::Idle => "idle",
            UnitState::Wander => "wandering",
            UnitState::Sleep => "sleeping",
            UnitState::Graze => "grazing",
            UnitState::Hunt => "hunting",
            UnitState::Flee => "fleeing",
            UnitState::Gather => "gathering",
            UnitState::Deliver => "hauling",
            UnitState::Build => "building",
            UnitState::Patrol => "patrolling",
            UnitState::March => "marching",
            UnitState::Attack => "fighting",
            UnitState::Rampage => "rampaging",
            UnitState::Follow => "escorting",
            UnitState::Sail => "sailing",
        }
    }
}

/// A living creature.
#[derive(Clone, Debug)]
pub struct Unit {
    pub id: u32,
    pub alive: bool,
    pub race: Race,
    pub kind: UnitKind,
    /// Name; only leaders, kings and bosses get one.
    pub name: String,
    pub pos: Hex,
    pub hp: i32,
    pub max_hp: i32,
    pub damage: i32,
    pub armor: i32,
    pub speed: i32,
    pub accuracy: f32,
    pub dodge: f32,
    pub attack_cooldown: u8,
    pub cooldown: u8,
    pub traits: u64,
    pub statuses: [u16; StatusKind::COUNT],
    /// Age in years.
    pub age: u16,
    pub level: u8,
    pub xp: u32,
    pub kills: u32,
    /// Accumulated movement points; spend `tile cost` to step.
    pub move_points: i32,
    pub state: UnitState,
    pub village: Option<u32>,
    pub kingdom: Option<u32>,
    pub enemy: Option<u32>,
    pub destination: Option<Hex>,
    pub home: Hex,
    pub path: Vec<Hex>,
    pub path_i: usize,
    /// What this unit is carrying home.
    pub carry: Option<(Resource, u8)>,
    pub item: Item,
    /// 0 = full, 100 = starving.
    pub hunger: u8,
    /// True while crossing water in a boat.
    pub sailing: bool,
    /// For soldiers: how much work they have put into a siege.
    pub siege_progress: u16,
}

/// Player-visible role of a unit.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum UnitKind {
    #[default]
    Civilian,
    /// Village leader (becomes a king when the village founds a kingdom).
    Leader,
    King,
    Soldier,
    Animal,
    Monster,
}

impl UnitKind {
    pub fn name(self) -> &'static str {
        match self {
            UnitKind::Civilian => "civilian",
            UnitKind::Leader => "leader",
            UnitKind::King => "king",
            UnitKind::Soldier => "soldier",
            UnitKind::Animal => "animal",
            UnitKind::Monster => "monster",
        }
    }
}

impl Unit {
    /// Build a unit of `race` at `pos`, applying the race template.
    pub fn new(id: u32, race: Race, pos: Hex, kind: UnitKind) -> Self {
        let d = race.def();
        let kind = if d.category == Category::Animal {
            UnitKind::Animal
        } else if d.category == Category::Monster || d.category == Category::Boss {
            UnitKind::Monster
        } else {
            kind
        };
        Unit {
            id,
            alive: true,
            race,
            kind,
            name: String::new(),
            pos,
            hp: d.hp,
            max_hp: d.hp,
            damage: d.damage,
            armor: d.armor,
            speed: d.speed,
            accuracy: d.accuracy,
            dodge: d.dodge,
            attack_cooldown: d.attack_cooldown,
            cooldown: 0,
            traits: d.trait_bits,
            statuses: [0; StatusKind::COUNT],
            age: 0,
            level: 1,
            xp: 0,
            kills: 0,
            move_points: 0,
            state: UnitState::Idle,
            village: None,
            kingdom: None,
            enemy: None,
            destination: None,
            home: pos,
            path: Vec::new(),
            path_i: 0,
            carry: None,
            item: Item::none(),
            hunger: 0,
            sailing: false,
            siege_progress: 0,
        }
    }

    // --- traits ------------------------------------------------------------

    pub fn has_trait(&self, bit: u64) -> bool {
        self.traits & bit != 0
    }

    pub fn add_trait(&mut self, bit: u64) -> bool {
        let had = self.has_trait(bit);
        self.traits |= bit;
        !had
    }

    pub fn remove_trait(&mut self, bit: u64) -> bool {
        let had = self.has_trait(bit);
        self.traits &= !bit;
        had
    }

    pub fn trait_names(&self) -> Vec<&'static str> {
        traits::ALL
            .into_iter()
            .filter(|b| self.has_trait(*b))
            .map(traits::name)
            .collect()
    }

    // --- statuses ----------------------------------------------------------

    pub fn has_status(&self, s: StatusKind) -> bool {
        self.statuses[s.index()] > 0
    }

    pub fn status_ticks(&self, s: StatusKind) -> u16 {
        self.statuses[s.index()]
    }

    /// Apply a status, keeping the longer remaining duration.
    pub fn apply_status(&mut self, s: StatusKind, ticks: u16) {
        if self.has_trait(traits::IMMORTAL) && s == StatusKind::Plague {
            // Immortals can carry the plague but not suffer from it.
            return;
        }
        let slot = &mut self.statuses[s.index()];
        *slot = (*slot).max(ticks);
    }

    pub fn clear_status(&mut self, s: StatusKind) {
        self.statuses[s.index()] = 0;
    }

    pub fn clear_statuses(&mut self) {
        self.statuses = [0; StatusKind::COUNT];
    }

    pub fn status_names(&self) -> Vec<&'static str> {
        StatusKind::ALL
            .into_iter()
            .filter(|s| self.has_status(*s))
            .map(|s| s.name())
            .collect()
    }

    // --- derived stats -----------------------------------------------------

    pub fn item_damage_bonus(&self) -> i32 {
        if self.item.kind.is_weapon() {
            self.item.kind.damage_bonus(self.item.tier)
        } else {
            0
        }
    }

    pub fn item_armor_bonus(&self) -> i32 {
        self.item.kind.armor_bonus(self.item.tier)
    }

    /// Final attack power after item, level, traits and statuses.
    pub fn attack_power(&self) -> i32 {
        let mut dmg = self.damage + self.item_damage_bonus();
        dmg += dmg * (self.level.saturating_sub(1) as i32) / 12;
        if self.has_trait(traits::BLESSED) {
            dmg = dmg * 3 / 2;
        }
        if self.has_trait(traits::CURSED) {
            dmg = dmg * 2 / 3;
        }
        if self.has_trait(traits::POWERED) {
            dmg *= 2;
        }
        if self.has_trait(traits::ENRAGED) || self.has_status(StatusKind::Enraged) {
            dmg = dmg * 3 / 2;
        }
        if self.has_status(StatusKind::Poisoned) {
            dmg = dmg * 4 / 5;
        }
        if self.has_trait(traits::GIANT) {
            dmg = dmg * 3 / 2;
        }
        if self.race == Race::Zombie || self.has_trait(traits::ZOMBIE) {
            dmg = dmg * 3 / 2;
        }
        dmg.max(1)
    }

    /// Final armour after item, traits and statuses.
    pub fn armor_value(&self) -> i32 {
        let mut armor = self.armor + self.item_armor_bonus();
        if self.has_trait(traits::BLESSED) || self.has_trait(traits::BLESSED_ARMOR) {
            armor += 3;
        }
        if self.has_trait(traits::CURSED) {
            armor -= 2;
        }
        if self.has_status(StatusKind::Frozen) {
            armor -= 1;
        }
        armor.max(0)
    }

    /// Hit chance against `target`, clamped to a sane range.
    pub fn hit_chance(&self, target: &Unit) -> f32 {
        let mut acc = self.accuracy;
        if self.has_status(StatusKind::Sleeping) {
            return 0.0;
        }
        if self.has_status(StatusKind::Caffeinated) || self.has_trait(traits::CAFFEINATED) {
            acc += 0.05;
        }
        if self.has_status(StatusKind::Webbed) {
            acc -= 0.25;
        }
        let mut dodge = target.dodge;
        if target.has_status(StatusKind::Sleeping) {
            dodge = 0.0;
        }
        if target.has_status(StatusKind::Frozen) || target.has_status(StatusKind::Webbed) {
            dodge = 0.0;
        }
        if target.has_trait(traits::CAFFEINATED) {
            dodge += 0.05;
        }
        (acc - dodge).clamp(0.05, 0.98)
    }

    /// Movement budget per tick, before terrain.
    pub fn move_speed(&self) -> i32 {
        let mut speed = self.speed;
        if self.has_status(StatusKind::Caffeinated) || self.has_trait(traits::CAFFEINATED) {
            speed = speed * 3 / 2;
        }
        if self.has_status(StatusKind::Frozen) || self.has_status(StatusKind::Webbed) {
            speed = 0;
        }
        if self.has_status(StatusKind::Sleeping) {
            speed = 0;
        }
        if self.sailing {
            speed /= 2; // boats are slow
        }
        speed
    }

    /// Can this unit stand on this tile?
    pub fn can_enter(&self, tile: &Tile) -> bool {
        if tile.has_lava() && !self.has_trait(traits::FLYING) && !self.has_trait(traits::FIREPROOF) {
            return false;
        }
        if self.has_trait(traits::FLYING) {
            return true;
        }
        if tile.is_water() {
            return self.sailing || self.has_trait(traits::SWIMMER);
        }
        if tile.is_mountain() {
            return self.has_trait(traits::MOUNTAIN_WALKER);
        }
        tile.is_land()
    }

    /// Terrain cost in movement points. Higher is slower.
    pub fn terrain_cost(&self, tile: &Tile) -> i32 {
        crate::path::terrain_cost_for(tile, self.sailing)
    }

    #[allow(dead_code)]
    fn legacy_terrain_cost(&self, tile: &Tile) -> i32 {
        let mut cost = match tile.biome {
            Biome::Forest | Biome::Jungle => 56,
            Biome::Taiga => 50,
            Biome::Swamp => 64,
            Biome::Mushroom => 60,
            Biome::Desert => 44,
            Biome::Snow | Biome::Permafrost | Biome::Tundra => 52,
            Biome::Mountain => 96,
            Biome::Ocean | Biome::Shallow | Biome::Ice => 60,
            Biome::Beach | Biome::Grass | Biome::Savanna | Biome::Wasteland | Biome::Ash => 40,
            _ => 44,
        };
        if tile.road >= 2 {
            cost = cost * 55 / 100;
        } else if tile.road == 1 {
            cost = cost * 75 / 100;
        }
        if tile.river && !self.sailing && !self.has_trait(traits::FLYING) {
            cost = cost * 3 / 2;
        }
        cost.max(10)
    }

    /// Level up from accumulated XP; returns true when a level was gained.
    pub fn try_level_up(&mut self) -> bool {
        let want = 1 + self.xp / 150;
        if want as u8 > self.level && self.level < 10 {
            self.level += 1;
            let hp_bonus = self.max_hp / 10;
            self.max_hp += hp_bonus;
            self.hp += hp_bonus;
            true
        } else {
            false
        }
    }

    /// Add XP and level up if the unit earned it.
    pub fn gain_xp(&mut self, amount: u32) {
        self.xp += amount;
        while self.try_level_up() {}
    }

    pub fn is_civilized(&self) -> bool {
        self.race.is_civilized()
    }

    pub fn is_animal(&self) -> bool {
        self.race.is_animal()
    }

    pub fn is_monster(&self) -> bool {
        self.race.is_monster()
    }

    pub fn is_hostile(&self) -> bool {
        self.race.is_hostile() || self.has_trait(traits::MAD) || self.has_status(StatusKind::Mad)
    }

    /// Does this unit want to attack `other` right now?
    pub fn wants_to_fight(&self, other: &Unit) -> bool {
        if !self.alive || !other.alive || self.id == other.id {
            return false;
        }
        if self.has_status(StatusKind::Sleeping) || self.has_status(StatusKind::Frozen) {
            return false;
        }
        // Mad units and monsters attack everything that moves.
        if self.has_trait(traits::MAD)
            || self.has_status(StatusKind::Mad)
            || self.race.hates_race(other.race)
            || other.race.hates_race(self.race)
        {
            return true;
        }
        // Predators hunt prey races.
        if self.race.is_predator() && self.race.hates_race(other.race) {
            return true;
        }
        // Armies fight armies of enemy kingdoms, and civilians of enemy kingdoms.
        if let (Some(mine), Some(theirs)) = (self.kingdom, other.kingdom) {
            if mine != theirs && self.kind == UnitKind::Soldier {
                return true;
            }
        }
        if self.kind == UnitKind::Soldier && other.kind != UnitKind::Soldier && other.is_civilized()
        {
            return other.kingdom.is_none() || self.kingdom != other.kingdom;
        }
        false
    }

    /// One-line summary for the CLI and the chronicle.
    pub fn describe(&self) -> String {
        let name = if self.name.is_empty() {
            self.race.name().to_string()
        } else {
            self.name.clone()
        };
        let mut s = format!(
            "#{} {} ({}) {}hp lvl{} at {}",
            self.id,
            name,
            self.kind.name(),
            self.hp,
            self.level,
            self.pos
        );
        let t = self.trait_names();
        if !t.is_empty() {
            s.push_str(&format!(" [{}]", t.join(", ")));
        }
        let st = self.status_names();
        if !st.is_empty() {
            s.push_str(&format!(" {{{}}}", st.join(", ")));
        }
        if !self.item.is_none() {
            s.push_str(&format!(" holding {}", self.item.name()));
        }
        s.push_str(&format!(" {}", self.state.name()));
        s
    }
}

impl Tile {
    /// True when this tile is an open, easy-to-walk surface.
    pub fn is_open_ground(&self) -> bool {
        matches!(
            self.biome,
            Biome::Grass
                | Biome::Savanna
                | Biome::Beach
                | Biome::Desert
                | Biome::Wasteland
                | Biome::Ash
                | Biome::Tundra
        )
    }
}

impl World {
    /// Spawn a unit at `pos`. Returns the new unit id, or `None` if the tile is
    /// missing or the unit cannot stand there.
    pub fn spawn_unit(&mut self, race: Race, pos: Hex, kind: UnitKind) -> Option<u32> {
        self.spawn_unit_at(race, pos, kind, None)
    }

    /// Spawn a unit with an explicit village (used for births and settlers).
    pub fn spawn_unit_at(
        &mut self,
        race: Race,
        pos: Hex,
        kind: UnitKind,
        village: Option<u32>,
    ) -> Option<u32> {
        let tile = *self.tile(pos)?;
        let id = self.alloc_unit_slot();
        let mut unit = Unit::new(id, race, pos, kind);
        if !unit.can_enter(&tile) {
            if unit.sailing {
                unit.sailing = false;
            }
            // Flying and swimming things can land anywhere; anything else that
            // cannot stand here simply is not spawned.
            if !unit.can_enter(&tile) {
                self.free_unit_slot(id);
                return None;
            }
        }
        unit.village = village;
        if let Some(v) = village {
            unit.kingdom = self.villages.get(v as usize).and_then(|v| v.kingdom);
        }
        unit.home = pos;
        self.units[id as usize] = unit;
        self.stats.units_spawned += 1;
        Some(id)
    }

    /// Alive unit at `id`.
    pub fn unit(&self, id: u32) -> Option<&Unit> {
        self.units.get(id as usize).filter(|u| u.alive)
    }

    pub fn unit_mut(&mut self, id: u32) -> Option<&mut Unit> {
        self.units.get_mut(id as usize).filter(|u| u.alive)
    }

    /// Every alive unit id, in slot order (deterministic).
    pub fn unit_ids(&self) -> Vec<u32> {
        self.units
            .iter()
            .filter(|u| u.alive)
            .map(|u| u.id)
            .collect()
    }

    pub fn unit_count(&self) -> usize {
        self.units.iter().filter(|u| u.alive).count()
    }

    /// Ids of alive units within `radius` of `center`, nearest first.
    pub fn units_in_radius(&self, center: Hex, radius: i32) -> Vec<u32> {
        let mut hits: Vec<(i32, u32)> = self
            .units
            .iter()
            .filter(|u| u.alive && u.pos.distance(center) <= radius)
            .map(|u| (u.pos.distance(center), u.id))
            .collect();
        hits.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
        hits.into_iter().map(|(_, id)| id).collect()
    }

    /// Heal a unit, clamped to its maximum. Returns the amount actually healed.
    pub fn heal_unit(&mut self, id: u32, amount: i32) -> i32 {
        let Some(u) = self.unit_mut(id) else {
            return 0;
        };
        let before = u.hp;
        u.hp = (u.hp + amount).min(u.max_hp);
        u.hp - before
    }

    /// Hurt a unit. Handles shields, immunities, death, XP and corpse effects
    /// (zombie reanimation, loot). Returns true when the unit died.
    pub fn damage_unit(&mut self, id: u32, raw: i32, source: Option<u32>) -> bool {
        if raw <= 0 {
            return false;
        }
        let (died, reduce) = {
            let Some(u) = self.units.get(id as usize) else {
                return false;
            };
            if !u.alive {
                return false;
            }
            if u.has_status(StatusKind::Shielded)
                || u.has_trait(traits::SHIELDED)
                || u.has_trait(traits::IMMORTAL) && u.race == Race::Crabzilla
            {
                return false;
            }
            // Armour reduces damage but never below 1.
            let armor = if u.has_status(StatusKind::Burning) {
                u.armor_value() / 2
            } else {
                u.armor_value()
            };
            let dealt = (raw - armor).max(1);
            let npc = self.units.get(id as usize).map(|u| u.hp).unwrap_or(0);
            (npc - dealt <= 0, dealt)
        };
        self.stats.damage_dealt += reduce as u64;
        if let Some(u) = self.units.get_mut(id as usize) {
            u.hp -= reduce;
            // Being hurt wakes you up.
            u.clear_status(StatusKind::Sleeping);
            if u.hp > 0 {
                return false;
            }
            u.hp = 0;
        }
        if died {
            self.kill_unit(id, source);
            return true;
        }
        false
    }

    /// Kill a unit: awards XP to the killer, triggers zombie reanimation and
    /// infection spread, frees the slot later via compaction.
    pub fn kill_unit(&mut self, id: u32, source: Option<u32>) {
        let (was_infected, pos, race, village, kingdom) = match self.units.get(id as usize) {
            Some(u) if u.alive => (
                u.has_trait(traits::INFECTED) || u.has_status(StatusKind::Plague),
                u.pos,
                u.race,
                u.village,
                u.kingdom,
            ),
            _ => return,
        };
        if let Some(u) = self.units.get_mut(id as usize) {
            u.alive = false;
            u.hp = 0;
        }
        self.graveyard.push(id);
        self.stats.kills += 1;
        self.stats.deaths += 1;
        if let Some(src) = source {
            if src != id {
                if let Some(killer) = self.unit_mut(src) {
                    killer.kills += 1;
                    killer.gain_xp(60 + race.def().hp as u32 / 4);
                }
            }
        }
        if let Some(v) = village {
            if let Some(vill) = self.villages.get_mut(v as usize) {
                if vill.alive {
                    vill.pop = vill.pop.saturating_sub(1);
                    vill.deaths += 1;
                }
            }
        }
        if let Some(k) = kingdom {
            if let Some(kd) = self.kingdoms.get_mut(k as usize) {
                if kd.alive {
                    kd.population = kd.population.saturating_sub(1);
                    if kd.king == Some(id) {
                        kd.king = None;
                    }
                }
            }
        }
        // Zombie infection reanimates the corpse.
        if was_infected && race.is_civilized() && !race.is_monster() {
            if let Some(zid) = self.spawn_unit(Race::Zombie, pos, UnitKind::Monster) {
                if let Some(z) = self.units.get_mut(zid as usize) {
                    z.home = pos;
                }
            }
            self.stats.zombies_raised += 1;
        }
    }

    /// Kill every unit in a radius; returns how many died. Sources of "clean"
    /// deaths (Divine Light, black holes) use this.
    pub fn kill_units_in_radius(&mut self, center: Hex, radius: i32) -> u32 {
        let ids = self.units_in_radius(center, radius);
        let mut n = 0;
        for id in ids {
            if self.unit(id).is_some() {
                self.kill_unit(id, None);
                n += 1;
            }
        }
        n
    }

    /// Number of alive units whose race is `race`.
    pub fn count_race(&self, race: Race) -> usize {
        self.units
            .iter()
            .filter(|u| u.alive && u.race == race)
            .count()
    }

    /// The closest unit that `hunter` would attack, within `radius`.
    pub fn nearest_enemy(&self, hunter: u32, radius: i32) -> Option<u32> {
        let me = self.unit(hunter)?;
        let mut best: Option<(i32, u32)> = None;
        for other in self.units.iter().filter(|u| u.alive) {
            if !me.wants_to_fight(other) {
                continue;
            }
            let d = me.pos.distance(other.pos);
            if d > radius {
                continue;
            }
            let better = match best {
                None => true,
                Some((bd, _)) => d < bd,
            };
            if better {
                best = Some((d, other.id));
            }
        }
        best.map(|(_, id)| id)
    }

    /// Resolve one attack from `attacker` against `target`. Applies damage,
    /// status effects from the attacker's race and bombs.
    pub fn resolve_attack(&mut self, attacker: u32, target: u32) {
        let (hit, damage, race, is_bomb, bomb_power) = {
            let Some(a) = self.units.get(attacker as usize).filter(|u| u.alive) else {
                return;
            };
            let Some(t) = self.units.get(target as usize).filter(|u| u.alive) else {
                return;
            };
            let hit = self.rng.chance(a.hit_chance(t));
            let mut dmg = a.attack_power();
            // Critical hits: bigger for hunters and bosses.
            let crit = if a.has_trait(traits::BOSS) { 0.12 } else { 0.05 };
            if self.rng.chance(crit) {
                dmg *= 2;
            }
            let is_bomb = a.item.kind.is_bomb() && self.rng.chance(0.25);
            (
                hit,
                dmg,
                a.race,
                is_bomb,
                a.item.kind.damage_bonus(a.item.tier.max(1)) * 3,
            )
        };
        if !hit {
            return;
        }
        if is_bomb {
            // Bombs are area damage; the target still takes the direct hit.
            let at = self.unit(target).map(|u| u.pos);
            if let Some(at) = at {
                self.explode(at, 2, bomb_power.max(20), Some(attacker));
            }
        }
        self.damage_unit(target, damage, Some(attacker));
        // Racial on-hit statuses.
        let mut apply: Option<(StatusKind, u16)> = None;
        match race {
            Race::Zombie => apply = Some((StatusKind::Plague, 0)), // handled as infection below
            Race::ColdOne => apply = Some((StatusKind::Frozen, StatusKind::Frozen.default_ticks())),
            Race::Spider => apply = Some((StatusKind::Webbed, StatusKind::Webbed.default_ticks())),
            Race::Snake | Race::Scorpion | Race::Alien => {
                apply = Some((StatusKind::Poisoned, StatusKind::Poisoned.default_ticks()))
            }
            Race::Wolf | Race::Bear | Race::Shark | Race::Gorilla | Race::Fox | Race::Eagle => {
                apply = Some((StatusKind::Bleeding, StatusKind::Bleeding.default_ticks()))
            }
            _ => {}
        }
        if let Some((kind, ticks)) = apply {
            if ticks > 0 {
                if let Some(t) = self.unit_mut(target) {
                    t.apply_status(kind, ticks);
                }
            }
        }
        if race == Race::Zombie || race == Race::Tumor || race == Race::Alien {
            // Bites infect.
            if let Some(t) = self.unit_mut(target) {
                if t.race.is_civilized() {
                    t.add_trait(traits::INFECTED);
                }
            }
        }
    }

    /// Give every unit a level-up check and decay of temporary traits.
    pub fn unit_status_tick(&mut self) {
        let mut dead: Vec<(u32, i32)> = Vec::new(); // (unit id, damage)
        for i in 0..self.units.len() {
            if !self.units[i].alive {
                continue;
            }
            let mut dmg = 0i32;
            // Timed statuses.
            for s in StatusKind::ALL {
                let slot = self.units[i].statuses[s.index()];
                if slot == 0 {
                    continue;
                }
                self.units[i].statuses[s.index()] = slot - 1;
                match s {
                    StatusKind::Burning => {
                        if !self.units[i].has_trait(traits::FIREPROOF) {
                            dmg += 3;
                        }
                    }
                    StatusKind::Poisoned => dmg += 1,
                    StatusKind::Bleeding => dmg += 2,
                    StatusKind::Plague => {
                        dmg += 1;
                        // The plague jumps to a neighbour occasionally.
                        if slot % 25 == 0 {
                            let pos = self.units[i].pos;
                            let near = self.units_in_radius(pos, 1);
                            for other in near {
                                if other != self.units[i].id
                                    && self
                                        .unit(other)
                                        .map(|u| u.race.is_civilized())
                                        .unwrap_or(false)
                                    && self.rng.chance(0.5)
                                {
                                    if let Some(u) = self.unit_mut(other) {
                                        u.apply_status(StatusKind::Plague, 140);
                                    }
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
            // Old age: becomes possible after `max_age`, chance grows with age.
            let (race, age) = (self.units[i].race, self.units[i].age);
            let max_age = race.def().max_age;
            if max_age > 0 && age >= max_age && !self.units[i].has_trait(traits::IMMORTAL) {
                let over = (age - max_age) as f32;
                let chance = (over / 40.0).min(0.35);
                if self.rng.chance(chance) {
                    self.units[i].hp = 0;
                }
            }
            if dmg > 0 {
                dead.push((self.units[i].id, dmg));
            }
        }
        for (id, dmg) in dead {
            self.damage_unit(id, dmg, None);
        }
    }

    /// Which village claims this unit, if any.
    pub fn village_of(&self, id: u32) -> Option<&Village> {
        let u = self.unit(id)?;
        let v = u.village?;
        self.villages.get(v as usize).filter(|v| v.alive)
    }

    /// Walk one step along the unit's path. Returns true when a step was taken.
    pub fn unit_step_along_path(&mut self, id: u32) -> bool {
        let (next, cost, blocked) = {
            let Some(u) = self.units.get(id as usize).filter(|u| u.alive) else {
                return false;
            };
            if u.path_i >= u.path.len() {
                return false;
            }
            let next = u.path[u.path_i];
            let Some(tile) = self.tile(next).copied() else {
                return false;
            };
            if u.can_enter(&tile) {
                (next, u.terrain_cost(&tile), false)
            } else {
                (next, 0, true)
            }
        };
        if blocked {
            // Terrain changed under the unit (ice melted, lava appeared, a boat
            // found no water). Re-path the rest of the trip or give up on it.
            let (from, goal) = match self.unit(id) {
                Some(u) => (u.pos, u.path.last().copied().unwrap_or(u.pos)),
                None => return false,
            };
            let mode = self.path_mode_for(id);
            match find_path(self, from, goal, mode) {
                Some(p) => {
                    if let Some(u) = self.unit_mut(id) {
                        u.path_i = if p.len() > 1 { 1 } else { 0 };
                        u.path = p;
                    }
                }
                None => {
                    if let Some(u) = self.unit_mut(id) {
                        u.path.clear();
                        u.path_i = 0;
                        u.destination = None;
                    }
                }
            }
            return false;
        }
        {
            let Some(u) = self.unit_mut(id) else {
                return false;
            };
            if u.move_points < cost {
                return false;
            }
            u.move_points -= cost;
            u.pos = next;
            u.path_i += 1;
        }
        true
    }

    /// The pathing mode this unit should use right now.
    pub fn path_mode_for(&self, id: u32) -> PathMode {
        match self.units.get(id as usize) {
            None => PathMode::Land,
            Some(u) => {
                if u.sailing {
                    PathMode::Sail
                } else if u.has_trait(traits::FLYING) {
                    PathMode::Fly
                } else if u.has_trait(traits::SWIMMER) {
                    PathMode::Amphibious
                } else {
                    PathMode::Land
                }
            }
        }
    }

    /// Send a unit to a destination, computing a path. Returns false when no
    /// route exists in the requested mode.
    pub fn order_move(&mut self, id: u32, dest: Hex) -> bool {
        let mode = self.path_mode_for(id);
        let Some(u) = self.unit(id) else {
            return false;
        };
        let from = u.pos;
        let Some(path) = find_path(self, from, dest, mode) else {
            return false;
        };
        if let Some(u) = self.unit_mut(id) {
            u.destination = Some(dest);
            u.path_i = if path.len() > 1 { 1 } else { 0 };
            u.path = path;
        }
        true
    }

    /// Board a boat so the unit can cross water (used by settlers and armies).
    pub fn board_boat(&mut self, id: u32) -> bool {
        let Some(u) = self.unit_mut(id) else {
            return false;
        };
        if u.sailing || u.has_trait(traits::SWIMMER) || u.has_trait(traits::FLYING) {
            return false;
        }
        u.sailing = true;
        true
    }

    /// Leave the boat when back on dry land.
    pub fn disembark(&mut self, id: u32) {
        let (sailing, pos) = match self.unit(id) {
            Some(u) => (u.sailing, u.pos),
            None => return,
        };
        if !sailing {
            return;
        }
        let dry = self.tile(pos).map(|t| !t.is_water()).unwrap_or(true);
        if dry {
            if let Some(u) = self.unit_mut(id) {
                u.sailing = false;
            }
        }
    }

    /// Push a unit into a fleeing state, away from the given threat.
    pub fn order_flee(&mut self, id: u32, threat: Hex) {
        let Some(u) = self.unit(id) else {
            return;
        };
        let from = u.pos;
        // Find a walkable neighbour that increases distance from the threat.
        let mut options: Vec<(i32, Hex)> = Vec::new();
        for nb in from.neighbors() {
            if let Some(t) = self.tile(nb) {
                if t.is_walkable() {
                    options.push((nb.distance(threat), nb));
                }
            }
        }
        options.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        if let Some((_, target)) = options.first().copied() {
            if let Some(u) = self.unit_mut(id) {
                u.state = UnitState::Flee;
                u.enemy = None;
                u.path = vec![from, target];
                u.path_i = 1;
            }
        }
    }
}
