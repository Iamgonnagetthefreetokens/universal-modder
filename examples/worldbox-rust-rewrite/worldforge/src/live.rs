//! Watch a world develop, live, in a browser tab.
//!
//! `worldforge watch` runs the simulation and answers a tiny HTTP server on the
//! same thread: the world advances between requests, [`state_json`] is one frame of
//! everything that moves (units, villages, kingdoms, wars, the chronicle), and the
//! two heavy layers — [`terrain_json`] and [`territory_json`] — are re-sent only
//! when the map itself changes. `live_page.html` draws it as hexes on a canvas, so
//! what you watch is the simulation, not a replay.
//!
//! The server is hand-written on `std::net`, like the rest of the crate: no
//! dependencies, no framework. It binds `0.0.0.0` on purpose, serves `GET` only,
//! answers every request on its own connection and sets no frame-blocking headers,
//! so a browser that reaches this machine through a dev-preview proxy can open it
//! and the page's relative URLs work wherever it is mounted.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crate::hex::Hex;
use crate::powers::Power;
use crate::races::{Category, Race};
use crate::terrain::Biome;
use crate::units::UnitKind;
use crate::world::World;

/// Where the page lives if nobody says otherwise.
pub const DEFAULT_PORT: u16 = 25608;
/// The page itself, compiled into the binary so the server has nothing to read
/// from disk.
pub const PAGE: &str = include_str!("live_page.html");

const B36: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";
/// The territory alphabet: 62 factions is more than any world here has held.
const FACTIONS: &[u8; 62] = b"0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ";
/// Ceilings so one frame cannot grow without bound on a huge world.
const MAX_UNITS: usize = 600;
const MAX_MARKS: usize = 400;
const MAX_NEWS: usize = 16;
/// How often the heavy layers are rebuilt and compared (they change slowly).
const SCAN: Duration = Duration::from_millis(500);

/// How to serve a world.
#[derive(Clone, Debug)]
pub struct WatchOptions {
    pub host: String,
    pub port: u16,
    pub tps: f32,
}

impl Default for WatchOptions {
    fn default() -> Self {
        WatchOptions {
            host: "0.0.0.0".to_string(),
            port: DEFAULT_PORT,
            tps: 20.0,
        }
    }
}

fn base36(n: usize) -> char {
    B36[n % 36] as char
}

/// JSON string escaping: the names in this crate are generated ASCII, but the
/// server must never be able to emit a broken frame.
pub fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

fn hex6(c: (u8, u8, u8)) -> String {
    format!("#{:02x}{:02x}{:02x}", c.0, c.1, c.2)
}

fn kind_index(kind: UnitKind) -> usize {
    match kind {
        UnitKind::Civilian => 0,
        UnitKind::Leader => 1,
        UnitKind::King => 2,
        UnitKind::Soldier => 3,
        UnitKind::Animal => 4,
        UnitKind::Monster => 5,
    }
}

fn kind_name(kind: UnitKind) -> &'static str {
    match kind {
        UnitKind::Civilian => "civilian",
        UnitKind::Leader => "leader",
        UnitKind::King => "king",
        UnitKind::Soldier => "soldier",
        UnitKind::Animal => "animal",
        UnitKind::Monster => "monster",
    }
}

fn biome_index(biome: Biome) -> usize {
    Biome::ALL.iter().position(|b| *b == biome).unwrap_or(0)
}

/// The six offset neighbours of an odd-r tile: `(odd_row, even_row)` differ on the
/// diagonals, which is the whole point of the layout.
fn neighbours(col: i32, row: i32) -> [(i32, i32); 6] {
    if row & 1 == 0 {
        [
            (col + 1, row),
            (col - 1, row),
            (col, row - 1),
            (col - 1, row - 1),
            (col, row + 1),
            (col - 1, row + 1),
        ]
    } else {
        [
            (col + 1, row),
            (col - 1, row),
            (col, row - 1),
            (col + 1, row - 1),
            (col, row + 1),
            (col + 1, row + 1),
        ]
    }
}

/// Everything the server needs, and nothing it does not. One thread owns it, so
/// there are no locks anywhere: a request mutates the world in place, then the
/// loop steps it.
pub struct Live {
    pub world: World,
    pub tps: f32,
    pub paused: bool,
    pub frames: u64,
    next_tick: Instant,
    last_scan: Instant,
    biome: String,
    elev: String,
    owner: String,
    terrain_rev: u64,
    territory_rev: u64,
}

