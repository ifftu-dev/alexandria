//! ONNX paste / typing-bot classifier — backend inference via tract.
//!
//! The bundled `paste-v1.onnx` is embedded at compile time via
//! `include_bytes!` so there is no runtime filesystem lookup, no asset
//! protocol handshake, and no CSP for WASM to worry about. DAO-swapped
//! weights replace the active session at runtime when
//! `set_dao_session()` is called from the DAO upgrade flow.
//!
//! Replaces the legacy `src/utils/sentinel/paste-classifier.ts` +
//! `onnx-runtime.ts`. Mobile is fully supported because tract is a
//! pure-Rust crate with no WASM/CSP coupling — the `pasteClassifierDisabled`
//! gate that the frontend used to enforce is gone.

use std::io::Cursor;
use std::sync::{Arc, OnceLock, RwLock};

use anyhow::{anyhow, Context, Result};
use tract_onnx::prelude::*;

use super::features::FEATURE_DIM;

/// Embedded fallback weights. Always available; never fails to load
/// outside of catastrophic ONNX-parser regressions.
const BUNDLED_PASTE_V1: &[u8] = include_bytes!("../../resources/sentinel/paste-v1.onnx");

/// Maximum number of ONNX nodes allowed in a DAO-supplied graph. Caps
/// the attack surface for a malicious envelope that ships a giant
/// model just to trigger an OOM on parse / optimize. Our trained MLP
/// has ~10 nodes; setting the bar at 256 gives the DAO room to ship
/// modestly larger architectures (small transformers etc.) without
/// also accepting adversarial bloat.
const MAX_DAO_MODEL_NODES: usize = 256;

/// Maximum size in bytes for an incoming DAO ONNX blob. Pairs with
/// `MAX_WEIGHTS_BYTES` in `sentinel_priors.rs`; the smaller of the two
/// wins. Set conservatively because our bundled artifact is ~5 KB
/// and even a small transformer would be < 5 MiB.
const MAX_DAO_MODEL_BYTES: usize = 50 * 1024 * 1024;

/// `tract`'s `RunnableModel` is `Send + Sync`. Wrap in an `Arc<RwLock<...>>`
/// so the DAO swap path can atomically replace it without blocking
/// in-flight scoring calls for more than a release cycle.
type Runnable = SimplePlan<TypedFact, Box<dyn TypedOp>, Graph<TypedFact, Box<dyn TypedOp>>>;

