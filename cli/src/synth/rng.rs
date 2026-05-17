//! Seeded RNG + hashing helpers.
//!
//! All synthetic data uses ChaCha20 keyed off a 64-bit seed so a fixed
//! seed produces byte-identical output. The generated JSON is hashed
//! with Blake2b in tests to assert determinism over time — if a code
//! change shifts a single byte, the assertion will fail loudly and
//! force a `SYNTH_VERSION` bump.

use blake2::{Blake2b512, Digest};
use rand::SeedableRng;
use rand_chacha::ChaCha20Rng;

pub fn rng_from_seed(seed: u64) -> ChaCha20Rng {
    ChaCha20Rng::seed_from_u64(seed)
}

pub fn blake2b_hex(bytes: &[u8]) -> String {
    let mut hasher = Blake2b512::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    hex::encode(digest)
}
