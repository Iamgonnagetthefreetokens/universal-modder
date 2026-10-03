//! A* pathfinding over the hex grid.
//!
//! The grid is abstracted behind [`PathGrid`] so the search can be unit-tested
//! against a synthetic map (and against a BFS reference) without building a whole
//! world. Costs are integers and ties are broken by hex coordinate, which keeps
//! the search — and therefore the whole simulation — deterministic.

use crate::hex::Hex;
use crate::terrain::{Biome, Tile};
use crate::world::World;
use std::cmp::Ordering;
use std::collections::{BinaryHeap, VecDeque};

/// How a unit travels, which decides what it may cross.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PathMode {
    /// Dry land only. Mountains need the mountain-walker trait.
    Land,
    /// Land and water (swimmers, or units in boats).
    Amphibious,
    /// Water first: a boat route. Land is allowed but expensive so a route only
    /// lands at its destination.
    Sail,
    /// Over anything.
    Fly,
}

/// Anything the A* search can walk on.
pub trait PathGrid {
    fn in_bounds(&self, h: Hex) -> bool;
    /// Cost to *enter* `h`; `None` means impassable.
    fn cost(&self, h: Hex) -> Option<i32>;
    fn index(&self, h: Hex) -> usize;
    fn hex_at(&self, index: usize) -> Hex;
    fn len(&self) -> usize;
    /// True when the grid has no cells (maps are never empty in practice).
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
    /// Search budget; the search gives up after expanding this many nodes.
    fn budget(&self) -> usize {
        20_000
    }
}

/// Terrain cost table, shared by the search and by units.
pub fn terrain_cost_for(tile: &Tile, sailing: bool) -> i32 {
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
    if tile.river && !sailing {
        cost = cost * 3 / 2;
    }
    cost.max(10)
}

/// Tiles a land traveller cannot cross without a special trait.
fn land_blocked(tile: &Tile) -> bool {
    tile.is_water() || tile.is_mountain() || tile.has_lava()
}

struct GridView<'a> {
    world: &'a World,
    mode: PathMode,
}

impl PathGrid for GridView<'_> {
    fn in_bounds(&self, h: Hex) -> bool {
        self.world.in_bounds(h)
    }

    fn cost(&self, h: Hex) -> Option<i32> {
        let t = self.world.tile(h)?;
        match self.mode {
            // Flying ignores terrain.
            PathMode::Fly => Some(30),
            PathMode::Sail => {
                if t.is_sea() {
                    Some(60)
                } else if t.is_land() {
                    // Beaching is allowed so a boat can reach a coastal village,
                    // but it is expensive enough that routes stay at sea.
                    Some(260)
                } else {
                    None
                }
            }
            PathMode::Amphibious => {
                if t.is_sea() {
                    Some(60)
                } else if land_blocked(t) {
                    None
                } else {
                    Some(terrain_cost_for(t, true))
                }
            }
            PathMode::Land => {
                if land_blocked(t) {
                    None
                } else {
                    Some(terrain_cost_for(t, false))
                }
            }
        }
    }

    fn index(&self, h: Hex) -> usize {
        self.world.idx(h).unwrap_or(0)
    }

    fn hex_at(&self, index: usize) -> Hex {
        let w = self.world.width as usize;
        if w == 0 {
            return Hex::new(0, 0);
        }
        Hex::from_offset((index % w) as i32, (index / w) as i32)
    }

    fn len(&self) -> usize {
        self.world.width as usize * self.world.height as usize
    }
}