impl Live {
    pub fn new(world: World, tps: f32) -> Live {
        let mut live = Live {
            world,
            tps: tps.clamp(0.5, 120.0),
            paused: false,
            frames: 0,
            next_tick: Instant::now(),
            last_scan: Instant::now() - SCAN * 2,
            biome: String::new(),
            elev: String::new(),
            owner: String::new(),
            terrain_rev: 0,
            territory_rev: 0,
        };
        live.scan(true);
        live
    }

    /// Rebuild the heavy layers when they are stale, and keep the revision numbers
    /// a client uses to know whether its copy of the map is still good.
    pub fn scan(&mut self, force: bool) {
        let now = Instant::now();
        if !force && now.duration_since(self.last_scan) < SCAN {
            return;
        }
        self.last_scan = now;
        let biome = self.biome_chars();
        let elev = self.elev_chars();
        if biome != self.biome || elev != self.elev {
            self.biome = biome;
            self.elev = elev;
            self.terrain_rev += 1;
        }
        let owner = self.owner_chars();
        if owner != self.owner {
            self.owner = owner;
            self.territory_rev += 1;
        }
    }

    /// Step if the clock says so. Called from the serve loop, so the simulation
    /// only advances between requests — a slow client slows the world down, it does
    /// not queue frames.
    pub fn tick(&mut self, now: Instant) {
        if self.paused || now < self.next_tick {
            return;
        }
        self.world.step();
        self.frames += 1;
        let step = Duration::from_secs_f32(1.0 / self.tps.max(0.5));
        self.next_tick = now + step;
        if self.next_tick < now {
            self.next_tick = now;
        }
    }

    /// The terrain layer: one base36 biome index and one quantised elevation digit
    /// per tile, plus the palette a client needs to colour them. ~19 KB on a large
    /// map, which is why it has its own endpoint and a revision number.
    pub fn terrain_json(&self) -> String {
        let palette: Vec<String> = Biome::ALL
            .iter()
            .map(|b| format!("\"{}\"", hex6(b.color())))
            .collect();
        let names: Vec<String> = Biome::ALL
            .iter()
            .map(|b| format!("\"{}\"", esc(b.name())))
            .collect();
        format!(
            "{{\"rev\":{},\"w\":{},\"h\":{},\"biome\":\"{}\",\"elev\":\"{}\",\"palette\":[{}],\"names\":[{}]}}",
            self.terrain_rev,
            self.world.width,
            self.world.height,
            self.biome,
            self.elev,
            palette.join(","),
            names.join(",")
        )
    }

    fn biome_chars(&self) -> String {
        let mut out = String::with_capacity(self.world.tiles.len());
        for tile in &self.world.tiles {
            out.push(base36(biome_index(tile.biome)));
        }
        out
    }

    fn elev_chars(&self) -> String {
        let mut out = String::with_capacity(self.world.tiles.len());
        for tile in &self.world.tiles {
            let v = ((tile.elevation as i32 + 300).clamp(0, 1200) * 35 / 1200) as usize;
            out.push(base36(v));
        }
        out
    }

    /// The territory layer: which kingdom owns each tile. `"."` is nobody, a
    /// base36 digit is kingdom index + 1.
    pub fn territory_json(&self) -> String {
        format!("{{\"rev\":{},\"owner\":\"{}\"}}", self.territory_rev, self.owner)
    }

    fn owner_chars(&self) -> String {
        let faction = self.village_factions();
        let mut out = String::with_capacity(self.world.tiles.len());
        for tile in &self.world.tiles {
            let f = tile
                .owner
                .and_then(|id| faction.get(id as usize).copied())
                .unwrap_or(-1);
            if f < 0 {
                out.push('.');
            } else {
                out.push(FACTIONS[f as usize % FACTIONS.len()] as char);
            }
        }
        out
    }

    /// Which faction a village's land belongs to: its kingdom if it has one, else a
    /// stand-in of its own so a young, kingless village still paints the map in its
    /// race's colour. `pseudo` indexes [`Live::factions`].
    fn village_factions(&self) -> Vec<i32> {
        let base = self.world.kingdoms.len();
        let mut pseudo = 0usize;
        let mut out = Vec::with_capacity(self.world.villages.len());
        for v in &self.world.villages {
            if !v.alive {
                out.push(-1);
                continue;
            }
            match v.kingdom {
                Some(k) => out.push(k as i32),
                None => {
                    out.push((base + pseudo) as i32);
                    pseudo += 1;
                }
            }
        }
        out
    }

