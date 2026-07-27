//! Per-attempt question selection + option shuffling (anti-gaming).
//!
//! A draw picks a stratified subset of a bank's questions and, for each
//! selected question, a shuffled option order — both deterministic in the
//! attempt `seed`. Because every attempt uses a fresh seed, no two attempts
//! present the same questions in the same order.
//!
//! # Two axes, not one
//!
//! Stratification spans a **(Bloom level, difficulty)** grid rather than
//! difficulty alone. The two measure different things: difficulty is how hard
//! an item is, a Bloom level is what kind of thinking it demands. A learner
//! who can recall every fact but cannot apply any of them should not be able
//! to draw a whole attempt from the `remember` row and pass — spanning both
//! axes is what stops that.
//!
//! When every item shares one Bloom level the grid collapses to a single row
//! and this behaves exactly as difficulty-only stratification did.
//!
//! # Changing the draw is safe
//!
//! An attempt persists its `question_ids` and `option_orders`; grading reads
//! those back rather than re-deriving them from the seed. So the algorithm
//! here can change without invalidating attempts already in flight.

use std::collections::BTreeMap;

use super::{shuffle, SplitMix64};
use crate::domain::bloom::BloomLevel;

/// Minimal metadata the randomizer needs about a bank question.
#[derive(Debug, Clone, PartialEq)]
pub struct QuestionMeta {
    pub id: String,
    pub difficulty: u8,
    pub option_count: usize,
    /// Cognitive level. Items authored before this axis existed default to
    /// [`BloomLevel::Apply`], which puts them all in one row — the previous
    /// behaviour.
    pub bloom: BloomLevel,
}

/// The result of a draw: which questions (in served order) and, per question,
/// the shuffled option order (`option_orders[q][pos]` = original option index
/// shown at position `pos`).
#[derive(Debug, Clone, PartialEq)]
pub struct Draw {
    pub question_ids: Vec<String>,
    pub option_orders: Vec<Vec<usize>>,
}

