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

/// Round timings to whole milliseconds, then derive `speed_ratio` from the
/// rounded values, and assemble the sample.
///
/// The LogNormal/Normal draws go through libm `exp`/`ln`, whose last bits
/// differ between macOS and Linux — which made the raw f32 timings (and
/// therefore the golden hashes) platform-dependent. Keystroke timings are
/// millisecond-resolution anyway, so rounding here collapses that sub-ULP
/// noise and makes the synthetic output bit-identical across platforms.
/// `speed_ratio` is then a pure IEEE division of integer-valued floats,
/// which is deterministic everywhere.
fn finish_sample(
    digraphs: Vec<String>,
    mut dwell_ms: Vec<f32>,
    mut flight_ms: Vec<f32>,
) -> KeystrokeSample {
    for v in dwell_ms.iter_mut() {
        *v = v.round();
    }
    for v in flight_ms.iter_mut() {
        *v = v.round();
    }
    let speed_ratio = finalize_speed_ratios(&dwell_ms, &flight_ms);
    KeystrokeSample {
        digraphs,
        dwell_ms,
        flight_ms,
        speed_ratio,
    }
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
    finish_sample(digraphs, dwell_ms, flight_ms)
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
    finish_sample(digraphs, dwell_ms, flight_ms)
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
    finish_sample(digraphs, dwell_ms, flight_ms)
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

    finish_sample(digraphs, dwell_ms, flight_ms)
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
    finish_sample(digraphs, dwell_ms, flight_ms)
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
    finish_sample(digraphs, dwell_ms, flight_ms)
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
    /// `SYNTH_VERSION = "v2"` for `count=64, seed=42`.
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
            "ab7a410ca65d9f7350d74f7f489b29b6c613ed4a10853aceeda14f96ff1c12e86bfa500d265015e3d35e98d7d2d23a977e879a5c0e9123c9d914ee20f64d211a",
        ),
        (
            AttackLabel::TypingBotConstant,
            "504cde0fec57cd757eac03ae31b49b9578aab6036d31f67b61ce257ba3298c000f479ea7cbb2a77a22f19b429ba85385354c62dd95f9f65d8f57a9461fcb500e",
        ),
        (
            AttackLabel::TypingBotJitter,
            "13440c940712801397ae264713cca9eb1ecff3d63b650e7145a982223477c049963a3c6654228afa3aaccbe9a2428ea26f01b9764fdb146640a7d830045ce19b",
        ),
        (
            AttackLabel::LlmPasteEdit,
            "f1e1f4662299e61681a9410f97476c86593c5dfda09965400243af4c91d5c08d74da9cb1ea02ef8aaa53182e1926c066c43198388b59b441078a2344b0bb616e",
        ),
        (
            AttackLabel::RemoteControl,
            "4f89d7ef1d6c21feee5f102f0cb2850c41cbf07d828da1b6d3f635e6bb234868b885595eabbaaf5e9e01e499be5e85eb556db3a45af95932b839bb0d6335c28d",
        ),
        (
            AttackLabel::HumanBaseline,
            "47311ffb6d1df51021aa73883492f565244f2f4b095af62d8d5d848081fb4273b64f94a1e79f164f12a0651f6be1c18fde9d94d96800b13afd03cb3b87cbb9b0",
        ),
    ];

    #[test]
    fn golden_hashes_match_synth_v2() {
        assert_eq!(
            SYNTH_VERSION, "v2",
            "goldens are pinned to SYNTH_VERSION=v2"
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

    /// Regeneration helper: `cargo test -p alex --bin alex -- --ignored \
    /// --nocapture print_goldens` prints the GOLDEN_HASHES table to paste
    /// above after an intentional SYNTH_VERSION bump.
    #[test]
    #[ignore]
    fn print_goldens() {
        for (label, _) in GOLDEN_HASHES {
            let bytes = serde_json::to_vec(&run(*label, 64, 42)).unwrap();
            println!("{:?} => {}", label, blake2b_hex(&bytes));
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
