//! Deterministic RNG.
//!
//! The simulation must be reproducible: the same seed and the same script have to
//! produce the byte-identical world, or the "state hash" oracle in the test suite
//! means nothing. Every random draw in the engine therefore comes from this PCG32
//! implementation, held by [`crate::world::World`], and nothing else.
//!
//! PCG32 (O'Neill, 2014): a 64-bit LCG whose output is xorshifted and then
//! bit-rotated by the top bits of the previous state. It is tiny, fast, and has
//! good statistical properties for a game sim.

/// Portable PCG32 random number generator.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rng {
    state: u64,
    inc: u64,
}

impl Rng {
    /// Create a stream from a seed. Two `Rng`s with the same seed produce the same
    /// sequence forever.
    pub fn new(seed: u64) -> Self {
        // Standard PCG seeding: derive a stream constant from the seed, run a
        // couple of warm-up steps, then scramble.
        let mut rng = Rng {
            state: 0,
            inc: (seed << 1) | 1,
        };
        rng.next_u32();
        rng.state = rng.state.wrapping_add(0x853c_49e6_748f_ea9b);
        rng.next_u32();
        rng
    }

    /// A stream that is guaranteed not to collide with `Rng::new(seed)`.
    pub fn stream(seed: u64, stream: u64) -> Self {
        let mut rng = Rng::new(seed);
        rng.inc = (stream << 1) | 1;
        rng.state = rng.state.wrapping_add(0x853c_49e6_748f_ea9b);
        rng.next_u32();
        rng
    }

    /// Raw 32-bit output.
    pub fn next_u32(&mut self) -> u32 {
        let old = self.state;
        self.state = old
            .wrapping_mul(6364136223846793005)
            .wrapping_add(self.inc);
        let xorshifted = (((old >> 18) ^ old) >> 27) as u32;
        let rot = (old >> 59) as u32;
        xorshifted.rotate_right(rot)
    }

    /// 64 bits, built from two 32-bit draws.
    pub fn next_u64(&mut self) -> u64 {
        ((self.next_u32() as u64) << 32) | self.next_u32() as u64
    }

    /// Uniform `f32` in `[0, 1)`.
    pub fn next_f32(&mut self) -> f32 {
        (self.next_u32() >> 8) as f32 / (1u32 << 24) as f32
    }

    /// Uniform `f64` in `[0, 1)`.
    pub fn next_f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// Uniform integer in `[0, n)`. Returns 0 when `n == 0`.
    pub fn below(&mut self, n: u32) -> u32 {
        if n == 0 {
            return 0;
        }
        // Debiased modulo (Lemire-ish rejection): draw until the value is in the
        // largest multiple of `n` that fits in u32.
        let zone = u32::MAX - (u32::MAX % n);
        loop {
            let v = self.next_u32();
            if v < zone {
                return v % n;
            }
        }
    }

    /// Uniform integer in `[lo, hi]` (inclusive on both ends).
    pub fn range(&mut self, lo: i32, hi: i32) -> i32 {
        if hi <= lo {
            return lo;
        }
        lo + self.below((hi - lo + 1) as u32) as i32
    }

    /// True with probability `p` (clamped to `[0, 1]`).
    pub fn chance(&mut self, p: f32) -> bool {
        if p <= 0.0 {
            return false;
        }
        if p >= 1.0 {
            return true;
        }
        self.next_f32() < p
    }

