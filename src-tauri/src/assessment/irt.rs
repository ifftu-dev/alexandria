//! Item response theory: estimating a learner's ability from their answers.
//!
//! A fixed-form quiz reports a fraction correct, which conflates *how many*
//! items were answered with *how hard* they were — ten easy items right is
//! not the ten hard items right, but both score 1.0. IRT separates the two:
//! each item has a difficulty and a discrimination, and a learner has an
//! ability `θ` on the same scale, so the estimate accounts for which items
//! were actually administered.
//!
//! # The model: 2PL
//!
//! The two-parameter logistic model. The probability that a learner of
//! ability `θ` answers an item correctly is
//!
//! ```text
//! P(correct | θ, a, b) = 1 / (1 + e^{-a (θ − b)})
//! ```
//!
//! where `b` is the item's difficulty (the `θ` at which a correct answer is
//! 50/50) and `a` is its discrimination (how sharply the item separates
//! learners around `b`). We deliberately skip the 3PL guessing parameter:
//! multi-select MCQ makes blind guessing a weak strategy, and `c` needs far
//! more response data to calibrate than a young deployment has.
//!
//! # Estimation: EAP, not MLE
//!
//! Ability is estimated by **expected a posteriori** — the mean of the
//! posterior over `θ` under a standard-normal prior, evaluated on a fixed
//! quadrature grid. EAP is chosen over maximum likelihood on purpose: MLE
//! diverges to ±∞ for an all-correct or all-incorrect response pattern, and
//! short assessments produce those constantly. EAP stays finite and returns
//! a sensible, prior-shrunk estimate with a standard error in every case,
//! including zero responses.
//!
//! The posterior standard deviation is the estimate's standard error, from
//! which a confidence interval falls out directly:
//! `θ̂ ± 1.96·SE`.

/// Item parameters under the 2PL model.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ItemParams {
    /// Discrimination. Higher means the item separates learners more sharply
    /// around its difficulty. Positive for any sane item.
    pub a: f64,
    /// Difficulty on the ability scale: the `θ` at which the item is 50/50.
    pub b: f64,
}

impl ItemParams {
    /// Seed parameters from the legacy 1–5 difficulty column, so every bank
    /// works before any real calibration exists.
    ///
    /// Maps difficulty linearly onto the ability scale — 3 is average
    /// (`b = 0`), 1 and 5 sit roughly ±1.6 away — with a fixed, moderate
    /// discrimination. These are deliberately uncalibrated but sane; real
    /// `(a, b)` from response data replace them later (Phase 2).
    pub fn from_difficulty(difficulty: u8) -> Self {
        let d = difficulty.clamp(1, 5) as f64;
        Self {
            a: 1.0,
            b: (d - 3.0) * 0.8,
        }
    }

    /// P(correct | θ) under 2PL.
    pub fn p_correct(&self, theta: f64) -> f64 {
        1.0 / (1.0 + (-self.a * (theta - self.b)).exp())
    }

    /// Fisher information this item carries about `θ`: `a² · P · (1 − P)`.
    ///
    /// Maximal where `θ = b`, and the quantity adaptive delivery maximises
    /// when choosing the next item — the item that tells us the most about a
    /// learner is the one whose difficulty sits nearest their current
    /// ability estimate.
    pub fn information(&self, theta: f64) -> f64 {
        let p = self.p_correct(theta);
        self.a * self.a * p * (1.0 - p)
    }
}

/// The ability scale is treated as effectively bounded to ±4 SD of the
/// standard-normal prior; beyond that the prior mass is negligible and
/// including it only wastes grid points.
const THETA_MAX: f64 = 4.0;
/// Quadrature resolution. 81 points over [−4, 4] is a 0.1 grid — fine enough
/// that the EAP mean and SD are stable to well under the SE we ever report.
const GRID_POINTS: usize = 81;

/// An ability estimate: the posterior mean and its standard error.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AbilityEstimate {
    /// Posterior mean of `θ`.
    pub theta: f64,
    /// Posterior standard deviation — the standard error of the estimate.
    pub se: f64,
}

impl AbilityEstimate {
    /// The prior with no evidence: standard normal, so mean 0 and SE 1.
    pub fn prior() -> Self {
        Self {
            theta: 0.0,
            se: 1.0,
        }
    }

    /// A 95% credible interval, `θ̂ ± 1.96·SE`.
    pub fn interval_95(&self) -> (f64, f64) {
        (self.theta - 1.96 * self.se, self.theta + 1.96 * self.se)
    }
}

/// Standard-normal density (unnormalised constant dropped — EAP normalises).
fn normal_prior(theta: f64) -> f64 {
    (-0.5 * theta * theta).exp()
}