/// Draw up to `count` questions, stratified across a (Bloom level,
/// difficulty) grid so the set spans both axes rather than clustering, then
/// shuffle each question's options. Deterministic in `seed`.
pub fn draw(questions: &[QuestionMeta], count: usize, seed: u64) -> Draw {
    let mut rng = SplitMix64(seed);

    // Group into Bloom rows, each row holding its difficulty buckets.
    // BTreeMaps throughout: iteration order must be fixed or the draw stops
    // being reproducible for a seed.
    let mut rows: BTreeMap<u8, BTreeMap<u8, Vec<&QuestionMeta>>> = BTreeMap::new();
    for q in questions {
        rows.entry(q.bloom.rank())
            .or_default()
            .entry(q.difficulty.min(5))
            .or_default()
            .push(q);
    }
    for row in rows.values_mut() {
        for bucket in row.values_mut() {
            shuffle(bucket, &mut rng);
        }
    }

    // Round-robin across Bloom rows *first*, taking one item per row per
    // pass and advancing that row's difficulty cursor each time.
    //
    // Iterating a flat (bloom, difficulty) map instead would exhaust every
    // difficulty within `remember` before reaching `understand`, so a short
    // draw would sit entirely in the lowest Bloom level — precisely what
    // this axis exists to prevent. Rows first means the first `n` picks span
    // `n` distinct Bloom levels, while the cursor keeps difficulty spread
    // inside each row.
    let mut cursors: BTreeMap<u8, usize> = rows.keys().map(|&b| (b, 0usize)).collect();
    let mut selected: Vec<&QuestionMeta> = Vec::new();
    let want = count.min(questions.len());
    let mut progress = true;

    while selected.len() < want && progress {
        progress = false;
        for (bloom, row) in rows.iter_mut() {
            if selected.len() >= want {
                break;
            }
            let difficulties: Vec<u8> = row.keys().copied().collect();
            if difficulties.is_empty() {
                continue;
            }
            let cursor = cursors.entry(*bloom).or_insert(0);

            // Walk this row's difficulties from the cursor until one yields.
            for step in 0..difficulties.len() {
                let d = difficulties[(*cursor + step) % difficulties.len()];
                if let Some(q) = row.get_mut(&d).and_then(|b| b.pop()) {
                    selected.push(q);
                    *cursor = (*cursor + step + 1) % difficulties.len();
                    progress = true;
                    break;
                }
            }
        }
    }

    // Shuffle the final served order too (so difficulty isn't monotonic).
    shuffle(&mut selected, &mut rng);

    let mut question_ids = Vec::with_capacity(selected.len());
    let mut option_orders = Vec::with_capacity(selected.len());
    for q in selected {
        question_ids.push(q.id.clone());
        let mut order: Vec<usize> = (0..q.option_count).collect();
        shuffle(&mut order, &mut rng);
        option_orders.push(order);
    }
    Draw {
        question_ids,
        option_orders,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::bloom::BloomLevel;

    fn bank(n: usize) -> Vec<QuestionMeta> {
        (0..n)
            .map(|i| QuestionMeta {
                id: format!("q{i}"),
                difficulty: (i % 5 + 1) as u8,
                option_count: 4,
                bloom: BloomLevel::Apply,
            })
            .collect()
    }

    /// A bank spanning both axes: Bloom level cycles independently of
    /// difficulty, so the grid has many populated cells.
    fn graded_bank(n: usize) -> Vec<QuestionMeta> {
        (0..n)
            .map(|i| QuestionMeta {
                id: format!("q{i}"),
                difficulty: (i % 5 + 1) as u8,
                option_count: 4,
                bloom: BloomLevel::ALL[i % BloomLevel::ALL.len()],
            })
            .collect()
    }

    fn bloom_of(id: &str, bank: &[QuestionMeta]) -> BloomLevel {
        bank.iter().find(|q| q.id == id).unwrap().bloom
    }

    #[test]
    fn draw_is_deterministic_for_a_seed() {
        let qs = bank(20);
        assert_eq!(draw(&qs, 5, 42), draw(&qs, 5, 42));
    }

    #[test]
    fn different_seeds_generally_differ() {
        let qs = bank(20);
        let a = draw(&qs, 5, 1);
        let b = draw(&qs, 5, 2);
        assert!(
            a != b,
            "distinct seeds should (almost always) draw differently"
        );
    }

    #[test]
    fn draws_requested_count_and_valid_ids() {
        let qs = bank(20);
        let d = draw(&qs, 5, 7);
        assert_eq!(d.question_ids.len(), 5);
        assert_eq!(d.option_orders.len(), 5);
        // no duplicate questions
        let mut ids = d.question_ids.clone();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), 5);
        // each option order is a permutation of 0..4
        for order in &d.option_orders {
            let mut o = order.clone();
            o.sort();
            assert_eq!(o, vec![0, 1, 2, 3]);
        }
    }

    #[test]
    fn count_capped_at_available() {
        let qs = bank(3);
        let d = draw(&qs, 10, 1);
        assert_eq!(d.question_ids.len(), 3);
    }

    #[test]
    fn stratifies_across_difficulties() {
        // 5 buckets, draw 5 → should hit multiple difficulty levels, not all
        // from one bucket.
        let qs = bank(25);
        let d = draw(&qs, 5, 99);
        let diffs: std::collections::HashSet<u8> = d
            .question_ids
            .iter()
            .map(|id| {
                let i: usize = id[1..].parse().unwrap();
                (i % 5 + 1) as u8
            })
            .collect();
        assert!(
            diffs.len() >= 3,
            "draw should span >= 3 difficulty levels, got {diffs:?}"
        );
    }

    #[test]
    fn stratifies_across_bloom_levels() {
        // The reason the second axis exists: an attempt must not be drawable
        // entirely from the `remember` row.
        let qs = graded_bank(36);
        let d = draw(&qs, 6, 99);
        let levels: std::collections::HashSet<BloomLevel> =
            d.question_ids.iter().map(|id| bloom_of(id, &qs)).collect();
        assert!(
            levels.len() >= 3,
            "draw should span >= 3 Bloom levels, got {levels:?}"
        );
    }

    #[test]
    fn spans_both_axes_at_once() {
        let qs = graded_bank(36);
        let d = draw(&qs, 6, 5);
        let cells: std::collections::HashSet<(BloomLevel, u8)> = d
            .question_ids
            .iter()
            .map(|id| {
                let q = qs.iter().find(|q| &q.id == id).unwrap();
                (q.bloom, q.difficulty)
            })
            .collect();
        assert_eq!(
            cells.len(),
            d.question_ids.len(),
            "each drawn item should come from a distinct grid cell: {cells:?}"
        );
    }

    #[test]
    fn uniform_bloom_still_stratifies_by_difficulty() {
        // With one Bloom level the grid collapses to a single row, and the
        // draw must behave exactly as difficulty-only stratification did.
        let qs = bank(25);
        let d = draw(&qs, 5, 99);
        let diffs: std::collections::HashSet<u8> = d
            .question_ids
            .iter()
            .map(|id| qs.iter().find(|q| &q.id == id).unwrap().difficulty)
            .collect();
        assert!(
            diffs.len() >= 3,
            "collapsing to one Bloom row must not lose difficulty spread, got {diffs:?}"
        );
    }

    #[test]
    fn bloom_axis_does_not_break_determinism() {
        let qs = graded_bank(30);
        assert_eq!(draw(&qs, 7, 1234), draw(&qs, 7, 1234));
    }

    #[test]
    fn draws_without_duplicates_across_the_grid() {
        let qs = graded_bank(30);
        let d = draw(&qs, 12, 3);
        let mut ids = d.question_ids.clone();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), d.question_ids.len(), "no item drawn twice");
    }
}
