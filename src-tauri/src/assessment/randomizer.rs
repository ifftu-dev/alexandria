//! Per-attempt question selection + option shuffling (anti-gaming).
//!
//! A draw picks a stratified subset of a bank's questions and, for each
//! selected question, a shuffled option order — both deterministic in the
//! attempt `seed` and the learner's [`DrawContext`]. Because every attempt
//! uses a fresh seed, no two attempts present the same questions in the same
//! order.
//!
//! # Adapted to the learner
//!
//! The draw also protects the bank and follows the learner (see
//! `ai-chat-spec.md` §13A, item-bank protection):
//!
//! * items this profile was served before are skipped while unseen ones
//!   remain, then reused least-exposed first;
//! * every transfer item (same skill, different surface form) is served on
//!   top of the standard draw;
//! * the standard draw centres on the [`DifficultyBand`] the learner's
//!   earlier answers suggest.
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

use std::collections::{BTreeMap, HashMap};

use super::irt::{estimate_theta_eap, ItemParams};
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
    /// A transfer item: same skill, different surface form. Every draw
    /// serves all of a bank's transfer items in addition to `count`
    /// standard items, whatever their exposure or difficulty.
    pub transfer: bool,
}

/// The result of a draw: which questions (in served order) and, per question,
/// the shuffled option order (`option_orders[q][pos]` = original option index
/// shown at position `pos`).
#[derive(Debug, Clone, PartialEq)]
pub struct Draw {
    pub question_ids: Vec<String>,
    pub option_orders: Vec<Vec<usize>>,
}

/// What a draw knows about the learner beyond the bank itself.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DrawContext {
    /// How many earlier attempts by this profile served each item. An item
    /// absent from the map has never been served.
    pub exposure: HashMap<String, u32>,
    /// The difficulty band the learner's earlier answers point to, or `None`
    /// with no graded history, in which case the draw spans every difficulty.
    pub band: Option<DifficultyBand>,
}

impl DrawContext {
    fn exposure_of(&self, id: &str) -> u32 {
        self.exposure.get(id).copied().unwrap_or(0)
    }

    fn in_band(&self, difficulty: u8) -> bool {
        self.band
            .is_none_or(|b| (b.low..=b.high).contains(&difficulty))
    }
}

/// An inclusive range of the 1–5 item difficulty scale.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DifficultyBand {
    pub low: u8,
    pub high: u8,
}

impl DifficultyBand {
    /// The band suggested by earlier answers, given as `(difficulty, score)`
    /// pairs with a score of at least 0.5 counting as correct.
    ///
    /// Ability is estimated with the same 2PL model adaptive delivery uses
    /// ([`estimate_theta_eap`] over [`ItemParams::from_difficulty`]); the band
    /// is the difficulty whose item is 50/50 nearest that ability, plus one
    /// level either side. `None` when there is no history.
    pub fn from_prior(prior: &[(u8, f64)]) -> Option<Self> {
        if prior.is_empty() {
            return None;
        }
        let responses: Vec<(ItemParams, bool)> = prior
            .iter()
            .map(|&(d, score)| (ItemParams::from_difficulty(d), score >= 0.5))
            .collect();
        let theta = estimate_theta_eap(&responses).theta;
        let centre = (1..=5u8)
            .min_by(|&a, &b| {
                let da = (ItemParams::from_difficulty(a).b - theta).abs();
                let db = (ItemParams::from_difficulty(b).b - theta).abs();
                da.total_cmp(&db)
            })
            .unwrap_or(3);
        Some(Self {
            low: centre.saturating_sub(1).max(1),
            high: (centre + 1).min(5),
        })
    }
}

