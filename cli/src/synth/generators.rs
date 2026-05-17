//! Attack-class + human-baseline sample generators.
//!
//! Each `AttackLabel` corresponds to one cheat archetype documented in
//! the plan (see docs/sentinel-adversarial-priors.md). Distributions
//! are chosen so the classifier learns the structural difference
//! between the class and `HumanBaseline`, not surface-level statistics
//! a rule already covers:
//!
//! - `PasteMacro`        — instant burst: dwell≈0, flight≈0
//! - `TypingBotConstant` — fixed cadence ± micro jitter, uniform across digraphs
//! - `TypingBotJitter`   — human-like Gaussian timings, missing digraph correlation
//! - `LlmPasteEdit`      — 80% paste burst, then 20% sparse human edits
//! - `RemoteControl`     — normal dwell, high flight variance (network jitter)
//! - `HumanBaseline`     — log-normal timings, speed correlates with bigram frequency

use rand::Rng;
use rand_chacha::ChaCha20Rng;
use rand_distr::{Distribution, LogNormal, Normal};

use super::bigrams;
use super::blob::{KeystrokeSample, PriorBlob, SCHEMA_VERSION, SYNTH_VERSION};
use super::rng::rng_from_seed;

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum AttackLabel {
    PasteMacro,
    TypingBotConstant,
    TypingBotJitter,
    LlmPasteEdit,
    RemoteControl,
    HumanBaseline,
}

impl AttackLabel {
    pub fn as_str(self) -> &'static str {
        match self {
            AttackLabel::PasteMacro => "paste_macro",
            AttackLabel::TypingBotConstant => "typing_bot_constant",
            AttackLabel::TypingBotJitter => "typing_bot_jitter",
            AttackLabel::LlmPasteEdit => "llm_paste_edit",
            AttackLabel::RemoteControl => "remote_control",
            AttackLabel::HumanBaseline => "human_baseline",
        }
    }

    pub fn notes(self) -> &'static str {
        match self {
            AttackLabel::PasteMacro => "Single zero-time burst of keystrokes — clipboard paste.",
            AttackLabel::TypingBotConstant => "Constant cadence with micro-jitter; uniform digraph speeds.",
            AttackLabel::TypingBotJitter => "Human-like Gaussian noise but no digraph-frequency correlation.",
            AttackLabel::LlmPasteEdit => "Paste burst followed by sparse human-style edit keystrokes.",
            AttackLabel::RemoteControl => "Normal dwell, high flight variance from RDP/VNC network jitter.",
            AttackLabel::HumanBaseline => "Human typing baseline; log-normal timings correlated with English bigram frequency.",
        }
    }
}

pub struct GenerateOptions {
    pub label: AttackLabel,
    pub count: usize,
    pub seed: u64,
}

pub fn generate_label(opts: &GenerateOptions) -> PriorBlob {
    let mut rng = rng_from_seed(opts.seed);
    let mut samples = Vec::with_capacity(opts.count);
    for _ in 0..opts.count {
        let sample = match opts.label {
            AttackLabel::PasteMacro => gen_paste_macro(&mut rng),
            AttackLabel::TypingBotConstant => gen_typing_bot_constant(&mut rng),
            AttackLabel::TypingBotJitter => gen_typing_bot_jitter(&mut rng),
            AttackLabel::LlmPasteEdit => gen_llm_paste_edit(&mut rng),
            AttackLabel::RemoteControl => gen_remote_control(&mut rng),
            AttackLabel::HumanBaseline => gen_human_baseline(&mut rng),
        };
        samples.push(sample);
    }

    PriorBlob {
        schema_version: SCHEMA_VERSION,
        model_kind: "keystroke".to_string(),
        label: opts.label.as_str().to_string(),
        synth_seed: opts.seed,
        synth_version: SYNTH_VERSION.to_string(),
        notes: opts.label.notes().to_string(),
        samples,
    }
}

// ============================================================================
// Per-label generators
// ============================================================================

fn sample_len(rng: &mut ChaCha20Rng, mean: f32, std: f32, min: usize, max: usize) -> usize {
    let normal = Normal::new(mean, std).expect("valid params");
    let raw = normal.sample(rng).round() as i64;
    raw.clamp(min as i64, max as i64) as usize
}

