//! English bigram frequency table (compact subset).
//!
//! Used by the `human_baseline` generator: humans type frequent
//! bigrams (`th`, `he`, `in`, `er`) faster than rare ones (`qx`,
//! `zj`). Encoding that pattern in synthetic samples is what lets a
//! classifier later distinguish a bot-with-Gaussian-jitter (uniform
//! across digraphs) from a real human.
//!
//! The table is a hand-curated subset weighted by Peter Norvig's
//! Google n-gram corpus (2009). Exact values aren't load-bearing —
//! we only need the *ordering* (frequent vs rare) and a non-trivial
//! spread for the speed model to encode something realistic.

/// Returns the relative frequency for a bigram (lowercase ASCII).
/// Unknown bigrams get a small floor so the distribution stays
/// well-defined.
pub fn frequency(digraph: &str) -> f32 {
    match digraph {
        "th" => 3.88,
        "he" => 3.68,
        "in" => 2.28,
        "er" => 2.18,
        "an" => 2.14,
        "re" => 1.75,
        "on" => 1.41,
        "at" => 1.33,
        "en" => 1.30,
        "nd" => 1.28,
        "ti" => 1.28,
        "es" => 1.27,
        "or" => 1.27,
        "te" => 1.20,
        "of" => 1.15,
        "ed" => 1.15,
        "is" => 1.13,
        "it" => 1.12,
        "al" => 1.09,
        "ar" => 1.07,
        "st" => 1.05,
        "to" => 1.04,
        "nt" => 1.04,
        "ng" => 0.95,
        "se" => 0.93,
        "ha" => 0.93,
        "as" => 0.87,
        "ou" => 0.87,
        "io" => 0.83,
        "le" => 0.83,
        "ve" => 0.83,
        "co" => 0.79,
        "me" => 0.79,
        "de" => 0.76,
        "hi" => 0.76,
        "ri" => 0.73,
        "ro" => 0.73,
        "ic" => 0.70,
        "ne" => 0.69,
        "ea" => 0.69,
        "ra" => 0.69,
        "ce" => 0.65,
        // Rare bigrams
        "qx" => 0.001,
        "zj" => 0.001,
        "qz" => 0.001,
        "jx" => 0.001,
        "vq" => 0.001,
        _ => 0.30,
    }
}

/// Compact bigram pool — enough breadth for the classifier to learn
/// digraph-correlated speed patterns, small enough to keep the seed
/// space deterministic and the JSON blobs human-readable in diffs.
pub const POOL: &[&str] = &[
    "th", "he", "in", "er", "an", "re", "on", "at", "en", "nd", "ti", "es", "or", "te", "of", "ed",
    "is", "it", "al", "ar", "st", "to", "nt", "ng", "se", "ha", "as", "ou", "io", "le", "ve", "co",
    "me", "de", "hi", "ri", "ro", "ic", "ne", "ea", "ra", "ce", "qx", "zj", "qz", "jx", "vq",
];

/// Sample a bigram weighted by frequency, given a uniform `[0,1]` draw.
pub fn sample_weighted(u: f32) -> &'static str {
    let mut total = 0.0_f32;
    for bg in POOL {
        total += frequency(bg);
    }
    let mut target = u * total;
    for bg in POOL {
        target -= frequency(bg);
        if target <= 0.0 {
            return bg;
        }
    }
    POOL[POOL.len() - 1]
}