/// Draw up to `count` questions, stratified across a (Bloom level,
/// difficulty) grid so the set spans both axes rather than clustering, then
/// shuffle each question's options. Deterministic in `seed` and `ctx`.
///
/// Every transfer item is served on top of the `count` standard items.
/// Standard items the profile has never been served are drawn first; only
/// when they run out does the draw fall back to served items, least exposed
/// first. Within each exposure level, items inside `ctx.band` come before
/// items outside it: exposure protects the bank, the band only sharpens
/// measurement, so exposure wins.
pub fn draw(questions: &[QuestionMeta], count: usize, seed: u64, ctx: &DrawContext) -> Draw {
    let mut rng = SplitMix64(seed);
    let (transfers, standard): (Vec<&QuestionMeta>, Vec<&QuestionMeta>) =
        questions.iter().partition(|q| q.transfer);
    let want = count.min(standard.len());

    // Tiers keyed (exposure, outside band): least exposed first, in-band
    // before out-of-band. BTreeMap keeps the order fixed.
    let mut tiers: BTreeMap<(u32, bool), Vec<&QuestionMeta>> = BTreeMap::new();
    for q in standard {
        let key = (ctx.exposure_of(&q.id), !ctx.in_band(q.difficulty));
        tiers.entry(key).or_default().push(q);
    }
    let mut selected: Vec<&QuestionMeta> = Vec::with_capacity(want + transfers.len());
    for tier in tiers.into_values() {
        if selected.len() >= want {
            break;
        }
        let needed = want - selected.len();
        selected.extend(stratified(tier, needed, &mut rng));
    }
    selected.extend(transfers);

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

/// Pick up to `want` items from `pool`, spread across the (Bloom level,
/// difficulty) grid.
fn stratified<'a>(
    pool: Vec<&'a QuestionMeta>,
    want: usize,
    rng: &mut SplitMix64,
) -> Vec<&'a QuestionMeta> {
    // Group into Bloom rows, each row holding its difficulty buckets.
    // BTreeMaps throughout: iteration order must be fixed or the draw stops
    // being reproducible for a seed.
    let mut rows: BTreeMap<u8, BTreeMap<u8, Vec<&QuestionMeta>>> = BTreeMap::new();
    for q in pool {
        rows.entry(q.bloom.rank())
            .or_default()
            .entry(q.difficulty.min(5))
            .or_default()
            .push(q);
    }
    for row in rows.values_mut() {
        for bucket in row.values_mut() {
            shuffle(bucket, rng);
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
    selected
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
                transfer: false,
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
                transfer: false,
            })
            .collect()
    }

    fn bloom_of(id: &str, bank: &[QuestionMeta]) -> BloomLevel {
        bank.iter().find(|q| q.id == id).unwrap().bloom
    }

    #[test]
    fn draw_is_deterministic_for_a_seed() {
        let qs = bank(20);
        assert_eq!(
            draw(&qs, 5, 42, &DrawContext::default()),
            draw(&qs, 5, 42, &DrawContext::default())
        );
    }

    #[test]
    fn different_seeds_generally_differ() {
        let qs = bank(20);
        let a = draw(&qs, 5, 1, &DrawContext::default());
        let b = draw(&qs, 5, 2, &DrawContext::default());
        assert!(
            a != b,
            "distinct seeds should (almost always) draw differently"
        );
    }

    #[test]
    fn draws_requested_count_and_valid_ids() {
        let qs = bank(20);
        let d = draw(&qs, 5, 7, &DrawContext::default());
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
        let d = draw(&qs, 10, 1, &DrawContext::default());
        assert_eq!(d.question_ids.len(), 3);
    }

    #[test]
    fn stratifies_across_difficulties() {
        // 5 buckets, draw 5 → should hit multiple difficulty levels, not all
        // from one bucket.
        let qs = bank(25);
        let d = draw(&qs, 5, 99, &DrawContext::default());
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
        let d = draw(&qs, 6, 99, &DrawContext::default());
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
        let d = draw(&qs, 6, 5, &DrawContext::default());
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
        let d = draw(&qs, 5, 99, &DrawContext::default());
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
        assert_eq!(
            draw(&qs, 7, 1234, &DrawContext::default()),
            draw(&qs, 7, 1234, &DrawContext::default())
        );
    }

    #[test]
    fn draws_without_duplicates_across_the_grid() {
        let qs = graded_bank(30);
        let d = draw(&qs, 12, 3, &DrawContext::default());
        let mut ids = d.question_ids.clone();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), d.question_ids.len(), "no item drawn twice");
    }
}
