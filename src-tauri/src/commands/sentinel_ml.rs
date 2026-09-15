//! Backend IPC surface for the Sentinel ML pipeline.
//!
//! Replaces every direct TS ML call. The frontend now buffers raw
//! events and invokes these commands at snapshot time / training time.
//! See `docs/sentinel.md` §AI Models for the full contract.

use crate::profile::scope::ProfileState as State;
use anyhow::Context;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::db::executor::DatabaseWorkload;
use crate::sentinel::features::{extract_paste_features, PasteFeatureInputs};
use crate::sentinel::keystroke_ae::{
    extract_digraph_features, AutoencoderWeights, KeystrokeAutoencoder,
};
use crate::sentinel::mouse_cnn::{MouseCnnWeights, MouseTrajectoryCnn};
use crate::sentinel::paste_classifier::{self, LoadedClassifierInfo};
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

/// Extract paste features and score them through the bundled classifier.
/// Returns the feature vector too so the
/// frontend can surface it in the Sentinel dashboard cheat-test view.
///
/// Latency budget: end-to-end < 10 ms for a typical 100-event snapshot
/// (feature extraction is O(n), tract inference is constant-time on
/// the 12 → 32 → 16 → 1 MLP). Emits `log::trace` timing so an operator
/// running `RUST_LOG=sentinel=trace` can verify per-stage cost.
#[tauri::command]
pub async fn sentinel_score_paste(
    _profile: crate::profile::scope::ProfileLease,
    req: ScorePasteRequest,
) -> Result<ScorePasteResponse, String> {
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
pub async fn sentinel_paste_classifier_info(
    _profile: crate::profile::scope::ProfileLease,
) -> LoadedClassifierInfo {
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
    let existing = load_user_model::<AutoencoderWeights>(
        &state,
        &req.user_address,
        &req.device_fp_prefix,
        KEYSTROKE_AE_KIND,
    )
    .await?;
    let epochs = req.epochs.unwrap_or(DEFAULT_AE_EPOCHS);
    let events = req.events;
    let negative_digraphs = req.negative_digraphs;
    let (loss, weights) = tokio::task::spawn_blocking(move || {
        let mut ae = match existing {
            Some(weights) => {
                KeystrokeAutoencoder::from_weights(&weights).map_err(|e| e.to_string())?
            }
            None => KeystrokeAutoencoder::new().map_err(|e| e.to_string())?,
        };
        let loss = ae
            .train(&events, epochs, &negative_digraphs)
            .map_err(|e| e.to_string())?;
        let weights = ae.export_weights().map_err(|e| e.to_string())?;
        Ok::<_, String>((loss, weights))
    })
    .await
    .map_err(|e| format!("keystroke training worker failed: {e}"))??;
    save_user_model(
        &state,
        &req.user_address,
        &req.device_fp_prefix,
        KEYSTROKE_AE_KIND,
        &weights,
        weights.train_loss,
        weights.trained_epochs,
        weights.training_samples,
    )
    .await?;
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
    )
    .await?
    else {
        return Ok(-1.0);
    };
    let event_count = req.events.len();
    let score = tokio::task::spawn_blocking(move || {
        let ae = KeystrokeAutoencoder::from_weights(&weights).map_err(|e| e.to_string())?;
        ae.score(&req.events).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("keystroke scoring worker failed: {e}"))??;
    log::trace!(
        target: "sentinel",
        "score_keystroke_ae: events={} total={}µs score={:.3}",
        event_count,
        t0.elapsed().as_micros(),
        score,
    );
    Ok(score)
}

/// Extract digraph features without scoring. Useful when the frontend
/// wants to preview what the AE would consume.
#[tauri::command]
pub async fn sentinel_extract_digraphs(
    _profile: crate::profile::scope::ProfileLease,
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
    let existing = load_user_model::<MouseCnnWeights>(
        &state,
        &req.user_address,
        &req.device_fp_prefix,
        MOUSE_CNN_KIND,
    )
    .await?;
    let epochs = req.epochs.unwrap_or(DEFAULT_CNN_EPOCHS);
    let points = req.points;
    let (loss, weights) = tokio::task::spawn_blocking(move || {
        let mut cnn = match existing {
            Some(weights) => {
                MouseTrajectoryCnn::from_weights(&weights).map_err(|e| e.to_string())?
            }
            None => MouseTrajectoryCnn::new().map_err(|e| e.to_string())?,
        };
        // Hard-coded synthetic bot segments (constant velocity, teleport,
        // jittered line, sine wave, instant) mirror the legacy fallback.
        let bots = synthetic_bot_segments();
        let loss = cnn
            .train(&points, &bots, epochs)
            .map_err(|e| e.to_string())?;
        let weights = cnn.export_weights().map_err(|e| e.to_string())?;
        Ok::<_, String>((loss, weights))
    })
    .await
    .map_err(|e| format!("mouse training worker failed: {e}"))??;
    save_user_model(
        &state,
        &req.user_address,
        &req.device_fp_prefix,
        MOUSE_CNN_KIND,
        &weights,
        weights.train_loss,
        weights.trained_epochs,
        weights.training_samples,
    )
    .await?;
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
    )
    .await?
    else {
        return Ok(-1.0);
    };
    let point_count = req.points.len();
    let prob = tokio::task::spawn_blocking(move || {
        let cnn = MouseTrajectoryCnn::from_weights(&weights).map_err(|e| e.to_string())?;
        cnn.predict(&req.points).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("mouse scoring worker failed: {e}"))??;
    log::trace!(
        target: "sentinel",
        "score_mouse_cnn: points={} total={}µs human_prob={:.3}",
        point_count,
        t0.elapsed().as_micros(),
        prob,
    );
    Ok(prob)
}

// ============================================================================
// Storage helpers
// ============================================================================

const BEHAVIORAL_PROFILE_KIND: &str = "behavioral_profile_v1";
const DEVICE_FINGERPRINT_HEX_LEN: usize = 64;
const DEVICE_FINGERPRINT_PREFIX_LEN: usize = 16;
const FACE_EMBEDDING_DIM: usize = 944;
const MAX_BEHAVIORAL_PROFILE_JSON_BYTES: usize = 64 * 1024;
const MAX_SAFE_JAVASCRIPT_INTEGER: u64 = 9_007_199_254_740_991;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BehavioralProfile {
    pub user_id: String,
    pub device_fingerprint: String,
    pub typing_pattern: TypingPattern,
    pub mouse_pattern: MousePattern,
    pub last_updated: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ai_models: Option<BehavioralAiModels>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TypingPattern {
    pub avg_dwell_time: f64,
    pub avg_flight_time: f64,
    pub speed_wpm: f64,
    pub sample_count: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MousePattern {
    pub avg_velocity: f64,
    pub avg_acceleration: f64,
    pub click_precision: f64,
    pub sample_count: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BehavioralAiModels {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub face_enrollment: Option<FaceEnrollment>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FaceEnrollment {
    pub vector: Vec<f64>,
    pub frame_count: u64,
    pub updated_at: u64,
}

fn local_user_address(conn: &Connection) -> Result<String, String> {
    conn.query_row(
        "SELECT stake_address FROM local_identity WHERE id = 1",
        [],
        |row| row.get(0),
    )
    .map_err(|error| format!("active profile identity is unavailable: {error}"))
}

fn validate_device_fingerprint(value: &str) -> Result<(), String> {
    if value.len() != DEVICE_FINGERPRINT_HEX_LEN
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err("device fingerprint must be 64 lowercase hexadecimal characters".into());
    }
    Ok(())
}

fn validate_device_fingerprint_prefix(value: &str) -> Result<(), String> {
    if value.len() != DEVICE_FINGERPRINT_PREFIX_LEN
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err("device fingerprint prefix must be 16 lowercase hexadecimal characters".into());
    }
    Ok(())
}

fn validate_nonnegative_finite(label: &str, value: f64) -> Result<(), String> {
    if !value.is_finite() || value < 0.0 {
        return Err(format!("{label} must be a finite non-negative number"));
    }
    Ok(())
}

fn validate_behavioral_profile(
    profile: &BehavioralProfile,
    expected_user: &str,
) -> Result<(), String> {
    if profile.user_id != expected_user {
        return Err("behavioral profile does not belong to the active identity".into());
    }
    validate_device_fingerprint(&profile.device_fingerprint)?;
    validate_nonnegative_finite("average dwell time", profile.typing_pattern.avg_dwell_time)?;
    validate_nonnegative_finite(
        "average flight time",
        profile.typing_pattern.avg_flight_time,
    )?;
    validate_nonnegative_finite("typing speed", profile.typing_pattern.speed_wpm)?;
    if profile.typing_pattern.sample_count > MAX_SAFE_JAVASCRIPT_INTEGER
        || profile.mouse_pattern.sample_count > MAX_SAFE_JAVASCRIPT_INTEGER
        || profile.last_updated > MAX_SAFE_JAVASCRIPT_INTEGER
    {
        return Err(
            "behavioral profile contains an integer that JavaScript cannot represent".into(),
        );
    }
    validate_nonnegative_finite("average mouse velocity", profile.mouse_pattern.avg_velocity)?;
    validate_nonnegative_finite(
        "average mouse acceleration",
        profile.mouse_pattern.avg_acceleration,
    )?;
    if !profile.mouse_pattern.click_precision.is_finite()
        || !(0.0..=1.0).contains(&profile.mouse_pattern.click_precision)
    {
        return Err("mouse click precision must be between zero and one".into());
    }
    if let Some(face) = profile
        .ai_models
        .as_ref()
        .and_then(|models| models.face_enrollment.as_ref())
    {
        if face.vector.len() != FACE_EMBEDDING_DIM {
            return Err(format!(
                "face enrollment must contain exactly {FACE_EMBEDDING_DIM} values"
            ));
        }
        if face.frame_count == 0 {
            return Err("face enrollment frame count must be positive".into());
        }
        if face.frame_count > MAX_SAFE_JAVASCRIPT_INTEGER
            || face.updated_at > MAX_SAFE_JAVASCRIPT_INTEGER
        {
            return Err(
                "face enrollment contains an integer that JavaScript cannot represent".into(),
            );
        }
        if face
            .vector
            .iter()
            .any(|value| !value.is_finite() || !(-1.0..=1.0).contains(value))
        {
            return Err("face enrollment contains an invalid value".into());
        }
    }
    Ok(())
}

fn load_behavioral_profile(
    conn: &Connection,
    user_address: &str,
    device_fp_prefix: &str,
) -> Result<Option<BehavioralProfile>, String> {
    let local_user = local_user_address(conn)?;
    if user_address != local_user {
        return Err("behavioral profile request does not match the active identity".into());
    }
    validate_device_fingerprint_prefix(device_fp_prefix)?;
    let json = conn
        .query_row(
            "SELECT weights_json FROM sentinel_user_models
             WHERE user_address = ?1 AND device_fp_prefix = ?2 AND model_kind = ?3",
            params![user_address, device_fp_prefix, BEHAVIORAL_PROFILE_KIND],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    let Some(json) = json else {
        return Ok(None);
    };
    if json.len() > MAX_BEHAVIORAL_PROFILE_JSON_BYTES {
        return Err("stored behavioral profile exceeds its size limit".into());
    }
    let profile: BehavioralProfile = serde_json::from_str(&json)
        .map_err(|error| format!("stored behavioral profile is invalid: {error}"))?;
    validate_behavioral_profile(&profile, &local_user)?;
    if !profile.device_fingerprint.starts_with(device_fp_prefix) {
        return Err("stored behavioral profile has the wrong device fingerprint".into());
    }
    Ok(Some(profile))
}

fn save_behavioral_profile(conn: &Connection, profile: &BehavioralProfile) -> Result<(), String> {
    let local_user = local_user_address(conn)?;
    validate_behavioral_profile(profile, &local_user)?;
    let device_fp_prefix = &profile.device_fingerprint[..DEVICE_FINGERPRINT_PREFIX_LEN];
    let json = serde_json::to_string(profile).map_err(|error| error.to_string())?;
    if json.len() > MAX_BEHAVIORAL_PROFILE_JSON_BYTES {
        return Err("behavioral profile exceeds its size limit".into());
    }
    conn.execute(
        "INSERT INTO sentinel_user_models
             (user_address, device_fp_prefix, model_kind, weights_json,
              train_loss, trained_epochs, training_samples, updated_at)
         VALUES (?1, ?2, ?3, ?4, NULL, 0, 0, datetime('now'))
         ON CONFLICT(user_address, device_fp_prefix, model_kind) DO UPDATE SET
             weights_json = excluded.weights_json,
             train_loss = NULL,
             trained_epochs = 0,
             training_samples = 0,
             updated_at = excluded.updated_at",
        params![
            profile.user_id,
            device_fp_prefix,
            BEHAVIORAL_PROFILE_KIND,
            json
        ],
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

/// Load the active learner's device-local behavioral baseline and face
/// embedding from the profile's SQLCipher database.
#[tauri::command]
pub async fn sentinel_load_behavioral_profile(
    state: State<'_, AppState>,
    user_address: String,
    device_fp_prefix: String,
) -> Result<Option<BehavioralProfile>, String> {
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "sentinel.behavioral-profile.load",
            move |db| load_behavioral_profile(db.conn(), &user_address, &device_fp_prefix),
        )
        .await
}

/// Store the active learner's device-local behavioral baseline and face
/// embedding in the profile's SQLCipher database.
#[tauri::command]
pub async fn sentinel_save_behavioral_profile(
    state: State<'_, AppState>,
    profile: BehavioralProfile,
) -> Result<(), String> {
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "sentinel.behavioral-profile.save",
            move |db| save_behavioral_profile(db.conn(), &profile),
        )
        .await
}

fn load_user_model_json(
    conn: &Connection,
    user_address: &str,
    device_fp_prefix: &str,
    model_kind: &str,
) -> Result<Option<String>, String> {
    if local_user_address(conn)? != user_address {
        return Err("user model request does not match the active identity".into());
    }
    validate_device_fingerprint_prefix(device_fp_prefix)?;
    conn.query_row(
        "SELECT weights_json FROM sentinel_user_models
         WHERE user_address = ?1 AND device_fp_prefix = ?2 AND model_kind = ?3",
        params![user_address, device_fp_prefix, model_kind],
        |row| row.get(0),
    )
    .optional()
    .map_err(|e| e.to_string())
}

#[allow(clippy::too_many_arguments)]
fn save_user_model_json(
    conn: &Connection,
    user_address: &str,
    device_fp_prefix: &str,
    model_kind: &str,
    json: &str,
    train_loss: f32,
    trained_epochs: i64,
    training_samples: i64,
    now: &str,
) -> Result<(), String> {
    if local_user_address(conn)? != user_address {
        return Err("user model write does not match the active identity".into());
    }
    validate_device_fingerprint_prefix(device_fp_prefix)?;
    conn.execute(
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
            trained_epochs,
            training_samples,
            now,
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub(crate) async fn load_user_model<W>(
    state: &State<'_, AppState>,
    user_address: &str,
    device_fp_prefix: &str,
    model_kind: &str,
) -> Result<Option<W>, String>
where
    W: for<'de> Deserialize<'de> + Send + 'static,
{
    let user_address = user_address.to_string();
    let device_fp_prefix = device_fp_prefix.to_string();
    let model_kind = model_kind.to_string();
    let model_kind_for_error = model_kind.clone();
    let json: Option<String> = state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "sentinel.user-model.load",
            move |db| {
                load_user_model_json(db.conn(), &user_address, &device_fp_prefix, &model_kind)
            },
        )
        .await?;
    match json {
        Some(s) => Ok(Some(
            serde_json::from_str::<W>(&s)
                .with_context(|| format!("parse {model_kind_for_error} weights"))
                .map_err(|e| e.to_string())?,
        )),
        None => Ok(None),
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn save_user_model<W: Serialize>(
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
    if !train_loss.is_finite() {
        return Err("user model training loss must be finite".into());
    }
    let trained_epochs = i64::try_from(trained_epochs)
        .map_err(|_| "trained epoch count exceeds SQLite integer range".to_string())?;
    let training_samples = i64::try_from(training_samples)
        .map_err(|_| "training sample count exceeds SQLite integer range".to_string())?;
    let user_address = user_address.to_string();
    let device_fp_prefix = device_fp_prefix.to_string();
    let model_kind = model_kind.to_string();
    let now = chrono::Utc::now().to_rfc3339();
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "sentinel.user-model.save",
            move |db| {
                save_user_model_json(
                    db.conn(),
                    &user_address,
                    &device_fp_prefix,
                    &model_kind,
                    &json,
                    train_loss,
                    trained_epochs,
                    training_samples,
                    &now,
                )
            },
        )
        .await
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
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "sentinel.user-model.status",
            move |db| {
                if local_user_address(db.conn())? != user_address {
                    return Err("model status request does not match the active identity".into());
                }
                validate_device_fingerprint_prefix(&device_fp_prefix)?;
                let mut stmt = db
                    .conn()
                    .prepare(
                        "SELECT model_kind, trained_epochs, training_samples, train_loss, updated_at
                         FROM sentinel_user_models
                         WHERE user_address = ?1 AND device_fp_prefix = ?2 AND model_kind != ?3
                         ORDER BY model_kind",
                    )
                    .map_err(|e| e.to_string())?;
                let rows = stmt
                    .query_map(
                        params![user_address, device_fp_prefix, BEHAVIORAL_PROFILE_KIND],
                        |row| {
                            Ok(UserModelStatus {
                                model_kind: row.get(0)?,
                                trained_epochs: row.get(1)?,
                                training_samples: row.get(2)?,
                                train_loss: row.get(3)?,
                                updated_at: row.get(4)?,
                            })
                        },
                    )
                    .map_err(|e| e.to_string())?;
                rows.collect::<Result<Vec<_>, _>>()
                    .map_err(|e| e.to_string())
            },
        )
        .await
}

#[tauri::command]
pub async fn sentinel_reset_user_models(
    state: State<'_, AppState>,
    user_address: String,
    device_fp_prefix: String,
) -> Result<(), String> {
    state
        .db_executor
        .execute(
            DatabaseWorkload::Learner,
            state.profile_lease(),
            "sentinel.user-model.reset",
            move |db| {
                if local_user_address(db.conn())? != user_address {
                    return Err("model reset request does not match the active identity".into());
                }
                validate_device_fingerprint_prefix(&device_fp_prefix)?;
                db.conn()
                    .execute(
                        "DELETE FROM sentinel_user_models
                         WHERE user_address = ?1 AND device_fp_prefix = ?2",
                        params![user_address, device_fp_prefix],
                    )
                    .map_err(|e| e.to_string())?;
                Ok(())
            },
        )
        .await
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

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn behavioral_db() -> crate::db::Database {
        let db = crate::db::Database::open_in_memory().unwrap();
        db.run_migrations().unwrap();
        db.conn()
            .execute(
                "INSERT INTO local_identity (id, stake_address, payment_address)
                 VALUES (1, 'stake_test_local', 'addr_test_local')",
                [],
            )
            .unwrap();
        db
    }

    fn behavioral_profile() -> BehavioralProfile {
        BehavioralProfile {
            user_id: "stake_test_local".into(),
            device_fingerprint: "a".repeat(DEVICE_FINGERPRINT_HEX_LEN),
            typing_pattern: TypingPattern {
                avg_dwell_time: 80.0,
                avg_flight_time: 120.0,
                speed_wpm: 60.0,
                sample_count: 12,
            },
            mouse_pattern: MousePattern {
                avg_velocity: 2.0,
                avg_acceleration: 0.5,
                click_precision: 0.9,
                sample_count: 8,
            },
            last_updated: 1_700_000_000_000,
            ai_models: Some(BehavioralAiModels {
                face_enrollment: Some(FaceEnrollment {
                    vector: vec![0.01; FACE_EMBEDDING_DIM],
                    frame_count: 5,
                    updated_at: 1_700_000_000_000,
                }),
            }),
        }
    }

    #[test]
    fn behavioral_profile_round_trips_in_encrypted_profile_database() {
        let db = behavioral_db();
        let expected = behavioral_profile();
        save_behavioral_profile(db.conn(), &expected).unwrap();

        let loaded = load_behavioral_profile(
            db.conn(),
            "stake_test_local",
            &"a".repeat(DEVICE_FINGERPRINT_PREFIX_LEN),
        )
        .unwrap();
        assert_eq!(loaded, Some(expected));
        let kind: String = db
            .conn()
            .query_row("SELECT model_kind FROM sentinel_user_models", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(kind, BEHAVIORAL_PROFILE_KIND);
    }

    #[test]
    fn behavioral_profile_rejects_wrong_identity_and_device() {
        let db = behavioral_db();
        let mut profile = behavioral_profile();
        profile.user_id = "stake_test_other".into();
        assert!(save_behavioral_profile(db.conn(), &profile)
            .unwrap_err()
            .contains("active identity"));

        assert!(load_behavioral_profile(
            db.conn(),
            "stake_test_other",
            &"a".repeat(DEVICE_FINGERPRINT_PREFIX_LEN),
        )
        .unwrap_err()
        .contains("active identity"));
        assert!(
            load_behavioral_profile(db.conn(), "stake_test_local", "not-a-fingerprint")
                .unwrap_err()
                .contains("16 lowercase hexadecimal")
        );
    }

    #[test]
    fn behavioral_profile_rejects_invalid_private_calibration() {
        let db = behavioral_db();
        let mut profile = behavioral_profile();
        profile
            .ai_models
            .as_mut()
            .unwrap()
            .face_enrollment
            .as_mut()
            .unwrap()
            .vector
            .pop();
        assert!(save_behavioral_profile(db.conn(), &profile)
            .unwrap_err()
            .contains("exactly 944"));

        let mut profile = behavioral_profile();
        profile.mouse_pattern.click_precision = f64::NAN;
        assert!(save_behavioral_profile(db.conn(), &profile)
            .unwrap_err()
            .contains("between zero and one"));

        let mut profile = behavioral_profile();
        profile.typing_pattern.sample_count = MAX_SAFE_JAVASCRIPT_INTEGER + 1;
        assert!(save_behavioral_profile(db.conn(), &profile)
            .unwrap_err()
            .contains("JavaScript cannot represent"));
    }

    #[test]
    fn user_model_storage_is_bound_to_active_identity_and_device() {
        let db = behavioral_db();
        let prefix = "a".repeat(DEVICE_FINGERPRINT_PREFIX_LEN);
        save_user_model_json(
            db.conn(),
            "stake_test_local",
            &prefix,
            KEYSTROKE_AE_KIND,
            r#"{"trained_epochs":1}"#,
            0.25,
            1,
            25,
            "2026-09-15T00:00:00Z",
        )
        .unwrap();
        assert!(
            load_user_model_json(db.conn(), "stake_test_local", &prefix, KEYSTROKE_AE_KIND,)
                .unwrap()
                .is_some()
        );

        let read_error =
            load_user_model_json(db.conn(), "stake_test_other", &prefix, KEYSTROKE_AE_KIND)
                .unwrap_err();
        assert!(read_error.contains("active identity"), "got: {read_error}");
        let write_error = save_user_model_json(
            db.conn(),
            "stake_test_other",
            &prefix,
            KEYSTROKE_AE_KIND,
            "{}",
            0.25,
            1,
            25,
            "2026-09-15T00:00:00Z",
        )
        .unwrap_err();
        assert!(
            write_error.contains("active identity"),
            "got: {write_error}"
        );
        assert!(load_user_model_json(
            db.conn(),
            "stake_test_local",
            "not-a-device-id",
            KEYSTROKE_AE_KIND,
        )
        .unwrap_err()
        .contains("16 lowercase hexadecimal"));
    }
}