    /// Pick an element uniformly.
    pub fn pick<'a, T>(&mut self, xs: &'a [T]) -> &'a T {
        &xs[self.below(xs.len() as u32) as usize]
    }

    /// Pick an element and copy it (for `Copy` element types).
    pub fn pick_copy<T: Copy>(&mut self, xs: &[T]) -> T {
        xs[self.below(xs.len() as u32) as usize]
    }

    /// In-place Fisher-Yates shuffle.
    pub fn shuffle<T>(&mut self, xs: &mut [T]) {
        if xs.len() < 2 {
            return;
        }
        for i in (1..xs.len()).rev() {
            let j = self.below(i as u32 + 1) as usize;
            xs.swap(i, j);
        }
    }

    /// Weighted index: returns the first index whose cumulative weight exceeds a
    /// uniform draw. Zero-weight entries are never chosen.
    pub fn weighted(&mut self, weights: &[u32]) -> usize {
        let total: u32 = weights.iter().sum();
        if total == 0 {
            return 0;
        }
        let mut roll = self.below(total);
        for (i, w) in weights.iter().enumerate() {
            if *w == 0 {
                continue;
            }
            if roll < *w {
                return i;
            }
            roll -= *w;
        }
        weights.len().saturating_sub(1)
    }

    /// A shuffle-free "jitter around a centre" helper used by spawners.
    pub fn jitter(&mut self, amount: i32) -> i32 {
        self.range(-amount, amount)
    }

    /// The full internal state, for saving and for the world hash.
    pub fn state(&self) -> u64 {
        self.state
    }

    /// The stream increment, for saving.
    pub fn inc(&self) -> u64 {
        self.inc
    }

    /// Rebuild an RNG exactly where it was left.
    pub fn from_state(state: u64, inc: u64) -> Rng {
        Rng { state, inc }
    }

    /// Advance the stream without using the value (keeps saves small when a
    /// system needs to consume randomness in lockstep).
    pub fn skip(&mut self, n: u32) {
        for _ in 0..n {
            self.next_u32();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_seed_same_sequence() {
        let mut a = Rng::new(12345);
        let mut b = Rng::new(12345);
        for _ in 0..1000 {
            assert_eq!(a.next_u32(), b.next_u32());
        }
    }

    #[test]
    fn different_seeds_diverge() {
        let mut a = Rng::new(1);
        let mut b = Rng::new(2);
        let sa: Vec<u32> = (0..64).map(|_| a.next_u32()).collect();
        let sb: Vec<u32> = (0..64).map(|_| b.next_u32()).collect();
        assert_ne!(sa, sb);
    }

    #[test]
    fn range_is_inclusive_and_bounded() {
        let mut rng = Rng::new(7);
        let mut seen_lo = false;
        let mut seen_hi = false;
        for _ in 0..10_000 {
            let v = rng.range(-3, 4);
            assert!((-3..=4).contains(&v));
            seen_lo |= v == -3;
            seen_hi |= v == 4;
        }
        assert!(seen_lo && seen_hi);
        assert_eq!(rng.range(5, 5), 5);
        assert_eq!(rng.range(5, 1), 5);
    }

    #[test]
    fn floats_are_in_unit_interval() {
        let mut rng = Rng::new(99);
        let mut sum = 0.0f64;
        for _ in 0..100_000 {
            let v = rng.next_f32();
            assert!((0.0..1.0).contains(&v));
            sum += v as f64;
        }
        let mean = sum / 100_000.0;
        assert!((mean - 0.5).abs() < 0.01, "mean was {mean}");
    }

    #[test]
    fn below_is_uniform_enough() {
        let mut rng = Rng::new(4242);
        let mut buckets = [0u32; 6];
        for _ in 0..60_000 {
            buckets[rng.below(6) as usize] += 1;
        }
        // Every bucket should be near 10_000.
        for b in buckets {
            assert!((9_000..11_000).contains(&b), "bucket was {b}");
        }
    }

    #[test]
    fn shuffle_keeps_elements() {
        let mut rng = Rng::new(3);
        let mut xs = (0..32).collect::<Vec<i32>>();
        rng.shuffle(&mut xs);
        xs.sort_unstable();
        assert_eq!(xs, (0..32).collect::<Vec<i32>>());
    }

    #[test]
    fn weighted_respects_zero_weights() {
        let mut rng = Rng::new(11);
        for _ in 0..1000 {
            assert_ne!(rng.weighted(&[0, 5, 0, 5]), 0);
            assert_ne!(rng.weighted(&[0, 5, 0, 5]), 2);
        }
    }
}