    /// The stand-in factions, in the order their indices appear: `(name, colour)`.
    pub fn factions(&self) -> Vec<(String, String)> {
        self.world
            .villages
            .iter()
            .filter(|v| v.alive && v.kingdom.is_none())
            .map(|v| (v.name.clone(), hex6(v.race.def().color)))
            .collect()
    }

    /// One frame of everything that moves. Small on purpose: the map layers are
    /// separate endpoints, so a client can poll this several times a second.
    pub fn state_json(&self) -> String {
        let world = &self.world;
        let mut out = String::with_capacity(8192);
        out.push_str(&format!(
            "{{\"tick\":{},\"year\":{},\"frames\":{},\"tps\":{},\"paused\":{},\"hash\":\"0x{:016x}\",\"w\":{},\"h\":{},\"terrain_rev\":{},\"territory_rev\":{}",
            world.tick,
            world.year,
            self.frames,
            self.tps,
            self.paused,
            world.state_hash(),
            world.width,
            world.height,
            self.terrain_rev,
            self.territory_rev,
        ));

        out.push_str(&format!(
            ",\"age\":[\"{}\",{},{}]",
            esc(world.age.age.name()),
            world.age.years_in_age,
            world.age.duration_years
        ));

        let races = world.population_by_race();
        out.push_str(",\"races\":[");
        for (i, (race, n)) in races.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            out.push_str(&format!("[\"{}\",{}]", esc(race.def().name), n));
        }
        out.push(']');

        let s = &world.stats;
        out.push_str(&format!(
            ",\"stats\":{{\"spawned\":{},\"births\":{},\"deaths\":{},\"kills\":{},\"villages\":{},\"kingdoms\":{},\"wars\":{},\"peace\":{},\"captures\":{},\"fallen\":{},\"rebellions\":{},\"buildings\":{},\"disasters\":{},\"nukes\":{},\"powers\":{},\"ages\":{}}}",
            s.units_spawned, s.births, s.deaths, s.kills, s.villages_founded, s.kingdoms_founded,
            s.wars_declared, s.wars_ended, s.cities_captured, s.kingdoms_fallen, s.rebellions,
            s.buildings_built, s.disasters, s.nukes_dropped, s.powers_cast, s.age_changes
        ));

        let wars = war_pairs(world);
        out.push_str(",\"kingdoms\":[");
        for (i, k) in world.kingdoms.iter().filter(|k| k.alive).enumerate() {
            if i > 0 {
                out.push(',');
            }
            let enemies = k
                .wars
                .iter()
                .filter(|w| world.kingdom(w.enemy).is_some())
                .count();
            out.push_str(&format!(
                "[{},\"{}\",\"{}\",\"{}\",{},{},{},{},\"{}\",\"{}\",{}]",
                k.id,
                esc(&k.name),
                hex6(k.color),
                esc(k.race.def().name),
                k.population,
                k.cities.len(),
                enemies,
                k.king.map(|v| v as i64).unwrap_or(-1),
                esc(&k.king_name),
                esc(&crate::kingdom::culture::names(k.culture).join(", ")),
                k.allies.len()
            ));
        }
        out.push(']');

        out.push_str(",\"villages\":[");
        for (i, v) in world.villages.iter().filter(|v| v.alive).enumerate() {
            if i > 0 {
                out.push(',');
            }
            let (col, row) = v.center.to_offset();
            out.push_str(&format!(
                "[{},\"{}\",\"{}\",{},{},{},{},{},{},{},{},{},{}]",
                v.id,
                esc(&v.name),
                esc(v.race.def().name),
                v.kingdom.map(|k| k as i64).unwrap_or(-1),
                col,
                row,
                v.pop,
                v.soldiers,
                v.walls,
                v.capital as u8,
                v.siege,
                v.besieged_by.map(|k| k as i64).unwrap_or(-1),
                v.loyalty
            ));
        }
        out.push(']');

        out.push_str(",\"units\":[");
        for (units, u) in world
            .units
            .iter()
            .filter(|u| u.alive)
            .take(MAX_UNITS)
            .enumerate()
        {
            let (col, row) = u.pos.to_offset();
            if units > 0 {
                out.push(',');
            }
            let hp = (u.hp.max(0) * 100 / u.max_hp.max(1)).clamp(0, 100);
            out.push_str(&format!(
                "[{},{},{},{},{},{},\"{}\"]",
                col,
                row,
                race_index(u.race),
                kind_index(u.kind),
                hp,
                u.kingdom.map(|k| k as i64).unwrap_or(-1),
                esc(&u.name)
            ));
        }
        out.push(']');

