//! Per-attempt question selection + option shuffling (anti-gaming).
//!
//! A draw picks a difficulty-stratified subset of a bank's questions and, for
//! each selected question, a shuffled option order — both deterministic in the
//! attempt `seed` so grading can reproduce them. Because every attempt uses a
//! fresh seed, no two attempts present the same questions in the same order.

use super::{shuffle, SplitMix64};

/// Minimal metadata the randomizer needs about a bank question.
#[derive(Debug, Clone, PartialEq)]
pub struct QuestionMeta {
    pub id: String,
    pub difficulty: u8,
    pub option_count: usize,
}

/// The result of a draw: which questions (in served order) and, per question,
/// the shuffled option order (`option_orders[q][pos]` = original option index
/// shown at position `pos`).
#[derive(Debug, Clone, PartialEq)]
pub struct Draw {
    pub question_ids: Vec<String>,
    pub option_orders: Vec<Vec<usize>>,
}

/// Draw up to `count` questions, stratified across difficulty buckets so the
/// set spans easy→hard rather than clustering, then shuffle each question's
/// options. Deterministic in `seed`.
pub fn draw(questions: &[QuestionMeta], count: usize, seed: u64) -> Draw {
    let mut rng = SplitMix64(seed);

    // Bucket by difficulty (1..=5), shuffle within each bucket.
    let mut buckets: Vec<Vec<&QuestionMeta>> = vec![Vec::new(); 6];
    for q in questions {
        let d = (q.difficulty.min(5)) as usize;
        buckets[d].push(q);
    }
    for b in buckets.iter_mut() {
        shuffle(b, &mut rng);
    }

    // Round-robin across non-empty buckets (easy→hard) so the draw spreads
    // difficulty, until we have `count` (or run out).
    let mut selected: Vec<&QuestionMeta> = Vec::new();
    let want = count.min(questions.len());
    let mut progress = true;
    while selected.len() < want && progress {
        progress = false;
        for b in buckets.iter_mut() {
            if selected.len() >= want {
                break;
            }
            if let Some(q) = b.pop() {
                selected.push(q);
                progress = true;
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

    fn bank(n: usize) -> Vec<QuestionMeta> {
        (0..n)
            .map(|i| QuestionMeta {
                id: format!("q{i}"),
                difficulty: (i % 5 + 1) as u8,
                option_count: 4,
            })
            .collect()
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
}
