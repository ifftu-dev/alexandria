//! Backend IPC surface for the Sentinel ML pipeline.
//!
//! Replaces every direct TS ML call. The frontend now buffers raw
//! events and invokes these commands at snapshot time / training time.
//! See `docs/sentinel.md` §AI Models for the full contract.

use anyhow::Context;
use rusqlite::params;
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::sentinel::features::{extract_paste_features, PasteFeatureInputs};
use crate::sentinel::keystroke_ae::{
    extract_digraph_features, AutoencoderWeights, KeystrokeAutoencoder,
};
use crate::sentinel::mouse_cnn::{MouseCnnWeights, MouseTrajectoryCnn};
use crate::sentinel::paste_classifier::{self, ClassifierSource, LoadedClassifierInfo};
use crate::sentinel::types::{DigraphFeatures, KeystrokeEvent, MousePoint};
use crate::AppState;

// ============================================================================
// Paste classifier — features + tract inference
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct ScorePasteRequest {
    pub events: Vec<KeystrokeEvent>,
    pub paste_event_count: u32,
    pub pasted_char_count: u32,
    pub window_ms: f32,
}

#[derive(Debug, Serialize)]
pub struct ScorePasteResponse {
    pub features: Vec<f32>,
    pub score: f32,
    pub classifier: LoadedClassifierInfo,
}

/// Extract paste features and score them through the active classifier
/// (bundled or DAO-swapped). Returns the feature vector too so the
/// frontend can surface it in the Sentinel dashboard cheat-test view.
///
/// Latency budget: end-to-end < 10 ms for a typical 100-event snapshot
/// (feature extraction is O(n), tract inference is constant-time on
/// the 12 → 32 → 16 → 1 MLP). Emits `log::trace` timing so an operator
/// running `RUST_LOG=sentinel=trace` can verify per-stage cost.
#[tauri::command]
pub async fn sentinel_score_paste(req: ScorePasteRequest) -> Result<ScorePasteResponse, String> {
    let t0 = std::time::Instant::now();
    let inputs = PasteFeatureInputs {
        keystrokes: &req.events,
        paste_event_count: req.paste_event_count,
        pasted_char_count: req.pasted_char_count,
        window_ms: req.window_ms,
    };
    let features = extract_paste_features(&inputs);
    let t_features = t0.elapsed();
    let score = paste_classifier::score(&features).map_err(|e| e.to_string())?;
    let t_total = t0.elapsed();
    log::trace!(
        target: "sentinel",
        "score_paste: events={} feat={}µs total={}µs score={:.3}",
        req.events.len(),
        t_features.as_micros(),
        t_total.as_micros(),
        score,
    );
    Ok(ScorePasteResponse {
        features: features.to_vec(),
        score,
        classifier: paste_classifier::loaded_info(),
    })
}

/// Inspect the currently loaded paste-classifier source + version.
#[tauri::command]
pub async fn sentinel_paste_classifier_info() -> LoadedClassifierInfo {
    paste_classifier::loaded_info()
}

#[derive(Debug, Deserialize)]
pub struct LoadDaoClassifierRequest {
    pub bytes: Vec<u8>,
    pub version: String,
}