        out.push_str(",\"wars\":[");
        for (i, (a, b, year)) in wars.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            out.push_str(&format!("[{a},{b},{year}]"));
        }
        out.push(']');

        let fronts = self.fronts(&wars);
        push_marks(&mut out, "fronts", &fronts);
        let clashes = self.clashes(&wars);
        push_marks(&mut out, "clashes", &clashes);

        out.push_str(",\"news\":[");
        for (i, e) in world.chronicle.iter().rev().take(MAX_NEWS).enumerate() {
            if i > 0 {
                out.push(',');
            }
            out.push_str(&format!(
                "[{},{},\"{}\",\"{}\"]",
                e.year,
                e.tick,
                esc(e.kind.name()),
                esc(&e.text)
            ));
        }
        out.push(']');

        out.push_str(&format!(",\"faction_base\":{}", world.kingdoms.len()));
        out.push_str(",\"factions\":[");
        for (i, (name, colour)) in self.factions().iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            out.push_str(&format!("[\"{}\",\"{}\"]", esc(name), colour));
        }
        out.push(']');

        out.push_str(",\"kinds\":[");
        for (i, k) in [
            UnitKind::Civilian,
            UnitKind::Leader,
            UnitKind::King,
            UnitKind::Soldier,
            UnitKind::Animal,
            UnitKind::Monster,
        ]
        .iter()
        .enumerate()
        {
            if i > 0 {
                out.push(',');
            }
            out.push_str(&format!("\"{}\"", kind_name(*k)));
        }
        out.push(']');

        out.push_str(",\"race_colors\":[");
        for (i, r) in Race::ALL.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            out.push_str(&format!("\"{}\"", hex6(r.def().color)));
        }
        out.push(']');

        out.push_str(",\"species\":[");
        for (i, r) in Race::ALL.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            let def = r.def();
            let category = match def.category {
                Category::Civilized => "civilized",
                Category::Animal => "animal",
                Category::Monster => "monster",
                Category::Boss => "boss",
            };
            out.push_str(&format!("[\"{}\",\"{category}\"]", esc(def.name)));
        }
        out.push(']');

        out.push_str(",\"powers\":[");
        for (i, p) in Power::ALL.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            out.push_str(&format!("[\"{}\",\"{}\"]", esc(p.name()), esc(p.category().name())));
        }
        out.push(']');

        out.push('}');
        out
    }

    /// Tiles where two kingdoms that are at war share a border: the front line.
    fn fronts(&self, wars: &[(u32, u32, u32)]) -> Vec<(i32, i32)> {
        if wars.is_empty() {
            return Vec::new();
        }
        let owner = self.owner_grid();
        let mut out = Vec::new();
        let w = self.world.width as i32;
        let h = self.world.height as i32;
        for row in 0..h {
            for col in 0..w {
                let k = owner[(row * w + col) as usize];
                if k < 0 {
                    continue;
                }
                for (nc, nr) in neighbours(col, row) {
                    if nc < 0 || nr < 0 || nc >= w || nr >= h {
                        continue;
                    }
                    let k2 = owner[(nr * w + nc) as usize];
                    if k2 >= 0 && k2 != k && at_war(wars, k as u32, k2 as u32) {
                        out.push((col, row));
                        break;
                    }
                }
                if out.len() >= MAX_MARKS {
                    break;
                }
            }
        }
        out
    }

    /// Tiles where units of two sides that are fighting stand next to each other:
    /// at-war kingdoms, or a monster beside anyone. This is what a siege looks
    /// like from above, a tick at a time.
    fn clashes(&self, wars: &[(u32, u32, u32)]) -> Vec<(i32, i32)> {
        let mut pts: Vec<(i32, i32, i32, bool, bool)> = Vec::new(); // col,row,kingdom,monster,animal
        for u in self.world.units.iter().filter(|u| u.alive) {
            let (col, row) = u.pos.to_offset();
            let kingdom = u.kingdom.map(|k| k as i32).unwrap_or(-1);
            let monster = matches!(u.kind, UnitKind::Monster);
            let animal = matches!(u.kind, UnitKind::Animal);
            pts.push((col, row, kingdom, monster, animal));
        }
        let mut out: Vec<(i32, i32)> = Vec::new();
        for i in 0..pts.len() {
            for j in i + 1..pts.len() {
                let (c1, r1, k1, m1, a1) = pts[i];
                let (c2, r2, k2, m2, a2) = pts[j];
                if (c1 - c2).abs() > 1 || (r1 - r2).abs() > 1 {
                    continue;
                }
                let fight = (m1 && !a2) || (m2 && !a1) || at_war(wars, k1 as u32, k2 as u32);
                if fight {
                    if !out.contains(&(c1, r1)) {
                        out.push((c1, r1));
                    }
                    if !out.contains(&(c2, r2)) {
                        out.push((c2, r2));
                    }
                }
                if out.len() >= MAX_MARKS {
                    return out;
                }
            }
        }
        out
    }

    /// Kingdom per tile (`-1` for none), row-major in offset order.
    fn owner_grid(&self) -> Vec<i32> {
        self.owner
            .bytes()
            .map(|b| {
                if b == b'.' {
                    -1
                } else {
                    // a faction digit, value - 1
                    FACTIONS
                        .iter()
                        .position(|f| *f == b)
                        .map(|i| i as i32)
                        .unwrap_or(-1)
                }
            })
            .collect()
    }

    /// Apply a command from the page. Everything the page can do, the CLI can too;
    /// the query string is deliberately trivial (`power=12&col=5&row=9`).
    pub fn apply(&mut self, query: &str) -> String {
        let mut message = String::new();
        let mut touched_map = false;
        if q(query, "resume").is_some() {
            self.paused = false;
            message = "running".to_string();
        }
        if q(query, "pause").is_some() {
            self.paused = true;
            message = "paused".to_string();
        }
        if let Some(n) = q(query, "tps").and_then(|v| v.parse::<f32>().ok()) {
            self.tps = n.clamp(0.5, 120.0);
            if self.tps > 100.0 {
                message = format!("speed {:.0} ticks/s — hold tight", self.tps);
            } else {
                message = format!("speed {:.1} ticks/s", self.tps);
            }
        }
        if let Some(n) = q(query, "step").and_then(|v| v.parse::<u64>().ok()) {
            let n = n.min(5000);
            self.paused = true;
            self.world.step_n(n);
            message = format!("stepped {n} ticks");
        }
        if let Some(i) = q(query, "power").and_then(|v| v.parse::<usize>().ok()) {
            if let (Some(col), Some(row)) = (
                q(query, "col").and_then(|v| v.parse::<i32>().ok()),
                q(query, "row").and_then(|v| v.parse::<i32>().ok()),
            ) {
                if let Some(power) = Power::ALL.get(i) {
                    let outcome = self.world.cast(*power, Hex::from_offset(col, row));
                    message = outcome.message;
                    touched_map = true;
                }
            }
        }
        if let Some(i) = q(query, "spawn").and_then(|v| v.parse::<usize>().ok()) {
            if let (Some(col), Some(row)) = (
                q(query, "col").and_then(|v| v.parse::<i32>().ok()),
                q(query, "row").and_then(|v| v.parse::<i32>().ok()),
            ) {
                if let Some(race) = Race::ALL.get(i) {
                    let kind = match race.def().category {
                        Category::Civilized => UnitKind::Civilian,
                        Category::Animal => UnitKind::Animal,
                        Category::Monster | Category::Boss => UnitKind::Monster,
                    };
                    let _ = self.world.spawn_unit(*race, Hex::from_offset(col, row), kind);
                    message = format!("{} placed at {col},{row}", race.def().name);
                    touched_map = true;
                }
            }
        }
        if touched_map {
            self.scan(true);
        }
        format!("{{\"ok\":true,\"msg\":\"{}\"}}", esc(&message))
    }
}

