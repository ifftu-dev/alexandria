//! Synthetic adversarial-prior data generation for the Sentinel paste
//! classifier.
//!
//! Produces JSON blobs matching the schema in
//! `docs/sentinel-adversarial-priors.md` §Phase 2 plus a new
//! `model_kind = "keystroke_aggregate"` variant for windowed-feature
//! training data. The same code path produces both the training corpus
//! and the holdout — callers use disjoint seed ranges to keep them
//! statistically independent.
//!
//! Generators are deterministic given a seed (ChaCha20 RNG). A fixed
//! seed produces byte-identical output; tests pin this with Blake2b
//! hash assertions so model retraining is reproducible.

pub mod bigrams;
pub mod blob;
pub mod generators;
pub mod rng;

pub use blob::PriorBlob;