/// Load DAO-supplied ONNX bytes into the active session.
///
/// The bytes must hash to a `weights_cid` the Sentinel DAO has ratified for
/// this `version`. This used to be documented as the *caller's* job — "caller
/// is responsible for envelope/eval verification" — with the frontend fetching
/// bytes over the network and handing them straight in.
///
/// That is the wrong place for the check, twice over. It defers a security
/// decision across an IPC boundary to code in another language, so anything
/// that can call the IPC surface skips it entirely. And what it feeds is an
/// ONNX parser: `tract-nnef` carries RUSTSEC-2026-0217, an integer overflow in
/// the tensor parser giving an out-of-bounds read on model load, and the fix
/// cannot be taken here because tract 0.21.16 pins `half =2.4.1` while the
/// media stack needs `half ^2.5`. So the parser must not see bytes nobody
/// vouched for.
///
/// The hash comparison is the whole gate: a ratified `weights_cid` is a BLAKE3
/// of the exact artifact the DAO voted on.
#[tauri::command]
pub async fn sentinel_load_dao_classifier(
    state: State<'_, AppState>,
    req: LoadDaoClassifierRequest,
) -> Result<LoadedClassifierInfo, String> {
    // 50 MiB cap matches MAX_WEIGHTS_BYTES in sentinel_priors.rs.
    const MAX_BYTES: usize = 50 * 1024 * 1024;
    if req.bytes.len() > MAX_BYTES {
        return Err(format!(
            "DAO weights blob too large: {} > {} bytes",
            req.bytes.len(),
            MAX_BYTES
        ));
    }

    let computed = blake3::hash(&req.bytes).to_hex().to_string();
    {
        let db_guard = state
            .db
            .lock()
            .map_err(|_| "database lock poisoned".to_string())?;
        let db = db_guard.as_ref().ok_or("database not initialized")?;
        let ratified: i64 = db
            .conn()
            .query_row(
                // `cid` is the BLAKE3 of the artifact the DAO voted on. Every
                // row in this table is ratified — `ratified_at` is NOT NULL
                // with a default — so presence is the check.
                "SELECT COUNT(*) FROM sentinel_priors \
                 WHERE model_kind = 'paste_classifier_weights' \
                   AND cid = ?1",
                params![&computed],
                |r| r.get(0),
            )
            .map_err(|e| format!("checking ratified priors: {e}"))?;
        if ratified == 0 {
            return Err(format!(
                "refusing to load classifier weights: BLAKE3 {computed} is not a \
                 ratified Sentinel DAO artifact"
            ));
        }
    }

    paste_classifier::set_dao_session(&req.bytes, req.version).map_err(|e| e.to_string())?;
    Ok(paste_classifier::loaded_info())
}

/// Drop the DAO session and revert to the bundled artifact. Used by
/// the kill-switch + version-blocklist response paths.
#[tauri::command]
pub async fn sentinel_revert_classifier_to_bundled() -> LoadedClassifierInfo {
    paste_classifier::revert_to_bundled();
    paste_classifier::loaded_info()
}

// ============================================================================
// Keystroke autoencoder — per-user candle training + scoring
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct TrainKeystrokeAeRequest {
    pub user_address: String,
    pub device_fp_prefix: String,
    pub events: Vec<KeystrokeEvent>,
    #[serde(default)]
    pub epochs: Option<usize>,
    #[serde(default)]
    pub negative_digraphs: Vec<DigraphFeatures>,
}

#[derive(Debug, Serialize)]
pub struct TrainKeystrokeAeResponse {
    pub train_loss: f32,
    pub training_samples: usize,
    pub trained_epochs: usize,
}

const DEFAULT_AE_EPOCHS: usize = 80;
const KEYSTROKE_AE_KIND: &str = "keystroke_ae";
const MOUSE_CNN_KIND: &str = "mouse_cnn";

#[tauri::command]
pub async fn sentinel_train_keystroke_ae(
    state: State<'_, AppState>,
    req: TrainKeystrokeAeRequest,
) -> Result<TrainKeystrokeAeResponse, String> {
    let mut ae = match load_user_model::<AutoencoderWeights>(
        &state,
        &req.user_address,
        &req.device_fp_prefix,
        KEYSTROKE_AE_KIND,
    )? {
        Some(w) => KeystrokeAutoencoder::from_weights(&w).map_err(|e| e.to_string())?,
        None => KeystrokeAutoencoder::new().map_err(|e| e.to_string())?,
    };
    let epochs = req.epochs.unwrap_or(DEFAULT_AE_EPOCHS);
    let loss = ae
        .train(&req.events, epochs, &req.negative_digraphs)
        .map_err(|e| e.to_string())?;
    let weights = ae.export_weights().map_err(|e| e.to_string())?;
    save_user_model(
        &state,
        &req.user_address,
        &req.device_fp_prefix,
        KEYSTROKE_AE_KIND,
        &weights,
        ae.train_loss(),
        weights.trained_epochs,
        weights.training_samples,
    )?;
    Ok(TrainKeystrokeAeResponse {
        train_loss: loss,
        training_samples: weights.training_samples,
        trained_epochs: weights.trained_epochs,
    })
}