fn race_index(race: Race) -> usize {
    Race::ALL.iter().position(|r| *r == race).unwrap_or(0)
}

fn push_marks(out: &mut String, name: &str, marks: &[(i32, i32)]) {
    out.push_str(&format!(",\"{name}\":["));
    for (i, (col, row)) in marks.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&format!("[{col},{row}]"));
    }
    out.push(']');
}

/// Every war in the world once, as `(a, b, year it began)` with `a < b`.
pub fn war_pairs(world: &World) -> Vec<(u32, u32, u32)> {
    let mut out = Vec::new();
    for k in world.kingdoms.iter().filter(|k| k.alive) {
        for war in &k.wars {
            if world.kingdom(war.enemy).is_none() {
                continue;
            }
            let (a, b) = if k.id < war.enemy {
                (k.id, war.enemy)
            } else {
                (war.enemy, k.id)
            };
            if !out.iter().any(|(x, y, _)| *x == a && *y == b) {
                out.push((a, b, war.start_year));
            }
        }
    }
    out
}

/// Are these two kingdoms at war right now?
pub fn at_war(wars: &[(u32, u32, u32)], a: u32, b: u32) -> bool {
    if a == b {
        return false;
    }
    let (lo, hi) = if a < b { (a, b) } else { (b, a) };
    wars.iter().any(|(x, y, _)| *x == lo && *y == hi)
}