/// Estimate ability by EAP over `responses`, each an `(item, correct)` pair.
///
/// Total and finite for every input: no responses returns the prior; an
/// all-correct or all-incorrect pattern returns a prior-shrunk estimate
/// rather than diverging, which is the whole reason EAP is used here.
pub fn estimate_theta_eap(responses: &[(ItemParams, bool)]) -> AbilityEstimate {
    // No evidence is exactly the prior — and skips the grid, which would only
    // reproduce it up to truncation and discretisation error.
    if responses.is_empty() {
        return AbilityEstimate::prior();
    }

    // Fixed grid over the support of the prior.
    let step = 2.0 * THETA_MAX / (GRID_POINTS as f64 - 1.0);

    let mut sum_w = 0.0; // Σ posterior
    let mut sum_wt = 0.0; // Σ θ·posterior
    let mut sum_wtt = 0.0; // Σ θ²·posterior

    for k in 0..GRID_POINTS {
        let theta = -THETA_MAX + step * k as f64;

        // Posterior ∝ prior(θ) · likelihood(θ). Accumulate the log-likelihood
        // to avoid underflow when many items multiply together.
        let mut log_lik = 0.0;
        for (item, correct) in responses {
            let p = item.p_correct(theta).clamp(1e-12, 1.0 - 1e-12);
            log_lik += if *correct { p.ln() } else { (1.0 - p).ln() };
        }

        let w = normal_prior(theta) * log_lik.exp();
        sum_w += w;
        sum_wt += w * theta;
        sum_wtt += w * theta * theta;
    }

    // Degenerate guard: if the grid mass underflowed to zero (should not
    // happen with the clamp above), fall back to the prior.
    if sum_w <= 0.0 || !sum_w.is_finite() {
        return AbilityEstimate::prior();
    }

    let mean = sum_wt / sum_w;
    let var = (sum_wtt / sum_w - mean * mean).max(0.0);
    AbilityEstimate {
        theta: mean,
        se: var.sqrt(),
    }
}