/// Hex-distance heuristic. Never overestimates: the cheapest step costs 10.
fn heuristic(a: Hex, b: Hex) -> i32 {
    a.distance(b) * 10
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Node {
    f: i32,
    h: i32,
    hex: Hex,
}

impl Ord for Node {
    fn cmp(&self, other: &Self) -> Ordering {
        // BinaryHeap is a max-heap; reverse the comparison so the smallest f wins.
        // The (h, hex) tie-break keeps the result independent of heap internals.
        other
            .f
            .cmp(&self.f)
            .then(other.h.cmp(&self.h))
            .then(other.hex.cmp(&self.hex))
    }
}

impl PartialOrd for Node {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// A* search. Returns the path *including* the start hex, or `None` when the goal
/// is unreachable (or the search ran out of budget).
pub fn astar<G: PathGrid>(grid: &G, start: Hex, goal: Hex) -> Option<Vec<Hex>> {
    if start == goal {
        return Some(vec![start]);
    }
    if !grid.in_bounds(start) || !grid.in_bounds(goal) {
        return None;
    }
    grid.cost(goal)?;
    let n = grid.len();
    let mut g: Vec<i32> = vec![i32::MAX; n];
    let mut came: Vec<u32> = vec![u32::MAX; n];
    let mut closed: Vec<bool> = vec![false; n];
    let mut open: BinaryHeap<Node> = BinaryHeap::new();

    let si = grid.index(start);
    let gi = grid.index(goal);
    if si >= n || gi >= n {
        return None;
    }
    g[si] = 0;
    let h0 = heuristic(start, goal);
    open.push(Node {
        f: h0,
        h: h0,
        hex: start,
    });
    let mut expanded = 0usize;

    while let Some(node) = open.pop() {
        let cur = node.hex;
        let ci = grid.index(cur);
        if closed[ci] {
            continue;
        }
        closed[ci] = true;
        if cur == goal {
            let mut path = vec![cur];
            let mut idx = ci;
            while idx != si {
                let prev = came[idx];
                if prev == u32::MAX {
                    return None;
                }
                let prev_hex = grid.hex_at(prev as usize);
                path.push(prev_hex);
                idx = prev as usize;
                if path.len() > n {
                    return None; // defensive: never loop forever
                }
            }
            path.reverse();
            return Some(path);
        }
        expanded += 1;
        if expanded > grid.budget() {
            return None;
        }
        for nb in cur.neighbors() {
            if !grid.in_bounds(nb) {
                continue;
            }
            let ni = grid.index(nb);
            if ni >= n || closed[ni] {
                continue;
            }
            let Some(step) = grid.cost(nb) else {
                continue;
            };
            let tentative = g[ci].saturating_add(step);
            if tentative < g[ni] {
                g[ni] = tentative;
                came[ni] = ci as u32;
                let h = heuristic(nb, goal);
                open.push(Node {
                    f: tentative + h,
                    h,
                    hex: nb,
                });
            }
        }
    }
    None
}

/// Path between two hexes of a world, using the given travel mode.
pub fn find_path(world: &World, start: Hex, goal: Hex, mode: PathMode) -> Option<Vec<Hex>> {
    let grid = GridView { world, mode };
    astar(&grid, start, goal)
}

/// Breadth-first flood fill: every reachable hex with its step distance from
/// `start`. Used by the AI to find the nearest target and by tests as a
/// reference implementation for A*.
pub fn bfs_distances<G: PathGrid>(grid: &G, start: Hex, max_steps: i32) -> Vec<(Hex, i32)> {
    let mut out = Vec::new();
    if !grid.in_bounds(start) {
        return out;
    }
    let mut seen = vec![false; grid.len()];
    let mut queue: VecDeque<(Hex, i32)> = VecDeque::new();
    seen[grid.index(start)] = true;
    queue.push_back((start, 0));
    while let Some((cur, d)) = queue.pop_front() {
        out.push((cur, d));
        if d >= max_steps {
            continue;
        }
        for nb in cur.neighbors() {
            if !grid.in_bounds(nb) || grid.cost(nb).is_none() {
                continue;
            }
            let ni = grid.index(nb);
            if seen[ni] {
                continue;
            }
            seen[ni] = true;
            queue.push_back((nb, d + 1));
        }
    }
    out
}

/// Nearest hex reachable from `start` that satisfies `pred`, by BFS. Used for
/// "walk to the nearest tree / ore / build site" decisions.
pub fn nearest_matching<G: PathGrid, F: Fn(Hex) -> bool>(
    grid: &G,
    start: Hex,
    max_steps: i32,
    pred: F,
) -> Option<(Hex, i32)> {
    for (h, d) in bfs_distances(grid, start, max_steps) {
        if pred(h) {
            return Some((h, d));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// A small synthetic map: `#` is a wall, `~` is expensive water.
    struct Map {
        cells: HashMap<(i32, i32), char>,
        w: i32,
        h: i32,
    }

    impl Map {
        fn new(rows: &[&str]) -> Self {
            let mut cells = HashMap::new();
            let w = rows[0].chars().count() as i32;
            for (r, row) in rows.iter().enumerate() {
                assert_eq!(row.chars().count() as i32, w, "rows must be equally long");
                for (c, ch) in row.chars().enumerate() {
                    cells.insert((c as i32, r as i32), ch);
                }
            }
            Map {
                cells,
                w,
                h: rows.len() as i32,
            }
        }

        fn path_to_dest(&self, from: (i32, i32), to: (i32, i32)) -> Option<Vec<(i32, i32)>> {
            let p = astar(
                self,
                Hex::from_offset(from.0, from.1),
                Hex::from_offset(to.0, to.1),
            )?;
            Some(p.iter().map(|h| h.to_offset()).collect())
        }
    }

    impl PathGrid for Map {
        fn in_bounds(&self, h: Hex) -> bool {
            let (c, r) = h.to_offset();
            c >= 0 && r >= 0 && c < self.w && r < self.h
        }
        fn cost(&self, h: Hex) -> Option<i32> {
            let (c, r) = h.to_offset();
            match self.cells.get(&(c, r)) {
                Some('#') => None,
                Some('~') => Some(80),
                Some(_) => Some(10),
                None => None,
            }
        }
        fn index(&self, h: Hex) -> usize {
            let (c, r) = h.to_offset();
            (r * self.w + c) as usize
        }
        fn hex_at(&self, index: usize) -> Hex {
            Hex::from_offset((index % self.w as usize) as i32, (index / self.w as usize) as i32)
        }
        fn len(&self) -> usize {
            (self.w * self.h) as usize
        }
    }

    #[test]
    fn straight_line_path_is_shortest() {
        let map = Map::new(&["....", "....", "...."]);
        let path = map.path_to_dest((0, 0), (3, 0)).unwrap();
        assert_eq!(path.len(), 4);
        assert_eq!(path.first(), Some(&(0, 0)));
        assert_eq!(path.last(), Some(&(3, 0)));
        for pair in path.windows(2) {
            assert_eq!(
                Hex::from_offset(pair[0].0, pair[0].1).distance(Hex::from_offset(pair[1].0, pair[1].1)),
                1
            );
        }
    }

    #[test]
    fn walls_force_a_detour() {
        let map = Map::new(&["....", "###.", "...."]);
        let path = map.path_to_dest((0, 0), (0, 2)).unwrap();
        assert!(path.len() > 3, "detour should be longer than the 3-step straight line");
        assert!(path.iter().all(|(c, r)| *c >= 0 && *r >= 0 && *c < 4 && *r < 3));
    }

    #[test]
    fn walls_can_block_completely() {
        // A wall column splits the map; there is no way around it.
        let map = Map::new(&[".#.", ".#.", ".#."]);
        assert!(map.path_to_dest((0, 0), (2, 0)).is_none());
        // With a gap, the same trip succeeds.
        let gap = Map::new(&[".#.", ".#.", "..."]);
        assert!(gap.path_to_dest((0, 0), (2, 0)).is_some());
    }

    #[test]
    fn astar_agrees_with_bfs_step_counts() {
        let map = Map::new(&[".....", "..#..", ".....", "..#..", "....."]);
        let start = Hex::from_offset(0, 0);
        let goal = Hex::from_offset(4, 4);
        let path = astar(&map, start, goal).unwrap();
        let bfs = bfs_distances(&map, start, 40);
        let bfs_dist = bfs
            .iter()
            .find(|(h, _)| *h == goal)
            .map(|(_, d)| *d)
            .unwrap();
        // BFS counts steps; A* returns hexes. Same number of moves.
        assert_eq!(path.len() as i32 - 1, bfs_dist);
    }

    #[test]
    fn expensive_water_is_avoided_when_a_land_route_exists() {
        // Two routes from (0,0) to (4,0): straight over two water tiles, or a
        // detour over land.
        let map = Map::new(&["..~~.", ".....", "....."]);
        let path = map.path_to_dest((0, 0), (4, 0)).unwrap();
        let water_steps = path
            .iter()
            .filter(|(c, r)| matches!(map.cells.get(&(*c, *r)), Some('~')))
            .count();
        assert_eq!(water_steps, 0, "A* should route around the water: {path:?}");
    }

    #[test]
    fn nearest_matching_finds_the_closest_target() {
        let map = Map::new(&["....", "...#", "...."]);
        let start = Hex::from_offset(0, 0);
        let hit = nearest_matching(&map, start, 10, |h| h.to_offset() == (3, 0));
        assert_eq!(hit.map(|(h, d)| (h.to_offset(), d)), Some(((3, 0), 3)));
        let none = nearest_matching(&map, start, 10, |h| h.to_offset() == (9, 9));
        assert!(none.is_none());
    }

    #[test]
    fn start_equals_goal_and_out_of_bounds() {
        let map = Map::new(&["..", ".."]);
        let h = Hex::from_offset(1, 1);
        assert_eq!(astar(&map, h, h), Some(vec![h]));
        assert!(astar(&map, h, Hex::from_offset(5, 5)).is_none());
    }

    #[test]
    fn terrain_costs_are_ordered_sensibly() {
        let plain = Tile {
            biome: Biome::Grass,
            ..Default::default()
        };
        let forest = Tile {
            biome: Biome::Forest,
            ..Default::default()
        };
        let mountain = Tile {
            biome: Biome::Mountain,
            ..Default::default()
        };
        let road = Tile {
            biome: Biome::Grass,
            road: 2,
            ..Default::default()
        };
        assert!(terrain_cost_for(&road, false) < terrain_cost_for(&plain, false));
        assert!(terrain_cost_for(&plain, false) < terrain_cost_for(&forest, false));
        assert!(terrain_cost_for(&forest, false) < terrain_cost_for(&mountain, false));
    }
}