#[derive(Clone)]
struct LoadedClassifier {
    source: ClassifierSource,
    version: String,
    model: Arc<Runnable>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ClassifierSource {
    Bundled,
    Dao,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct LoadedClassifierInfo {
    pub source: ClassifierSource,
    pub version: String,
}

static CLASSIFIER: OnceLock<RwLock<LoadedClassifier>> = OnceLock::new();

fn build_runnable(bytes: &[u8]) -> Result<Arc<Runnable>> {
    let model = tract_onnx::onnx()
        .model_for_read(&mut Cursor::new(bytes))
        .context("parse ONNX bytes")?
        .with_input_fact(0, f32::fact([1, FEATURE_DIM as i32]).into())
        .context("set input fact [1, FEATURE_DIM]")?
        .into_optimized()
        .context("optimize graph")?
        .into_runnable()
        .context("compile runnable")?;
    Ok(Arc::new(model))
}

fn ensure_initialized() -> &'static RwLock<LoadedClassifier> {
    CLASSIFIER.get_or_init(|| {
        let model = build_runnable(BUNDLED_PASTE_V1)
            .expect("bundled paste-v1.onnx failed to parse — release-blocking");
        RwLock::new(LoadedClassifier {
            source: ClassifierSource::Bundled,
            version: "bundled-v1".to_string(),
            model,
        })
    })
}

/// Score a 12-dim feature vector. Returns a probability in [0,1] where
/// higher means more cheat-like. Errors only on tract internal failure;
/// the bundled model guarantees a usable session always exists.
pub fn score(features: &[f32; FEATURE_DIM]) -> Result<f32> {
    let runnable = {
        let guard = ensure_initialized()
            .read()
            .map_err(|_| anyhow!("classifier rwlock poisoned"))?;
        guard.model.clone()
    };
    let input = tract_ndarray::Array2::from_shape_vec((1, FEATURE_DIM), features.to_vec())
        .context("reshape features to [1,12]")?;
    let result = runnable
        .run(tvec!(Tensor::from(input).into()))
        .context("run inference")?;
    let view = result[0]
        .to_array_view::<f32>()
        .context("read output tensor")?;
    let raw = view
        .iter()
        .next()
        .copied()
        .ok_or_else(|| anyhow!("empty output tensor"))?;
    if !raw.is_finite() {
        return Err(anyhow!("non-finite classifier output: {raw}"));
    }
    Ok(raw.clamp(0.0, 1.0))
}

/// Replace the active session with one built from DAO-supplied ONNX
/// bytes. Caller is responsible for envelope/eval re-verification —
/// this function trusts the *origin* of the bytes but still validates
/// shape + size + node count before handing them to tract.
///
/// Returns `Ok(())` on success; leaves the previous session in place
/// (bundled or earlier DAO) on failure.
pub fn set_dao_session(bytes: &[u8], version: String) -> Result<()> {
    if bytes.len() > MAX_DAO_MODEL_BYTES {
        return Err(anyhow!(
            "DAO model exceeds size cap: {} > {} bytes",
            bytes.len(),
            MAX_DAO_MODEL_BYTES
        ));
    }
    // Build through tract once to count nodes BEFORE swapping into the
    // shared session. A malicious envelope with thousands of ops would
    // already have OOM'd by here; the cap is an additional safety belt.
    let model = build_runnable(bytes)?;
    let node_count = model.model().nodes().len();
    if node_count > MAX_DAO_MODEL_NODES {
        return Err(anyhow!(
            "DAO model exceeds node cap: {} > {}",
            node_count,
            MAX_DAO_MODEL_NODES
        ));
    }
    let mut guard = ensure_initialized()
        .write()
        .map_err(|_| anyhow!("classifier rwlock poisoned"))?;
    *guard = LoadedClassifier {
        source: ClassifierSource::Dao,
        version,
        model,
    };
    Ok(())
}

/// Drop the DAO session and revert to the bundled artifact. Used by
/// the kill switch + rollback paths.
pub fn revert_to_bundled() {
    if let Some(lock) = CLASSIFIER.get() {
        if let Ok(mut guard) = lock.write() {
            if matches!(guard.source, ClassifierSource::Dao) {
                if let Ok(model) = build_runnable(BUNDLED_PASTE_V1) {
                    *guard = LoadedClassifier {
                        source: ClassifierSource::Bundled,
                        version: "bundled-v1".to_string(),
                        model,
                    };
                }
            }
        }
    }
}

pub fn loaded_info() -> LoadedClassifierInfo {
    let guard = ensure_initialized().read().expect("rwlock poisoned");
    LoadedClassifierInfo {
        source: guard.source.clone(),
        version: guard.version.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_model_loads_and_scores() {
        // Cold-load + score on the bundled weights — proves the
        // include_bytes! + tract parse path works end-to-end.
        let features = [0.0_f32; FEATURE_DIM];
        let s = score(&features).expect("score should succeed on bundled model");
        assert!((0.0..=1.0).contains(&s), "score out of [0,1]: {s}");
    }

    #[test]
    fn paste_like_features_score_higher_than_human() {
        let mut paste = [0.0_f32; FEATURE_DIM];
        paste[4] = 1.0; // near-zero-flight frac
        paste[5] = 1.0; // max run / 200
        paste[7] = 0.0; // dwell CV near zero
        paste[8] = 0.0; // flight CV near zero
        paste[11] = 0.6;

        let mut human = [0.0_f32; FEATURE_DIM];
        human[0] = 85.0;
        human[2] = 120.0;
        human[7] = 0.25;
        human[8] = 0.30;
        human[11] = 0.6;

        let paste_score = score(&paste).unwrap();
        let human_score = score(&human).unwrap();
        // Generous bound — model trained on synthetic data; real
        // separation in the training corpus is >> 0.1.
        assert!(
            paste_score > human_score,
            "expected paste_score ({paste_score}) > human_score ({human_score})"
        );
    }

    #[test]
    fn loaded_info_returns_well_formed_state() {
        // The classifier's global state can be flipped to DAO by other
        // tests in this process (cargo runs tests in parallel and the
        // `CLASSIFIER` OnceLock is process-wide). We only assert that
        // loaded_info() returns *something* — the bundled-on-cold-start
        // invariant is covered by `bundled_model_loads_and_scores`.
        let info = loaded_info();
        assert!(!info.version.is_empty());
        assert!(matches!(
            info.source,
            ClassifierSource::Bundled | ClassifierSource::Dao
        ));
    }

    #[test]
    fn set_dao_session_rejects_oversized_bytes() {
        // Hand-craft a buffer that exceeds the size cap so the size
        // check trips before tract even sees it. Contents don't matter.
        let oversized = vec![0_u8; MAX_DAO_MODEL_BYTES + 1];
        let err = set_dao_session(&oversized, "bogus".into()).unwrap_err();
        assert!(
            err.to_string().contains("size cap"),
            "expected size-cap error, got: {err}"
        );
    }

    #[test]
    fn set_dao_session_accepts_bundled_within_node_cap() {
        // The bundled model is our reference for sane node counts.
        // If this fails after a retrain, MAX_DAO_MODEL_NODES likely
        // needs raising to match the new architecture.
        set_dao_session(BUNDLED_PASTE_V1, "test-dao".into())
            .expect("bundled model should pass node cap");
        let info = loaded_info();
        assert_eq!(info.source, ClassifierSource::Dao);
        // Revert so other tests see a clean classifier state.
        revert_to_bundled();
    }

    // ---- End-to-end cheat-test: synthetic streams → features → tract --
    //
    // Drives each of the six attack archetypes (matching the in-app
    // /dashboard/sentinel/cheat-test page) through `extract_paste_features`
    // and `score` against the bundled paste-v1 model. Replaces the
    // legacy vitest `cheat-detection.test.ts` that was deleted with the
    // backend rewrite. Fails loudly if the bundled model loses its
    // ability to distinguish any archetype from `human_baseline`.

    use crate::sentinel::features::{extract_paste_features, PasteFeatureInputs};
    use crate::sentinel::types::KeystrokeEvent;

    fn lcg(seed: u32) -> impl FnMut() -> f32 {
        let mut s = if seed == 0 { 1 } else { seed };
        move || {
            s = s.wrapping_mul(1664525).wrapping_add(1013904223);
            (s as f32) / (u32::MAX as f32)
        }
    }

    fn gauss(rng: &mut impl FnMut() -> f32, mean: f32, std: f32) -> f32 {
        let u1 = rng().max(1e-9);
        let u2 = rng();
        mean + std * (-2.0 * u1.ln()).sqrt() * (2.0 * std::f32::consts::PI * u2).cos()
    }

    #[derive(Copy, Clone, Debug)]
    enum Archetype {
        PasteMacro,
        TypingBotConstant,
        TypingBotJitter,
        LlmPasteEdit,
        RemoteControl,
        HumanBaseline,
    }

    fn generate_stream(arch: Archetype, count: usize, seed: u32) -> Vec<KeystrokeEvent> {
        let mut r = lcg(seed);
        let mut out = Vec::with_capacity(count);
        match arch {
            Archetype::PasteMacro => {
                for _ in 0..count {
                    out.push(KeystrokeEvent {
                        key: "char".into(),
                        dwell_ms: r() * 3.0,
                        flight_ms: r() * 2.0,
                    });
                }
            }
            Archetype::TypingBotConstant => {
                for _ in 0..count {
                    out.push(KeystrokeEvent {
                        key: "char".into(),
                        dwell_ms: (50.0 + gauss(&mut r, 0.0, 3.0)).max(0.0),
                        flight_ms: (80.0 + gauss(&mut r, 0.0, 3.0)).max(0.0),
                    });
                }
            }
            Archetype::TypingBotJitter => {
                for _ in 0..count {
                    out.push(KeystrokeEvent {
                        key: "char".into(),
                        dwell_ms: gauss(&mut r, 85.0, 25.0).max(1.0),
                        flight_ms: gauss(&mut r, 110.0, 35.0).max(1.0),
                    });
                }
            }
            Archetype::LlmPasteEdit => {
                let paste_n = (count as f32 * 0.8) as usize;
                for _ in 0..paste_n {
                    out.push(KeystrokeEvent {
                        key: "char".into(),
                        dwell_ms: r() * 3.0,
                        flight_ms: r() * 2.0,
                    });
                }
                for _ in paste_n..count {
                    out.push(KeystrokeEvent {
                        key: "char".into(),
                        dwell_ms: gauss(&mut r, 120.0, 40.0).clamp(20.0, 400.0),
                        flight_ms: gauss(&mut r, 250.0, 90.0).clamp(50.0, 800.0),
                    });
                }
            }
            Archetype::RemoteControl => {
                for _ in 0..count {
                    out.push(KeystrokeEvent {
                        key: "char".into(),
                        dwell_ms: gauss(&mut r, 85.0, 20.0).max(1.0),
                        flight_ms: gauss(&mut r, 180.0, 80.0).max(10.0),
                    });
                }
            }
            Archetype::HumanBaseline => {
                for i in 0..count {
                    let freq = if i % 4 == 0 { 1.0 } else { 1.2 };
                    out.push(KeystrokeEvent {
                        key: "char".into(),
                        dwell_ms: (gauss(&mut r, 85.0, 18.0) * freq).max(30.0),
                        flight_ms: (gauss(&mut r, 130.0, 35.0) * freq).max(40.0),
                    });
                }
            }
        }
        out
    }

    fn score_archetype(arch: Archetype, seed: u32) -> f32 {
        let events = generate_stream(arch, 120, seed);
        let (paste_events, pasted_chars) = match arch {
            Archetype::PasteMacro => (1, events.len() as u32),
            Archetype::LlmPasteEdit => (1, (events.len() as f32 * 0.8) as u32),
            _ => (0, 0),
        };
        let features = extract_paste_features(&PasteFeatureInputs {
            keystrokes: &events,
            paste_event_count: paste_events,
            pasted_char_count: pasted_chars,
            window_ms: 30_000.0,
        });
        score(&features).expect("tract score failed")
    }

    #[test]
    fn cheat_test_paste_macro_scores_high() {
        let s = score_archetype(Archetype::PasteMacro, 42);
        assert!(
            s > 0.5,
            "paste_macro should score > 0.5 on bundled model, got {s}"
        );
    }

    #[test]
    fn cheat_test_typing_bot_constant_scores_high() {
        let s = score_archetype(Archetype::TypingBotConstant, 42);
        assert!(s > 0.5, "typing_bot_constant should score > 0.5, got {s}");
    }

    #[test]
    fn cheat_test_llm_paste_edit_scores_high() {
        let s = score_archetype(Archetype::LlmPasteEdit, 42);
        // Hardest archetype — bound is slightly looser.
        assert!(s > 0.4, "llm_paste_edit should score > 0.4, got {s}");
    }

    #[test]
    fn cheat_test_human_baseline_below_warning_threshold() {
        // The bundled model was trained on Buffalo-statistics human
        // samples; this LCG/Gauss-generated plausibly-human noise is
        // out-of-distribution by construction. We only require that
        // it stays below the `paste_classifier_anomaly` warning gate
        // (0.95) — i.e. wouldn't auto-flag a session. Real-world FPR
        // measurement is tracked separately (see sentinel-federation.md
        // §12 "Real-world holdout evaluation" row).
        let s = score_archetype(Archetype::HumanBaseline, 42);
        assert!(
            s < 0.95,
            "human_baseline crossed warning gate (0.95), got {s}"
        );
    }

    #[test]
    fn cheat_test_snapshot_latency_under_budget() {
        // P0 #4 — keep snapshot-time inference well under the 10 ms
        // soft budget so the three-IPC-per-snapshot dispatch in
        // useSentinel doesn't push real users past 50 ms total.
        // Run 20 iterations and report the worst case.
        let events = generate_stream(Archetype::HumanBaseline, 120, 1);
        let inputs = PasteFeatureInputs {
            keystrokes: &events,
            paste_event_count: 0,
            pasted_char_count: 0,
            window_ms: 30_000.0,
        };
        let mut worst = std::time::Duration::ZERO;
        for _ in 0..20 {
            let t0 = std::time::Instant::now();
            let features = extract_paste_features(&inputs);
            let _ = score(&features).unwrap();
            let dt = t0.elapsed();
            if dt > worst {
                worst = dt;
            }
        }
        // 10 ms is the soft budget; assert at 20 ms to absorb CI
        // noise. Real wall-clock on a modern Mac runs <1 ms.
        assert!(
            worst < std::time::Duration::from_millis(20),
            "paste classifier snapshot path exceeded 20ms budget: {worst:?}"
        );
    }

    #[test]
    fn cheat_test_attacks_score_above_human() {
        // Aggregate guarantee: every attack archetype out-scores
        // human baseline. If this fails after a retrain, FPR is
        // about to spike in production.
        let human = score_archetype(Archetype::HumanBaseline, 42);
        for arch in [
            Archetype::PasteMacro,
            Archetype::TypingBotConstant,
            Archetype::TypingBotJitter,
            Archetype::LlmPasteEdit,
            Archetype::RemoteControl,
        ] {
            let attack = score_archetype(arch, 42);
            assert!(
                attack > human,
                "{:?} ({attack}) failed to exceed human ({human})",
                arch
            );
        }
    }
}
