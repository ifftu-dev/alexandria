//! Dynamic assessments: per-attempt randomized question selection + option
//! shuffling ([`randomizer`]) and host-side grading ([`grader`]).
//!
//! Both are pure and deterministic given a seed, so an attempt is reproducible
//! and the logic is unit-testable. The correct-answer key never leaves the
//! grader — it is not part of any client payload.

pub mod grader;
pub mod randomizer;

/// A tiny deterministic PRNG (SplitMix64) so a stored `seed` reproduces the
/// exact draw + shuffle for an attempt. Not cryptographic — it only needs to
/// be unpredictable-enough per attempt and fully reproducible for grading.
pub struct SplitMix64(pub u64);

impl SplitMix64 {
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform-ish index in `[0, n)`.
    pub fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            return 0;
        }
        (self.next_u64() % n as u64) as usize
    }
}

/// Fisher-Yates shuffle driven by a [`SplitMix64`], in place.
pub fn shuffle<T>(items: &mut [T], rng: &mut SplitMix64) {
    let n = items.len();
    for i in (1..n).rev() {
        let j = rng.below(i + 1);
        items.swap(i, j);
    }
}