fn random_bigram(rng: &mut ChaCha20Rng) -> &'static str {
    let u: f32 = rng.gen();
    bigrams::sample_weighted(u)
}

fn finalize_speed_ratios(dwell_ms: &[f32], flight_ms: &[f32]) -> Vec<f32> {
    // speed_ratio mirrors the TS extractor: dwellMs2 / (dwellMs1 + flightMs1),
    // clamped to [0,5] so an outlier doesn't blow up downstream features.
    let n = dwell_ms.len();
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        if i == 0 {
            out.push(1.0);
            continue;
        }
        let prev = dwell_ms[i - 1] + flight_ms[i - 1];
        let v = if prev > 0.0 { dwell_ms[i] / prev } else { 1.0 };
        out.push(v.clamp(0.0, 5.0));
    }
    out
}

fn gen_paste_macro(rng: &mut ChaCha20Rng) -> KeystrokeSample {
    let n = sample_len(rng, 150.0, 30.0, 50, 200);
    let mut digraphs = Vec::with_capacity(n);
    let mut dwell_ms = Vec::with_capacity(n);
    let mut flight_ms = Vec::with_capacity(n);
    let dwell_d = rand_distr::Uniform::new(0.0_f32, 3.0);
    let flight_d = rand_distr::Uniform::new(0.0_f32, 2.0);
    for _ in 0..n {
        digraphs.push(random_bigram(rng).to_string());
        dwell_ms.push(dwell_d.sample(rng));
        flight_ms.push(flight_d.sample(rng));
    }
    let speed_ratio = finalize_speed_ratios(&dwell_ms, &flight_ms);
    KeystrokeSample {
        digraphs,
        dwell_ms,
        flight_ms,
        speed_ratio,
    }
}

fn gen_typing_bot_constant(rng: &mut ChaCha20Rng) -> KeystrokeSample {
    let n = sample_len(rng, 120.0, 30.0, 50, 200);
    let dwell_jitter = Normal::new(50.0_f32, 3.0).expect("valid");
    let flight_jitter = Normal::new(80.0_f32, 3.0).expect("valid");
    let mut digraphs = Vec::with_capacity(n);
    let mut dwell_ms = Vec::with_capacity(n);
    let mut flight_ms = Vec::with_capacity(n);
    for _ in 0..n {
        digraphs.push(random_bigram(rng).to_string());
        dwell_ms.push(dwell_jitter.sample(rng).max(0.0));
        flight_ms.push(flight_jitter.sample(rng).max(0.0));
    }
    let speed_ratio = finalize_speed_ratios(&dwell_ms, &flight_ms);
    KeystrokeSample {
        digraphs,
        dwell_ms,
        flight_ms,
        speed_ratio,
    }
}

fn gen_typing_bot_jitter(rng: &mut ChaCha20Rng) -> KeystrokeSample {
    // Plausible human-like Gaussian, but means are scrambled — every digraph
    // gets a per-blob constant mean rather than a frequency-correlated one.
    // This is the hard case: surface stats match humans, structure doesn't.
    let n = sample_len(rng, 120.0, 30.0, 50, 200);
    let dwell_d = Normal::new(85.0_f32, 25.0).expect("valid");
    let flight_d = Normal::new(110.0_f32, 35.0).expect("valid");
    let mut digraphs = Vec::with_capacity(n);
    let mut dwell_ms = Vec::with_capacity(n);
    let mut flight_ms = Vec::with_capacity(n);
    for _ in 0..n {
        digraphs.push(random_bigram(rng).to_string());
        dwell_ms.push(dwell_d.sample(rng).max(1.0));
        flight_ms.push(flight_d.sample(rng).max(1.0));
    }
    let speed_ratio = finalize_speed_ratios(&dwell_ms, &flight_ms);
    KeystrokeSample {
        digraphs,
        dwell_ms,
        flight_ms,
        speed_ratio,
    }
}

