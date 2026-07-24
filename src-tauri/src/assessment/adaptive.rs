//! Adaptive item selection.
//!
//! A fixed-form attempt serves the same drawn set to everyone. Adaptive
//! delivery instead picks each next item from the current ability estimate,
//! so a strong learner is not made to grind through easy items and a weak one
//! is not buried under hard ones — the test converges on ability in fewer
//! items, which is the whole point of computer-adaptive testing.
//!
//! # What "best next item" means
//!
//! The most informative item for a learner is the one whose Fisher
//! information ([`ItemParams::information`]) is greatest at their current
//! `θ̂` — roughly, the item whose difficulty sits nearest their ability. But
//! always serving the single most informative item is a bank-security
//! disaster: every learner near a given ability sees the same handful of
//! items, so those items leak fast.
//!
//! # Exposure control
//!
//! So selection is **randomesque**: rank the remaining items by information
//! at `θ̂`, then pick uniformly at random from the top `k`. The served item
//! is still highly informative, but which of the top few a given learner
//! sees varies, spreading exposure across the bank. `k` trades measurement
//! efficiency against exposure — larger `k` protects the bank more and
//! measures slightly less sharply per item.
//!
//! # Stopping
//!
//! An attempt ends when the estimate is precise enough (`SE < target`), or a
//! `max_items` ceiling is hit, whichever comes first — but never before a
//! `min_items` floor, so a lucky-looking early run cannot mint a credential
//! off two questions.

use crate::assessment::irt::{estimate_theta_eap, AbilityEstimate, ItemParams};
use crate::assessment::SplitMix64;

/// One selectable item: its id and 2PL parameters.
#[derive(Debug, Clone, PartialEq)]
pub struct PoolItem {
    pub id: String,
    pub params: ItemParams,
}

/// When an adaptive attempt should stop.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StopRule {
    /// Target standard error; stop once the estimate is at least this precise.
    pub se_target: f64,
    /// Never stop before this many items (guards against a two-item pass).
    pub min_items: u32,
    /// Never serve more than this many items.
    pub max_items: u32,
}

impl Default for StopRule {
    fn default() -> Self {
        Self {
            se_target: 0.3,
            min_items: 5,
            max_items: 20,
        }
    }
}

/// Randomesque exposure width: choose uniformly among the top-`k` most
/// informative remaining items. 1 would be pure maximum-information (and
/// maximally leaky); a handful spreads exposure while staying informative.
pub const EXPOSURE_TOP_K: usize = 3;

/// Pick the next item to serve given the current estimate and the set already
/// administered.
///
/// Ranks unadministered items by information at `theta`, then draws uniformly
/// from the top [`EXPOSURE_TOP_K`]. Returns `None` when nothing is left to
/// serve. Deterministic in `rng`, so an attempt replays identically.
pub fn select_next_item<'a>(
    pool: &'a [PoolItem],
    administered: &[String],
    theta: f64,
    rng: &mut SplitMix64,
) -> Option<&'a PoolItem> {
    // Candidates: everything not already served.
    let mut candidates: Vec<&PoolItem> = pool
        .iter()
        .filter(|it| !administered.iter().any(|done| done == &it.id))
        .collect();
    if candidates.is_empty() {
        return None;
    }

    // Rank by information at the current ability, most informative first.
    // Tie-break on id so the ordering is total and reproducible — float
    // information values can tie, and an unstable sort would make the draw
    // depend on input order rather than only on the seed.
    candidates.sort_by(|a, b| {
        b.params
            .information(theta)
            .total_cmp(&a.params.information(theta))
            .then_with(|| a.id.cmp(&b.id))
    });

    let top = candidates.len().min(EXPOSURE_TOP_K);
    let pick = rng.below(top);
    Some(candidates[pick])
}

/// Whether to stop after `items_answered` items given the current estimate.
pub fn should_stop(estimate: &AbilityEstimate, answered: u32, rule: &StopRule) -> bool {
    if answered < rule.min_items {
        return false;
    }
    if answered >= rule.max_items {
        return true;
    }
    estimate.se <= rule.se_target
}