#[derive(Debug, Deserialize)]
pub struct ScoreKeystrokeAeRequest {
    pub user_address: String,
    pub device_fp_prefix: String,
    pub events: Vec<KeystrokeEvent>,
}

/// Score keystroke events against the user's trained autoencoder.
/// Returns `-1.0` if no model is trained yet (mirrors the legacy TS
/// contract for "advisory signal currently unavailable").
#[tauri::command]
pub async fn sentinel_score_keystroke_ae(
    state: State<'_, AppState>,
    req: ScoreKeystrokeAeRequest,
) -> Result<f32, String> {
    let t0 = std::time::Instant::now();
    let Some(weights) = load_user_model::<AutoencoderWeights>(
        &state,
        &req.user_address,
        &req.device_fp_prefix,
        KEYSTROKE_AE_KIND,
    )?
    else {
        return Ok(-1.0);
    };
    let ae = KeystrokeAutoencoder::from_weights(&weights).map_err(|e| e.to_string())?;
    let score = ae.score(&req.events).map_err(|e| e.to_string())?;
    log::trace!(
        target: "sentinel",
        "score_keystroke_ae: events={} total={}µs score={:.3}",
        req.events.len(),
        t0.elapsed().as_micros(),
        score,
    );
    Ok(score)
}

/// Extract digraph features without scoring. Useful when the frontend
/// wants to preview what the AE would consume.
#[tauri::command]
pub async fn sentinel_extract_digraphs(
    events: Vec<KeystrokeEvent>,
) -> Result<Vec<DigraphFeatures>, String> {
    Ok(extract_digraph_features(&events))
}

// ============================================================================
// Mouse trajectory CNN
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct TrainMouseCnnRequest {
    pub user_address: String,
    pub device_fp_prefix: String,
    pub points: Vec<MousePoint>,
    #[serde(default)]
    pub epochs: Option<usize>,
}

#[derive(Debug, Serialize)]
pub struct TrainMouseCnnResponse {
    pub train_loss: f32,
    pub training_samples: usize,
    pub trained_epochs: usize,
}

const DEFAULT_CNN_EPOCHS: usize = 80;

#[tauri::command]
pub async fn sentinel_train_mouse_cnn(
    state: State<'_, AppState>,
    req: TrainMouseCnnRequest,
) -> Result<TrainMouseCnnResponse, String> {
    let mut cnn = match load_user_model::<MouseCnnWeights>(
        &state,
        &req.user_address,
        &req.device_fp_prefix,
        MOUSE_CNN_KIND,
    )? {
        Some(w) => MouseTrajectoryCnn::from_weights(&w).map_err(|e| e.to_string())?,
        None => MouseTrajectoryCnn::new().map_err(|e| e.to_string())?,
    };
    // Hard-coded synthetic bot segments (constant velocity, teleport,
    // jittered line, sine wave, instant) mirror the legacy fallback
    // when DAO-ratified mouse priors haven't synced yet.
    let bots = synthetic_bot_segments();
    let epochs = req.epochs.unwrap_or(DEFAULT_CNN_EPOCHS);
    let loss = cnn
        .train(&req.points, &bots, epochs)
        .map_err(|e| e.to_string())?;
    let weights = cnn.export_weights().map_err(|e| e.to_string())?;
    save_user_model(
        &state,
        &req.user_address,
        &req.device_fp_prefix,
        MOUSE_CNN_KIND,
        &weights,
        weights.train_loss,
        weights.trained_epochs,
        weights.training_samples,
    )?;
    Ok(TrainMouseCnnResponse {
        train_loss: loss,
        training_samples: weights.training_samples,
        trained_epochs: weights.trained_epochs,
    })
}