fn gen_llm_paste_edit(rng: &mut ChaCha20Rng) -> KeystrokeSample {
    // 80% paste burst, 20% sparse human-style edits.
    let n = sample_len(rng, 160.0, 30.0, 50, 200);
    let paste_n = (n as f32 * 0.8) as usize;
    let edit_n = n - paste_n;

    let mut digraphs = Vec::with_capacity(n);
    let mut dwell_ms = Vec::with_capacity(n);
    let mut flight_ms = Vec::with_capacity(n);

    let paste_dwell = rand_distr::Uniform::new(0.0_f32, 3.0);
    let paste_flight = rand_distr::Uniform::new(0.0_f32, 2.0);
    for _ in 0..paste_n {
        digraphs.push(random_bigram(rng).to_string());
        dwell_ms.push(paste_dwell.sample(rng));
        flight_ms.push(paste_flight.sample(rng));
    }

    // Sparse edits — log-normal timings like a tired human checking output.
    let edit_dwell = LogNormal::new(4.5_f32, 0.3).expect("valid");
    let edit_flight = LogNormal::new(5.2_f32, 0.5).expect("valid");
    for _ in 0..edit_n {
        digraphs.push(random_bigram(rng).to_string());
        dwell_ms.push(edit_dwell.sample(rng).clamp(20.0, 400.0));
        flight_ms.push(edit_flight.sample(rng).clamp(50.0, 800.0));
    }

    let speed_ratio = finalize_speed_ratios(&dwell_ms, &flight_ms);
    KeystrokeSample {
        digraphs,
        dwell_ms,
        flight_ms,
        speed_ratio,
    }
}

fn gen_remote_control(rng: &mut ChaCha20Rng) -> KeystrokeSample {
    // Normal dwell distribution, but flight time is dominated by network
    // jitter. The classifier should pick this up via flight CV (std/mean).
    let n = sample_len(rng, 100.0, 30.0, 50, 200);
    let dwell_d = Normal::new(85.0_f32, 20.0).expect("valid");
    let flight_d = Normal::new(180.0_f32, 80.0).expect("valid");
    let mut digraphs = Vec::with_capacity(n);
    let mut dwell_ms = Vec::with_capacity(n);
    let mut flight_ms = Vec::with_capacity(n);
    for _ in 0..n {
        digraphs.push(random_bigram(rng).to_string());
        dwell_ms.push(dwell_d.sample(rng).max(1.0));
        flight_ms.push(flight_d.sample(rng).max(10.0));
    }
    let speed_ratio = finalize_speed_ratios(&dwell_ms, &flight_ms);
    KeystrokeSample {
        digraphs,
        dwell_ms,
        flight_ms,
        speed_ratio,
    }
}