/// Map an ability estimate onto the `[0, 1]` score scale the rest of the
/// system speaks (credentials, aggregation), via the probability that this
/// learner answers an average-difficulty, average-discrimination item
/// correctly.
///
/// This is a reporting convenience, not part of estimation: it lets an
/// adaptive attempt hand back a score comparable to a fixed-form fraction
/// without committing the credential to the `θ` scale.
pub fn theta_to_score(theta: f64) -> f64 {
    ItemParams { a: 1.0, b: 0.0 }.p_correct(theta)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assessment::SplitMix64;

    fn approx(a: f64, b: f64, eps: f64) -> bool {
        (a - b).abs() < eps
    }

    #[test]
    fn probability_is_half_at_the_difficulty() {
        let item = ItemParams { a: 1.5, b: 0.7 };
        assert!(approx(item.p_correct(0.7), 0.5, 1e-9));
    }

    #[test]
    fn probability_rises_with_ability() {
        let item = ItemParams { a: 1.0, b: 0.0 };
        assert!(item.p_correct(-2.0) < item.p_correct(0.0));
        assert!(item.p_correct(0.0) < item.p_correct(2.0));
        assert!(item.p_correct(-4.0) > 0.0 && item.p_correct(4.0) < 1.0);
    }

    #[test]
    fn information_peaks_at_the_difficulty() {
        // The basis of adaptive item selection: an item is most informative
        // for a learner whose ability sits at its difficulty.
        let item = ItemParams { a: 1.2, b: 0.5 };
        let at_b = item.information(0.5);
        assert!(at_b > item.information(-1.5));
        assert!(at_b > item.information(2.5));
    }

    #[test]
    fn difficulty_bootstrap_is_ordered_and_centred() {
        let easy = ItemParams::from_difficulty(1);
        let mid = ItemParams::from_difficulty(3);
        let hard = ItemParams::from_difficulty(5);
        assert!(easy.b < mid.b && mid.b < hard.b);
        assert!(approx(mid.b, 0.0, 1e-9), "difficulty 3 should be average");
        // Out-of-range clamps rather than producing absurd difficulties.
        assert_eq!(ItemParams::from_difficulty(0), easy);
        assert_eq!(ItemParams::from_difficulty(9), hard);
    }

    #[test]
    fn no_responses_returns_the_prior() {
        let est = estimate_theta_eap(&[]);
        assert!(approx(est.theta, 0.0, 1e-9));
        assert!(approx(est.se, 1.0, 1e-6));
    }

    #[test]
    fn all_correct_pattern_stays_finite_and_positive() {
        // The case that breaks MLE: every item right. EAP must return a
        // finite, above-average estimate, not +∞.
        let items: Vec<(ItemParams, bool)> = (0..8)
            .map(|_| (ItemParams { a: 1.2, b: 0.0 }, true))
            .collect();
        let est = estimate_theta_eap(&items);
        assert!(est.theta.is_finite());
        assert!(
            est.theta > 0.5,
            "all-correct should estimate high, got {}",
            est.theta
        );
        assert!(est.se.is_finite() && est.se > 0.0);
    }

    #[test]
    fn all_incorrect_pattern_stays_finite_and_negative() {
        let items: Vec<(ItemParams, bool)> = (0..8)
            .map(|_| (ItemParams { a: 1.2, b: 0.0 }, false))
            .collect();
        let est = estimate_theta_eap(&items);
        assert!(est.theta.is_finite());
        assert!(
            est.theta < -0.5,
            "all-incorrect should estimate low, got {}",
            est.theta
        );
    }

    #[test]
    fn more_items_shrink_the_standard_error() {
        // Information accumulates: a longer test pins ability more tightly.
        let one = estimate_theta_eap(&[(ItemParams { a: 1.0, b: 0.0 }, true)]);
        let many: Vec<(ItemParams, bool)> = (0..20)
            .map(|i| (ItemParams { a: 1.0, b: 0.0 }, i % 2 == 0))
            .collect();
        let long = estimate_theta_eap(&many);
        assert!(long.se < one.se, "more items should reduce SE");
    }

    #[test]
    fn confidence_interval_brackets_the_estimate() {
        let est = AbilityEstimate {
            theta: 0.4,
            se: 0.3,
        };
        let (lo, hi) = est.interval_95();
        assert!(lo < est.theta && est.theta < hi);
        assert!(approx(hi - lo, 2.0 * 1.96 * 0.3, 1e-9));
    }

    #[test]
    fn theta_maps_monotonically_onto_the_score_scale() {
        assert!(theta_to_score(-2.0) < theta_to_score(0.0));
        assert!(theta_to_score(0.0) < theta_to_score(2.0));
        assert!(approx(theta_to_score(0.0), 0.5, 1e-9));
        assert!((0.0..=1.0).contains(&theta_to_score(3.0)));
    }

    /// Recovery simulation — the test that licenses using these estimates.
    ///
    /// Generate synthetic learners at known abilities, simulate responses to
    /// a fixed item pool under the model, estimate ability back, and check
    /// the estimate tracks the truth. Deterministic via SplitMix64 so it is
    /// reproducible; uses response counts large enough that EAP's prior
    /// shrinkage toward zero is small.
    #[test]
    fn eap_recovers_known_ability() {
        // A spread of item difficulties, so a learner anywhere on the scale
        // meets some informative items.
        let pool: Vec<ItemParams> = (0..40)
            .map(|i| ItemParams {
                a: 1.2,
                b: -2.0 + 4.0 * (i as f64) / 39.0,
            })
            .collect();

        let mut rng = SplitMix64(0xD15EA5E);

        for &true_theta in &[-1.5f64, -0.5, 0.0, 0.5, 1.5] {
            // Average the estimate over several simulated learners at this
            // ability to smooth out sampling noise in a single response set.
            let mut theta_sum = 0.0;
            let trials = 30;
            for _ in 0..trials {
                let responses: Vec<(ItemParams, bool)> = pool
                    .iter()
                    .map(|item| {
                        let p = item.p_correct(true_theta);
                        // Uniform in [0,1) from the deterministic PRNG.
                        let u = (rng.next_u64() >> 11) as f64 / (1u64 << 53) as f64;
                        (*item, u < p)
                    })
                    .collect();
                theta_sum += estimate_theta_eap(&responses).theta;
            }
            let mean_est = theta_sum / trials as f64;
            assert!(
                approx(mean_est, true_theta, 0.25),
                "θ={true_theta}: recovered mean {mean_est} too far off"
            );
        }
    }

    #[test]
    fn estimate_orders_learners_correctly() {
        // Two learners answering the same items: the one who got the harder
        // ones right must estimate higher.
        let items = [
            ItemParams { a: 1.0, b: -1.0 },
            ItemParams { a: 1.0, b: 0.0 },
            ItemParams { a: 1.0, b: 1.0 },
        ];
        let weak = estimate_theta_eap(&[(items[0], true), (items[1], false), (items[2], false)]);
        let strong = estimate_theta_eap(&[(items[0], true), (items[1], true), (items[2], true)]);
        assert!(strong.theta > weak.theta);
    }
}