/// One `key=value` out of a query string.
fn q<'a>(query: &'a str, key: &str) -> Option<&'a str> {
    for pair in query.split('&') {
        if let Some((k, v)) = pair.split_once('=') {
            if k == key {
                return Some(v);
            }
        }
    }
    None
}

fn respond(stream: &TcpStream, code: u16, ctype: &str, body: &str) {
    let reason = match code {
        200 => "OK",
        204 => "No Content",
        404 => "Not Found",
        405 => "Method Not Allowed",
        _ => "OK",
    };
    let head = format!(
        "HTTP/1.1 {code} {reason}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let mut stream = stream;
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(body.as_bytes());
    let _ = stream.flush();
}

fn handle(mut stream: TcpStream, live: &mut Live) {
    let _ = stream.set_read_timeout(Some(Duration::from_millis(1500)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(10)));
    let mut buf = [0u8; 4096];
    let n = match stream.read(&mut buf) {
        Ok(0) => return,
        Ok(n) => n,
        Err(_) => return,
    };
    let request = String::from_utf8_lossy(&buf[..n]);
    let line = request.lines().next().unwrap_or("");
    let mut parts = line.split(' ');
    let method = parts.next().unwrap_or("");
    let target = parts.next().unwrap_or("/");
    if method != "GET" {
        return respond(&stream, 405, "text/plain; charset=utf-8", "GET only");
    }
    let (path, query) = match target.split_once('?') {
        Some((p, qs)) => (p, qs),
        None => (target, ""),
    };
    match path {
        "/" | "/index.html" => respond(&stream, 200, "text/html; charset=utf-8", PAGE),
        "/state" => {
            let body = live.state_json();
            respond(&stream, 200, "application/json; charset=utf-8", &body);
        }
        "/terrain" => {
            live.scan(false);
            let body = live.terrain_json();
            respond(&stream, 200, "application/json; charset=utf-8", &body);
        }
        "/territory" => {
            live.scan(false);
            let body = live.territory_json();
            respond(&stream, 200, "application/json; charset=utf-8", &body);
        }
        "/cmd" => {
            let body = live.apply(query);
            respond(&stream, 200, "application/json; charset=utf-8", &body);
        }
        "/favicon.ico" => respond(&stream, 204, "text/plain; charset=utf-8", ""),
        _ => respond(&stream, 404, "text/plain; charset=utf-8", "not found"),
    }
}

/// A bound server, before it starts looping. Split out so tests can talk to it
/// over a real socket without a real browser.
pub struct Watcher {
    listener: TcpListener,
    live: Live,
}

impl Watcher {
    pub fn bind(host: &str, port: u16, world: World, tps: f32) -> Result<Watcher, String> {
        let listener =
            TcpListener::bind((host, port)).map_err(|e| format!("bind {host}:{port}: {e}"))?;
        listener
            .set_nonblocking(true)
            .map_err(|e| format!("nonblocking: {e}"))?;
        Ok(Watcher {
            listener,
            live: Live::new(world, tps),
        })
    }

    /// The port actually in use — real, even when `bind` was given port 0.
    pub fn port(&self) -> u16 {
        self.listener
            .local_addr()
            .map(|a| a.port())
            .unwrap_or(0)
    }

    pub fn live(&self) -> &Live {
        &self.live
    }

    pub fn live_mut(&mut self) -> &mut Live {
        &mut self.live
    }

    /// Answer every request waiting, without blocking.
    pub fn drain(&mut self) -> usize {
        let mut served = 0;
        loop {
            match self.listener.accept() {
                Ok((stream, _)) => {
                    handle(stream, &mut self.live);
                    served += 1;
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(_) => break,
            }
        }
        served
    }

    /// Serve and step until `stop` is set. One loop owns the world: requests are
    /// answered between ticks, never during one.
    pub fn serve(&mut self, stop: &AtomicBool) {
        while !stop.load(Ordering::Relaxed) {
            self.drain();
            self.live.scan(false);
            self.live.tick(Instant::now());
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}

/// Run a world and serve the live page until the process is killed.
pub fn watch(world: World, options: &WatchOptions) -> Result<(), String> {
    let mut watcher = Watcher::bind(&options.host, options.port, world, options.tps)?;
    let port = watcher.port();
    println!("worldforge watch: http://{}:{port}/", options.host);
    println!(
        "  {}x{} world, {} kingdoms, {} villages — {:.0} ticks/s, live in the page",
        watcher.live().world.width,
        watcher.live().world.height,
        watcher.live().world.kingdoms.iter().filter(|k| k.alive).count(),
        watcher.live().world.villages.iter().filter(|v| v.alive).count(),
        watcher.live().tps
    );
    println!("  press Ctrl-C to stop");
    let stop = AtomicBool::new(false);
    watcher.serve(&stop);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json;
    use crate::worldgen::GenParams;

    fn world() -> World {
        let params = GenParams {
            width: 48,
            height: 32,
            seed: 20241003,
            ..Default::default()
        };
        let mut world = World::generate(params);
        world.seed_life(4, 8, 0);
        world.step_n(120);
        world
    }

    /// A world with two crowned kingdoms and a war between them, built directly:
    /// waiting for diplomacy to get there would make the test slow and flaky.
    fn world_at_war() -> World {
        let mut world = world();
        let villages: Vec<u32> = world
            .villages
            .iter()
            .filter(|v| v.alive)
            .map(|v| v.id)
            .take(2)
            .collect();
        assert_eq!(villages.len(), 2, "the fixture needs two villages");
        let a = world
            .found_kingdom(villages[0], world.villages[villages[0] as usize].race)
            .expect("the first village crowns a king");
        let b = world
            .found_kingdom(villages[1], world.villages[villages[1] as usize].race)
            .expect("the second village crowns a king");
        world.declare_war(a, b);
        world.step_n(30);
        world
    }

    #[test]
    fn escaping_cannot_break_a_frame() {
        assert_eq!(esc("a\"b\\c\nd"), "a\\\"b\\\\c\\nd");
        assert_eq!(esc("plain"), "plain");
    }

    #[test]
    fn the_state_frame_is_valid_json_with_the_parts_the_page_needs() {
        let live = Live::new(world(), 20.0);
        let text = live.state_json();
        let parsed = json::parse(&text).expect("the page would choke on this");
        for key in [
            "tick",
            "year",
            "hash",
            "age",
            "races",
            "stats",
            "kingdoms",
            "villages",
            "units",
            "wars",
            "factions",
            "faction_base",
            "fronts",
            "clashes",
            "news",
            "kinds",
            "race_colors",
            "species",
            "powers",
        ] {
            assert!(parsed.get(key).is_some(), "state is missing {key}");
        }
        assert!(parsed.get("tick").unwrap().as_i64().unwrap() >= 100);
        assert_eq!(parsed.get("species").unwrap().arr_len(), Race::ALL.len());
        assert_eq!(parsed.get("powers").unwrap().arr_len(), Power::ALL.len());
        assert_eq!(parsed.get("kinds").unwrap().arr_len(), 6);
        // every village entry is a full row: the table reads these by index
        let villages = parsed.get("villages").unwrap().as_arr().unwrap();
        assert!(!villages.is_empty(), "a settled world has villages");
        assert_eq!(villages[0].arr_len(), 13);
    }

    #[test]
    fn map_layers_cover_every_tile_and_are_valid_json() {
        let live = Live::new(world(), 20.0);
        let tiles = (live.world.width as usize) * (live.world.height as usize);
        let terrain = json::parse(&live.terrain_json()).unwrap();
        assert_eq!(terrain.get("biome").unwrap().as_str().unwrap().len(), tiles);
        assert_eq!(terrain.get("elev").unwrap().as_str().unwrap().len(), tiles);
        assert_eq!(terrain.get("palette").unwrap().arr_len(), Biome::ALL.len());
        assert_eq!(terrain.get("names").unwrap().arr_len(), Biome::ALL.len());
        assert!(terrain.get("rev").unwrap().as_i64().unwrap() >= 1);

        let territory = json::parse(&live.territory_json()).unwrap();
        assert_eq!(territory.get("owner").unwrap().as_str().unwrap().len(), tiles);
    }

    #[test]
    fn the_owner_layer_knows_which_faction_holds_what() {
        let live = Live::new(world(), 20.0);
        let owner = live.owner_grid();
        let claimed = owner.iter().filter(|f| **f >= 0).count();
        assert!(claimed > 0, "settled villages should own land");
        assert!(claimed < owner.len(), "and not all of it");
        // a claim is either an alive kingdom or a kingless village's stand-in
        let base = live.world.kingdoms.len();
        let pseudo = live.factions().len();
        for f in owner.iter().filter(|f| **f >= 0) {
            let f = *f as usize;
            if f < base {
                assert!(live.world.kingdom(f as u32).is_some(), "kingdom {f} is gone");
            } else {
                assert!(f - base < pseudo, "faction {f} has no village behind it");
            }
        }
        // and the layer is exactly as long as the map
        let chars = live.territory_json();
        let owner_str = json::parse(&chars).unwrap().get("owner").unwrap().as_str().unwrap().to_string();
        assert_eq!(owner_str.len(), live.world.tiles.len());
    }

    #[test]
    fn commands_pause_step_cast_and_place() {
        let mut live = Live::new(world(), 20.0);
        let reply = json::parse(&live.apply("pause=1")).unwrap();
        assert!(reply.get("ok").unwrap().as_bool().unwrap());
        assert!(live.paused);
        let before = live.world.tick;
        live.apply("step=30");
        assert_eq!(live.world.tick, before + 30, "step should move the clock");
        assert!(live.paused, "stepping by hand means paused");
        live.apply("resume=1&tps=45");
        assert!(!live.paused);
        assert!((live.tps - 45.0).abs() < 0.01);

        let units = live.world.units.iter().filter(|u| u.alive).count();
        let msg = live.apply("spawn=0&col=5&row=5");
        assert!(msg.contains("ok"), "{msg}");
        assert!(live.world.units.iter().filter(|u| u.alive).count() >= units);

        let reply = live.apply("power=0&col=10&row=10");
        assert!(json::parse(&reply).unwrap().get("ok").unwrap().as_bool().unwrap());
        // a power aimed off the map must not panic
        live.apply("power=0&col=9999&row=9999");
        live.apply("power=999&col=1&row=1");
        live.apply("spawn=999&col=1&row=1");
        live.apply("nonsense=1");
    }

    #[test]
    fn the_front_line_only_appears_where_enemies_meet() {
        let live = Live::new(world_at_war(), 20.0);
        let wars = war_pairs(&live.world);
        assert!(!wars.is_empty(), "120 ticks of a settled world should have wars");
        assert!(wars.iter().all(|(a, b, _)| a != b));
        assert!(!at_war(&wars, 0, 0), "a kingdom cannot be at war with itself");
        for (a, b, _) in &wars {
            assert!(at_war(&wars, *a, *b) && at_war(&wars, *b, *a));
        }
        let fronts = live.fronts(&wars);
        for (col, row) in fronts.iter().take(20) {
            let grid = live.owner_grid();
            let here = grid[(row * live.world.width as i32 + col) as usize];
            assert!(here >= 0, "a front tile is owned by someone");
            assert!(
                neighbours(*col, *row)
                    .iter()
                    .filter(|(c, r)| {
                        *c >= 0
                            && *r >= 0
                            && *c < live.world.width as i32
                            && *r < live.world.height as i32
                    })
                    .any(|(c, r)| {
                        let there = grid[(r * live.world.width as i32 + c) as usize];
                        there >= 0 && there != here && at_war(&wars, here as u32, there as u32)
                    }),
                "a front tile has an enemy next door"
            );
        }
    }

    #[test]
    fn scanning_only_bumps_the_revision_when_the_map_changes() {
        let mut live = Live::new(world(), 20.0);
        let rev = live.terrain_rev;
        let owner_rev = live.territory_rev;
        live.scan(true);
        assert_eq!(live.terrain_rev, rev, "an unchanged map is not a new revision");
        // a nuke at the busiest village: terrain and lives both change
        let target = live
            .world
            .villages
            .iter()
            .filter(|v| v.alive)
            .max_by_key(|v| v.pop)
            .map(|v| v.center.to_offset())
            .expect("a village to aim at");
        let nuke = Power::ALL
            .iter()
            .position(|p| p.name() == "nuke")
            .expect("a nuke in the toolbox");
        let hash = live.world.state_hash();
        live.apply(&format!("power={nuke}&col={}&row={}", target.0, target.1));
        assert_ne!(live.world.state_hash(), hash, "the nuke did nothing at all");
        live.scan(true);
        assert!(
            live.terrain_rev > rev || live.territory_rev > owner_rev,
            "the map changed, so a layer revision should have too"
        );
    }
}