#[derive(Debug, Deserialize)]
pub struct ScoreMouseCnnRequest {
    pub user_address: String,
    pub device_fp_prefix: String,
    pub points: Vec<MousePoint>,
}

#[tauri::command]
pub async fn sentinel_score_mouse_cnn(
    state: State<'_, AppState>,
    req: ScoreMouseCnnRequest,
) -> Result<f32, String> {
    let t0 = std::time::Instant::now();
    let Some(weights) = load_user_model::<MouseCnnWeights>(
        &state,
        &req.user_address,
        &req.device_fp_prefix,
        MOUSE_CNN_KIND,
    )?
    else {
        return Ok(-1.0);
    };
    let cnn = MouseTrajectoryCnn::from_weights(&weights).map_err(|e| e.to_string())?;
    let prob = cnn.predict(&req.points).map_err(|e| e.to_string())?;
    log::trace!(
        target: "sentinel",
        "score_mouse_cnn: points={} total={}µs human_prob={:.3}",
        req.points.len(),
        t0.elapsed().as_micros(),
        prob,
    );
    Ok(prob)
}

// ============================================================================
// Storage helpers
// ============================================================================

pub(crate) fn load_user_model<W: for<'de> Deserialize<'de>>(
    state: &State<'_, AppState>,
    user_address: &str,
    device_fp_prefix: &str,
    model_kind: &str,
) -> Result<Option<W>, String> {
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "db lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("db not initialized")?;
    let json: Option<String> = db
        .conn()
        .query_row(
            "SELECT weights_json FROM sentinel_user_models
             WHERE user_address = ?1 AND device_fp_prefix = ?2 AND model_kind = ?3",
            params![user_address, device_fp_prefix, model_kind],
            |row| row.get(0),
        )
        .or_else(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            other => Err(other),
        })
        .map_err(|e| e.to_string())?;
    match json {
        Some(s) => Ok(Some(
            serde_json::from_str::<W>(&s)
                .with_context(|| format!("parse {model_kind} weights"))
                .map_err(|e| e.to_string())?,
        )),
        None => Ok(None),
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn save_user_model<W: Serialize>(
    state: &State<'_, AppState>,
    user_address: &str,
    device_fp_prefix: &str,
    model_kind: &str,
    weights: &W,
    train_loss: f32,
    trained_epochs: usize,
    training_samples: usize,
) -> Result<(), String> {
    let json = serde_json::to_string(weights).map_err(|e| e.to_string())?;
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "db lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("db not initialized")?;
    let now = chrono::Utc::now().to_rfc3339();
    db.conn()
        .execute(
            "INSERT INTO sentinel_user_models
                 (user_address, device_fp_prefix, model_kind, weights_json,
                  train_loss, trained_epochs, training_samples, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(user_address, device_fp_prefix, model_kind) DO UPDATE SET
                 weights_json = excluded.weights_json,
                 train_loss = excluded.train_loss,
                 trained_epochs = excluded.trained_epochs,
                 training_samples = excluded.training_samples,
                 updated_at = excluded.updated_at",
            params![
                user_address,
                device_fp_prefix,
                model_kind,
                json,
                train_loss,
                trained_epochs as i64,
                training_samples as i64,
                now,
            ],
        )
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[derive(Debug, Serialize)]
pub struct UserModelStatus {
    pub model_kind: String,
    pub trained_epochs: i64,
    pub training_samples: i64,
    pub train_loss: Option<f64>,
    pub updated_at: String,
}

#[tauri::command]
pub async fn sentinel_user_models_status(
    state: State<'_, AppState>,
    user_address: String,
    device_fp_prefix: String,
) -> Result<Vec<UserModelStatus>, String> {
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "db lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("db not initialized")?;
    let conn = db.conn();
    let mut stmt = conn
        .prepare(
            "SELECT model_kind, trained_epochs, training_samples, train_loss, updated_at
             FROM sentinel_user_models
             WHERE user_address = ?1 AND device_fp_prefix = ?2
             ORDER BY model_kind",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![user_address, device_fp_prefix], |row| {
            Ok(UserModelStatus {
                model_kind: row.get(0)?,
                trained_epochs: row.get(1)?,
                training_samples: row.get(2)?,
                train_loss: row.get(3)?,
                updated_at: row.get(4)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(rows)
}

#[tauri::command]
pub async fn sentinel_reset_user_models(
    state: State<'_, AppState>,
    user_address: String,
    device_fp_prefix: String,
) -> Result<(), String> {
    let db_guard = state
        .db
        .lock()
        .map_err(|_| "db lock poisoned".to_string())?;
    let db = db_guard.as_ref().ok_or("db not initialized")?;
    db.conn()
        .execute(
            "DELETE FROM sentinel_user_models
             WHERE user_address = ?1 AND device_fp_prefix = ?2",
            params![user_address, device_fp_prefix],
        )
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn synthetic_bot_segments() -> Vec<[f32; 50 * 3]> {
    // Mirrors `mouse-trajectory-cnn.ts` synthetic negatives. Each
    // archetype tests a different "bot-shaped" failure mode the dense
    // head should learn to flag.
    let mut out = Vec::new();
    // Constant velocity right.
    let mut s = [0.0_f32; 50 * 3];
    for i in 0..50 {
        s[i * 3] = 0.5;
        s[i * 3 + 1] = 0.0;
        s[i * 3 + 2] = 1.0;
    }
    out.push(s);
    // Sine wave.
    let mut s = [0.0_f32; 50 * 3];
    for i in 0..50 {
        s[i * 3] = (i as f32 * 0.3).sin() * 0.5;
        s[i * 3 + 1] = (i as f32 * 0.3).cos() * 0.5;
        s[i * 3 + 2] = 1.0;
    }
    out.push(s);
    // Jittered straight line.
    let mut s = [0.0_f32; 50 * 3];
    for i in 0..50 {
        s[i * 3] = 0.3 + ((i % 3) as f32 - 1.0) * 0.05;
        s[i * 3 + 1] = 0.0;
        s[i * 3 + 2] = 1.0;
    }
    out.push(s);
    // Teleport (one big jump, zeros otherwise).
    let mut s = [0.0_f32; 50 * 3];
    s[25 * 3] = 1.0;
    s[25 * 3 + 1] = 1.0;
    s[25 * 3 + 2] = 0.01;
    out.push(s);
    // Linear interpolation diagonal.
    let mut s = [0.0_f32; 50 * 3];
    for i in 0..50 {
        s[i * 3] = 0.4;
        s[i * 3 + 1] = 0.4;
        s[i * 3 + 2] = 1.0;
    }
    out.push(s);

    // Silence the false "field never read" lint from rustc — the
    // `ClassifierSource` enum has variants we don't pattern-match here.
    let _ = ClassifierSource::Bundled;
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The DAO-classifier loader feeds an ONNX parser that carries a live
    /// out-of-bounds-read advisory (RUSTSEC-2026-0217) which cannot be patched
    /// here — tract 0.21.16 pins `half =2.4.1` against the media stack's
    /// `half ^2.5`. So the gate is that the bytes must hash to something the
    /// Sentinel DAO ratified. Unratified bytes must never reach the parser.
    #[test]
    fn unratified_weights_are_refused_before_parsing() {
        let db = crate::db::Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();

        let bytes = b"not a ratified model".to_vec();
        let computed = blake3::hash(&bytes).to_hex().to_string();

        let ratified: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM sentinel_priors \
                 WHERE model_kind = 'paste_classifier_weights' AND cid = ?1",
                params![&computed],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            ratified, 0,
            "arbitrary bytes must not match a ratified prior"
        );
    }
}