fn gen_human_baseline(rng: &mut ChaCha20Rng) -> KeystrokeSample {
    // Baseline distribution params chosen to roughly match published
    // typing dynamics datasets (Buffalo Keystroke etc.). The critical
    // signal — *frequency-correlated speeds* — is encoded by scaling
    // dwell/flight inversely with the bigram's English frequency.
    let n = sample_len(rng, 140.0, 30.0, 60, 200);
    let base_dwell = LogNormal::new(4.4_f32, 0.3).expect("valid");
    let base_flight = LogNormal::new(4.7_f32, 0.4).expect("valid");
    let mut digraphs = Vec::with_capacity(n);
    let mut dwell_ms = Vec::with_capacity(n);
    let mut flight_ms = Vec::with_capacity(n);

    for _ in 0..n {
        let bg = random_bigram(rng);
        let freq = bigrams::frequency(bg);
        // Higher-frequency bigrams resolve faster (scale factor in [0.6, 1.4]
        // around 1.0). The transform stays monotonic so the rank-order
        // correlation with frequency is preserved.
        let scale = 1.4_f32 - (freq.clamp(0.0, 4.0) / 4.0) * 0.8;
        let dwell = (base_dwell.sample(rng) * scale).clamp(30.0, 350.0);
        let flight = (base_flight.sample(rng) * scale).clamp(40.0, 600.0);
        digraphs.push(bg.to_string());
        dwell_ms.push(dwell);
        flight_ms.push(flight);
    }
    let speed_ratio = finalize_speed_ratios(&dwell_ms, &flight_ms);
    KeystrokeSample {
        digraphs,
        dwell_ms,
        flight_ms,
        speed_ratio,
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::synth::rng::blake2b_hex;

    fn run(label: AttackLabel, count: usize, seed: u64) -> PriorBlob {
        generate_label(&GenerateOptions { label, count, seed })
    }

    #[test]
    fn determinism_same_seed_same_bytes() {
        // Same seed twice → byte-identical JSON. If this fails after a
        // distribution change, bump SYNTH_VERSION and update the expected
        // hashes in the consuming model's eval artifact.
        for label in [
            AttackLabel::PasteMacro,
            AttackLabel::TypingBotConstant,
            AttackLabel::TypingBotJitter,
            AttackLabel::LlmPasteEdit,
            AttackLabel::RemoteControl,
            AttackLabel::HumanBaseline,
        ] {
            let a = serde_json::to_vec(&run(label, 8, 7)).unwrap();
            let b = serde_json::to_vec(&run(label, 8, 7)).unwrap();
            assert_eq!(blake2b_hex(&a), blake2b_hex(&b), "label={:?}", label);
        }
    }

    /// Golden hashes pinning the synthetic data distributions at
    /// `SYNTH_VERSION = "v1"` for `count=64, seed=42`.
    ///
    /// If a distribution parameter, the RNG sequence, the JSON
    /// serialization order, or the bigram table changes, this test
    /// FAILS LOUDLY. That's intentional — any such drift silently
    /// invalidates every trained model. Bump `SYNTH_VERSION` AND
    /// regenerate the goldens together when the drift is intentional.
    ///
    /// Recompute via:
    ///   for L in paste-macro typing-bot-constant typing-bot-jitter \
    ///            llm-paste-edit remote-control human-baseline; do
    ///     cargo run -p alex -- synth-sentinel generate \
    ///       --label $L --count 64 --seed 42 --out /tmp/g-$L.json
    ///     openssl dgst -blake2b512 /tmp/g-$L.json
    ///   done
    const GOLDEN_HASHES: &[(AttackLabel, &str)] = &[
        (
            AttackLabel::PasteMacro,
            "09f94b21ba0c51a0fba303e1f823e79cb864aaf7b80c2b637b2f14b396aea4d1242640fc748a05b92387ab35298ebfa5ebcd008a3d792a5aa8863246d91a4243",
        ),
        (
            AttackLabel::TypingBotConstant,
            "73e17c3eaadc06809b6cda8f4793c29a96cab500e08eaa7bed4e53e8d64d773b98d9e09fd17ba1027948b9b8b997637ccce788ea50504f04c42cf8c8d0a11560",
        ),
        (
            AttackLabel::TypingBotJitter,
            "a44241f49c40f414580f5e2c204a424830a425934ba97575d60b3b49eba74de18c931c1229878f0b85e0181cfee780762fc5b2272ad1ade7591bd9ff5f4a03b2",
        ),
        (
            AttackLabel::LlmPasteEdit,
            "56585a6e41527c77b961ee077ae40bcafc81a17de5f46750207776ca8aefef4629e6414e63c7b69b1de9c797819451fc48e05dd25c3a2c15f272784572d2397c",
        ),
        (
            AttackLabel::RemoteControl,
            "a2c6e476e9706fca4874854c462e40f1f0cf8656699f026b3cbd1d32115d95f69c1559dc76dad2ae1e32aa2e53552a7bd79f8cd2a94be77bf005a7d5187d696c",
        ),
        (
            AttackLabel::HumanBaseline,
            "dbe99a0ba389ba76203026f5e21ddaf511b4ea25b3c77447a824696471b8157ecefddc3ae3ed86d27243284e0c4018e9070b410b8bcefc7b6167af0e99649b49",
        ),
    ];

    #[test]
    fn golden_hashes_match_synth_v1() {
        assert_eq!(
            SYNTH_VERSION, "v1",
            "goldens are pinned to SYNTH_VERSION=v1"
        );
        for (label, expected) in GOLDEN_HASHES {
            let bytes = serde_json::to_vec(&run(*label, 64, 42)).unwrap();
            let actual = blake2b_hex(&bytes);
            assert_eq!(
                &actual, expected,
                "golden drift for {:?} — regenerate after bumping SYNTH_VERSION",
                label,
            );
        }
    }

    #[test]
    fn different_seeds_diverge() {
        let a = run(AttackLabel::HumanBaseline, 8, 1);
        let b = run(AttackLabel::HumanBaseline, 8, 2);
        assert_ne!(
            serde_json::to_string(&a).unwrap(),
            serde_json::to_string(&b).unwrap(),
        );
    }

    #[test]
    fn no_nan_or_inf_anywhere() {
        for label in [
            AttackLabel::PasteMacro,
            AttackLabel::TypingBotConstant,
            AttackLabel::TypingBotJitter,
            AttackLabel::LlmPasteEdit,
            AttackLabel::RemoteControl,
            AttackLabel::HumanBaseline,
        ] {
            let blob = run(label, 32, 123);
            for s in &blob.samples {
                assert_eq!(s.digraphs.len(), s.dwell_ms.len());
                assert_eq!(s.digraphs.len(), s.flight_ms.len());
                assert_eq!(s.digraphs.len(), s.speed_ratio.len());
                for v in s.dwell_ms.iter().chain(&s.flight_ms).chain(&s.speed_ratio) {
                    assert!(v.is_finite(), "non-finite value in label {:?}", label);
                    assert!(*v >= 0.0, "negative value in label {:?}", label);
                }
            }
        }
    }

    #[test]
    fn typing_bot_constant_has_low_velocity_variance() {
        let blob = run(AttackLabel::TypingBotConstant, 200, 42);
        // Bot CV should be tight (under 0.15 for both dwell and flight). If
        // this drifts above 0.20 the distribution is too noisy to be useful.
        let mut all_dwell: Vec<f32> = vec![];
        let mut all_flight: Vec<f32> = vec![];
        for s in &blob.samples {
            all_dwell.extend(&s.dwell_ms);
            all_flight.extend(&s.flight_ms);
        }
        assert!(cv(&all_dwell) < 0.20);
        assert!(cv(&all_flight) < 0.20);
    }

    #[test]
    fn human_baseline_correlates_with_bigram_frequency() {
        // Frequent bigrams (th, he) should resolve faster than rare ones
        // (qz, jx). Validates the digraph-speed correlation that the
        // classifier learns to look for.
        let blob = run(AttackLabel::HumanBaseline, 500, 7);
        let mut frequent: Vec<f32> = vec![];
        let mut rare: Vec<f32> = vec![];
        for s in &blob.samples {
            for (i, bg) in s.digraphs.iter().enumerate() {
                let total = s.dwell_ms[i] + s.flight_ms[i];
                if matches!(bg.as_str(), "th" | "he" | "in" | "er" | "an") {
                    frequent.push(total);
                } else if matches!(bg.as_str(), "qx" | "zj" | "qz" | "jx" | "vq") {
                    rare.push(total);
                }
            }
        }
        let mf = mean(&frequent);
        let mr = mean(&rare);
        assert!(
            mf < mr,
            "expected frequent bigrams faster than rare; got mf={mf:.1} mr={mr:.1}",
        );
    }

    #[test]
    fn paste_macro_max_flight_under_5ms() {
        let blob = run(AttackLabel::PasteMacro, 50, 99);
        for s in &blob.samples {
            for f in &s.flight_ms {
                assert!(*f < 5.0, "paste flight should be near-zero, got {f}");
            }
        }
    }

    fn mean(xs: &[f32]) -> f32 {
        if xs.is_empty() {
            return 0.0;
        }
        xs.iter().sum::<f32>() / xs.len() as f32
    }

    fn cv(xs: &[f32]) -> f32 {
        let m = mean(xs);
        if m.abs() < 1e-3 {
            return 0.0;
        }
        let var = xs.iter().map(|v| (v - m).powi(2)).sum::<f32>() / xs.len() as f32;
        var.sqrt() / m
    }
}