/// Re-estimate ability from the responses gathered so far. Thin wrapper over
/// [`estimate_theta_eap`] that pairs each administered item's parameters with
/// whether it was answered correctly.
pub fn current_estimate(responses: &[(ItemParams, bool)]) -> AbilityEstimate {
    estimate_theta_eap(responses)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pool(n: usize) -> Vec<PoolItem> {
        (0..n)
            .map(|i| PoolItem {
                id: format!("q{i}"),
                params: ItemParams {
                    a: 1.0,
                    b: -3.0 + 6.0 * (i as f64) / (n as f64 - 1.0),
                },
            })
            .collect()
    }

    #[test]
    fn selection_is_deterministic_in_the_seed() {
        let p = pool(20);
        let mut r1 = SplitMix64(42);
        let mut r2 = SplitMix64(42);
        let a = select_next_item(&p, &[], 0.0, &mut r1).unwrap();
        let b = select_next_item(&p, &[], 0.0, &mut r2).unwrap();
        assert_eq!(a.id, b.id);
    }

    #[test]
    fn selection_favours_items_near_the_ability_estimate() {
        // Over many draws at θ=0, the served items should cluster around
        // difficulty 0, not scatter uniformly across the bank.
        let p = pool(21); // difficulties -3..3 in 0.3 steps
        let mut rng = SplitMix64(7);
        let mut near = 0;
        for _ in 0..200 {
            let it = select_next_item(&p, &[], 0.0, &mut rng).unwrap();
            if it.params.b.abs() <= 1.0 {
                near += 1;
            }
        }
        assert!(near > 150, "expected most picks near θ=0, got {near}/200");
    }

    #[test]
    fn selection_never_repeats_an_administered_item() {
        let p = pool(10);
        let mut rng = SplitMix64(1);
        let mut served: Vec<String> = Vec::new();
        for _ in 0..10 {
            let it = select_next_item(&p, &served, 0.0, &mut rng).unwrap();
            assert!(!served.contains(&it.id));
            served.push(it.id.clone());
        }
        // Pool exhausted.
        assert!(select_next_item(&p, &served, 0.0, &mut rng).is_none());
    }

    #[test]
    fn exposure_control_spreads_picks_across_the_top_items() {
        // At a fixed θ the single most-informative item would always win
        // under pure max-info. Randomesque selection must serve more than one
        // distinct item as the first pick across seeds.
        let p = pool(20);
        let mut seen = std::collections::HashSet::new();
        for seed in 0..50 {
            let mut rng = SplitMix64(seed);
            let it = select_next_item(&p, &[], 0.3, &mut rng).unwrap();
            seen.insert(it.id.clone());
        }
        assert!(
            seen.len() >= 2,
            "exposure control should spread first picks, saw {}",
            seen.len()
        );
        assert!(
            seen.len() <= EXPOSURE_TOP_K,
            "picks must stay within the top-k, saw {}",
            seen.len()
        );
    }

    #[test]
    fn stops_only_after_the_minimum_even_if_precise() {
        let rule = StopRule {
            se_target: 0.5,
            min_items: 5,
            max_items: 20,
        };
        let precise = AbilityEstimate {
            theta: 0.0,
            se: 0.1,
        };
        assert!(
            !should_stop(&precise, 3, &rule),
            "must not stop below the floor"
        );
        assert!(
            should_stop(&precise, 5, &rule),
            "may stop once precise past the floor"
        );
    }

    #[test]
    fn stops_at_the_ceiling_even_if_imprecise() {
        let rule = StopRule::default();
        let imprecise = AbilityEstimate {
            theta: 0.0,
            se: 1.0,
        };
        assert!(!should_stop(&imprecise, 10, &rule));
        assert!(should_stop(&imprecise, rule.max_items, &rule));
    }

    #[test]
    fn stops_when_precise_enough_between_the_bounds() {
        let rule = StopRule::default();
        assert!(!should_stop(
            &AbilityEstimate {
                theta: 0.0,
                se: 0.4
            },
            8,
            &rule
        ));
        assert!(should_stop(
            &AbilityEstimate {
                theta: 0.0,
                se: 0.29
            },
            8,
            &rule
        ));
    }

    /// End-to-end: an adaptive attempt against a synthetic learner should
    /// converge to a sensible estimate in fewer items than a full pool, and
    /// order two learners correctly.
    #[test]
    fn an_adaptive_run_converges_and_orders_learners() {
        let p = pool(40);
        let rule = StopRule::default();

        let run = |true_theta: f64, seed: u64| -> (AbilityEstimate, u32) {
            let mut rng = SplitMix64(seed);
            let mut administered: Vec<String> = Vec::new();
            let mut responses: Vec<(ItemParams, bool)> = Vec::new();
            let mut est = AbilityEstimate::prior();
            let mut n = 0u32;
            loop {
                if should_stop(&est, n, &rule) {
                    break;
                }
                let Some(item) = select_next_item(&p, &administered, est.theta, &mut rng) else {
                    break;
                };
                let prob = item.params.p_correct(true_theta);
                let u = (rng.next_u64() >> 11) as f64 / (1u64 << 53) as f64;
                let correct = u < prob;
                administered.push(item.id.clone());
                responses.push((item.params, correct));
                est = current_estimate(&responses);
                n += 1;
            }
            (est, n)
        };

        let (weak, weak_n) = run(-1.2, 100);
        let (strong, strong_n) = run(1.2, 100);

        assert!(
            strong.theta > weak.theta,
            "adaptive run must order learners"
        );
        // Converged inside the ceiling, and used no more than the pool.
        assert!(weak_n <= rule.max_items && strong_n <= rule.max_items);
        assert!(weak_n >= rule.min_items && strong_n >= rule.min_items);
        // Estimates land in the right neighbourhood of the truth.
        assert!((weak.theta - -1.2).abs() < 0.7, "weak θ̂ {} off", weak.theta);
        assert!(
            (strong.theta - 1.2).abs() < 0.7,
            "strong θ̂ {} off",
            strong.theta
        );
    }
}
