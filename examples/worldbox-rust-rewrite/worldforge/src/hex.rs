//! Hex grid maths.
//!
//! The map is stored as a rectangular array of **odd-r offset** coordinates
//! (pointy-top hexes, read row by row), but every distance, neighbour and path
//! calculation happens in **axial** coordinates, where the arithmetic is clean.
//!
//! ```text
//!   odd rows are pushed right by half a hex:
//!
//!      (0,0) (1,0) (2,0)        axial -> offset: col = q + (r - (r & 1)) / 2
//!        (0,1) (1,1) (2,1)      offset -> axial: q = col - (row - (row & 1)) / 2
//!      (0,2) (1,2) (2,2)                        r = row
//! ```

use std::fmt;

/// Axial hex coordinate. `q` grows to the right, `r` grows down-left.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug, Default)]
pub struct Hex {
    pub q: i32,
    pub r: i32,
}

/// The six axial neighbour offsets, in clockwise order starting east.
pub const HEX_DIRS: [Hex; 6] = [
    Hex { q: 1, r: 0 },
    Hex { q: 1, r: -1 },
    Hex { q: 0, r: -1 },
    Hex { q: -1, r: 0 },
    Hex { q: -1, r: 1 },
    Hex { q: 0, r: 1 },
];

impl Hex {
    pub const fn new(q: i32, r: i32) -> Self {
        Hex { q, r }
    }

    /// Cube coordinates, useful for rounding a floating point hex.
    pub fn cube(self) -> (i32, i32, i32) {
        (self.q, -self.q - self.r, self.r)
    }

    /// Neighbour in direction `dir` (0..6, wraps).
    pub fn neighbor(self, dir: usize) -> Hex {
        let d = HEX_DIRS[dir % 6];
        Hex::new(self.q + d.q, self.r + d.r)
    }

    /// All six neighbours.
    pub fn neighbors(self) -> [Hex; 6] {
        [
            self.neighbor(0),
            self.neighbor(1),
            self.neighbor(2),
            self.neighbor(3),
            self.neighbor(4),
            self.neighbor(5),
        ]
    }

    /// Hex distance (number of steps between the two tiles).
    pub fn distance(self, other: Hex) -> i32 {
        let dq = self.q - other.q;
        let dr = self.r - other.r;
        (dq.abs() + dr.abs() + (dq + dr).abs()) / 2
    }

    /// Offset (column, row) for a rectangle-shaped map, odd rows shifted right.
    pub fn to_offset(self) -> (i32, i32) {
        let row = self.r;
        let col = self.q + (row - (row & 1)) / 2;
        (col, row)
    }

    /// Inverse of [`Hex::to_offset`].
    pub fn from_offset(col: i32, row: i32) -> Hex {
        let q = col - (row - (row & 1)) / 2;
        Hex::new(q, row)
    }

    /// Hexes exactly `radius` steps away, walking clockwise from the west corner.
    pub fn ring(self, radius: i32) -> Vec<Hex> {
        if radius <= 0 {
            return vec![self];
        }
        let mut out = Vec::with_capacity((radius * 6) as usize);
        // Start at the corner that is `radius` steps to the north-west.
        let mut h = Hex::new(self.q - radius, self.r + radius);
        for dir in 0..6 {
            for _ in 0..radius {
                out.push(h);
                h = h.neighbor(dir);
            }
        }
        out
    }

    /// Every hex within `radius` (including the centre), ordered ring by ring.
    pub fn spiral(self, radius: i32) -> Vec<Hex> {
        let mut out = Vec::new();
        for r in 0..=radius.max(0) {
            out.extend(self.ring(r));
        }
        out
    }

    /// Bresenham-equivalent hex line using cube rounding.
    pub fn line_to(self, other: Hex) -> Vec<Hex> {
        let n = self.distance(other);
        if n == 0 {
            return vec![self];
        }
        // Linear interpolation in axial space is the same as cube-space lerp,
        // because the third cube component is derived from the other two.
        let (aq, ar) = (self.q, self.r);
        let (bq, br) = (other.q, other.r);
        let mut out = Vec::with_capacity(n as usize + 1);
        for i in 0..=n {
            let t = i as f32 / n as f32;
            let q = aq as f32 + (bq - aq) as f32 * t;
            let r = ar as f32 + (br - ar) as f32 * t;
            out.push(round_cube(q, r));
        }
        out
    }

    /// Centre of the hex in pixel space, pointy-top layout.
    pub fn to_pixel(self, size: f32) -> (f32, f32) {
        let x = size * 3.0f32.sqrt() * (self.q as f32 + self.r as f32 / 2.0);
        let y = size * 1.5 * self.r as f32;
        (x, y)
    }

    /// Which hex contains this pixel, pointy-top layout.
    pub fn from_pixel(x: f32, y: f32, size: f32) -> Hex {
        let q = (3.0f32.sqrt() / 3.0 * x - y / 3.0) / size;
        let r = (2.0 / 3.0 * y) / size;
        round_cube(q, r)
    }
}

/// Round a fractional hex to the nearest real hex (cube coordinate rounding).
fn round_cube(q: f32, r: f32) -> Hex {
    let (x, z) = (q, r);
    let y = -x - z;
    let (rx, ry, rz) = (x.round(), y.round(), z.round());
    let dx = (rx - x).abs();
    let dy = (ry - y).abs();
    let dz = (rz - z).abs();
    // Correct the component with the largest rounding error; the other two
    // components are returned unchanged, which is why `ry` is not rebuilt.
    if dx > dy && dx > dz {
        Hex::new((-ry - rz) as i32, rz as i32)
    } else if dy > dz {
        Hex::new(rx as i32, rz as i32)
    } else {
        Hex::new(rx as i32, (-rx - ry) as i32)
    }
}

impl fmt::Display for Hex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (c, r) = self.to_offset();
        write!(f, "{c},{r}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offset_roundtrip_handles_negatives() {
        for row in -8..8 {
            for col in -8..8 {
                let h = Hex::from_offset(col, row);
                assert_eq!(h.to_offset(), (col, row), "col={col} row={row}");
            }
        }
    }

    #[test]
    fn neighbors_are_adjacent_and_symmetric() {
        let h = Hex::new(3, -2);
        for (i, n) in h.neighbors().iter().enumerate() {
            assert_eq!(h.distance(*n), 1);
            assert_eq!(n.neighbor((i + 3) % 6), h);
        }
    }

    #[test]
    fn distance_is_a_metric() {
        let a = Hex::new(0, 0);
        let b = Hex::new(4, -3);
        let c = Hex::new(-2, 5);
        assert_eq!(a.distance(b), b.distance(a));
        assert_eq!(a.distance(a), 0);
        assert!(a.distance(c) <= a.distance(b) + b.distance(c));
        // A straight line of neighbours is `n` steps long.
        let h = Hex::new(5, 5);
        assert_eq!(h.distance(h.neighbor(2)), 1);
        assert_eq!(Hex::new(0, 0).distance(Hex::new(0, 3)), 3);
    }

    #[test]
    fn ring_has_six_times_radius_hexes() {
        for radius in 1..6 {
            let ring = Hex::new(2, 2).ring(radius);
            assert_eq!(ring.len() as i32, radius * 6);
            for h in ring {
                assert_eq!(h.distance(Hex::new(2, 2)), radius);
            }
        }
        assert_eq!(Hex::new(0, 0).ring(0), vec![Hex::new(0, 0)]);
    }

    #[test]
    fn spiral_is_contiguous() {
        let center = Hex::new(0, 0);
        let all = center.spiral(3);
        assert_eq!(all.len(), 1 + 3 * 3 * 4);
        assert!(all.iter().all(|h| h.distance(center) <= 3));
    }

    #[test]
    fn line_endpoints_and_continuity() {
        let a = Hex::new(-3, 1);
        let b = Hex::new(4, -2);
        let line = a.line_to(b);
        assert_eq!(line.first(), Some(&a));
        assert_eq!(line.last(), Some(&b));
        assert_eq!(line.len() as i32, a.distance(b) + 1);
        for pair in line.windows(2) {
            assert_eq!(pair[0].distance(pair[1]), 1, "{:?} -> {:?}", pair[0], pair[1]);
        }
    }

    #[test]
    fn pixel_roundtrip() {
        for q in -6..6 {
            for r in -6..6 {
                let h = Hex::new(q, r);
                let (x, y) = h.to_pixel(16.0);
                assert_eq!(Hex::from_pixel(x, y, 16.0), h, "hex {h:?}");
                // A small nudge inside the hex still resolves to the same tile.
                assert_eq!(Hex::from_pixel(x + 2.0, y + 2.0, 16.0), h);
            }
        }
    }
}
