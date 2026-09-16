//! Labeled Sentinel sample blobs.
//!
//! The encrypted holdout set carries a labeled-samples blob. This module
//! owns the blob shape and its validation. It grants no authority: the
//! community prior library, its ratification and runtime classifier
//! replacement are deleted, so a valid blob only feeds local evaluation.
//!
//! The face model kind is forbidden by design (see decision 2 in
//! `docs/sentinel-federation.md`).

use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// Current blob schema version.
pub const CURRENT_SCHEMA_VERSION: u32 = 1;

/// Minimum labeled samples per blob. Below this, gradient inversion
/// becomes trivially successful and the blob adds little signal anyway.
pub const MIN_SAMPLES: usize = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelKind {
    Keystroke,
    Mouse,
    /// Trained ONNX classifier weights described by a metadata object
    /// (`WeightsBlobMeta`) rather than a samples array.
    PasteClassifierWeights,
}

impl ModelKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            ModelKind::Keystroke => "keystroke",
            ModelKind::Mouse => "mouse",
            ModelKind::PasteClassifierWeights => "paste_classifier_weights",
        }
    }
}

impl FromStr for ModelKind {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "keystroke" => Ok(ModelKind::Keystroke),
            "mouse" => Ok(ModelKind::Mouse),
            "paste_classifier_weights" => Ok(ModelKind::PasteClassifierWeights),
            "face" => Err(
                "face is not a permitted model_kind for Sentinel sample blobs \
                 (see docs/sentinel-federation.md decision 2)"
                    .into(),
            ),
            other => Err(format!("unknown model_kind: {other}")),
        }
    }
}

/// Parsed shape of a labeled-samples blob.
///
/// `samples` is left as an opaque JSON value so the blob can evolve its
/// inner shape (keystroke digraphs vs. mouse trajectories) independently
/// of this metadata envelope.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PriorBlob {
    pub schema_version: u32,
    pub model_kind: String,
    pub label: String,
    pub samples: serde_json::Value,
    #[serde(default)]
    pub notes: Option<String>,
    #[serde(default)]
    pub contributor_attribution: Option<String>,
}

/// Parse + validate a blob JSON string.
///
/// Two shapes are accepted:
///
/// - Labeled-samples blob (`keystroke`, `mouse`): `samples` is a JSON
///   array of ≥ `MIN_SAMPLES` entries.
/// - Weights blob (`paste_classifier_weights`): `samples` is an object
///   conforming to [`WeightsBlobMeta`].
pub fn validate_prior_blob(json: &str) -> Result<PriorBlob, String> {
    let blob: PriorBlob =
        serde_json::from_str(json).map_err(|e| format!("blob parse failed: {e}"))?;

    if blob.schema_version == 0 || blob.schema_version > CURRENT_SCHEMA_VERSION {
        return Err(format!(
            "unsupported schema_version {} (client knows up to {})",
            blob.schema_version, CURRENT_SCHEMA_VERSION
        ));
    }

    // Reject face kind loudly, even before generic validation — this is
    // a hard-line architectural invariant per the threat model.
    let kind = ModelKind::from_str(&blob.model_kind)?;

    if blob.label.trim().is_empty() {
        return Err("label must be a non-empty string".into());
    }

    match kind {
        ModelKind::Keystroke | ModelKind::Mouse => {
            let sample_count = match &blob.samples {
                serde_json::Value::Array(items) => items.len(),
                _ => return Err("samples must be a JSON array".into()),
            };
            if sample_count < MIN_SAMPLES {
                return Err(format!(
                    "at least {MIN_SAMPLES} samples required (got {sample_count})"
                ));
            }
        }
        ModelKind::PasteClassifierWeights => {
            validate_weights_meta(&blob.samples)?;
        }
    }

    Ok(blob)
}

/// Metadata payload carried inside the `samples` field of a
/// `paste_classifier_weights` blob.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WeightsBlobMeta {
    pub weights_cid: String,
    pub eval_cid: String,
    pub eval_tpr: f64,
    pub eval_fpr: f64,
    pub version: String,
}

pub fn validate_weights_meta(samples: &serde_json::Value) -> Result<WeightsBlobMeta, String> {
    let meta: WeightsBlobMeta = serde_json::from_value(samples.clone())
        .map_err(|e| format!("weights blob `samples` must be a WeightsBlobMeta object: {e}"))?;

    if meta.weights_cid.trim().is_empty() {
        return Err("weights_cid must be non-empty".into());
    }
    if meta.eval_cid.trim().is_empty() {
        return Err("eval_cid must be non-empty".into());
    }
    if meta.version.trim().is_empty() {
        return Err("version must be non-empty".into());
    }
    if !(0.0..=1.0).contains(&meta.eval_tpr) {
        return Err(format!("eval_tpr out of range [0,1]: {}", meta.eval_tpr));
    }
    if !(0.0..=1.0).contains(&meta.eval_fpr) {
        return Err(format!("eval_fpr out of range [0,1]: {}", meta.eval_fpr));
    }
    Ok(meta)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn synth_samples(n: usize) -> serde_json::Value {
        serde_json::Value::Array(
            (0..n)
                .map(|i| {
                    json!({
                        "dwellMs1": 80 + (i % 5),
                        "dwellMs2": 75 + (i % 4),
                        "flightMs": 120,
                        "speedRatio": 1.5,
                    })
                })
                .collect(),
        )
    }

    fn valid_keystroke_blob() -> serde_json::Value {
        json!({
            "schema_version": 1,
            "model_kind": "keystroke",
            "label": "paste_macro",
            "samples": synth_samples(25),
            "notes": "test fixture",
        })
    }

    fn valid_weights_blob() -> serde_json::Value {
        json!({
            "schema_version": 1,
            "model_kind": "paste_classifier_weights",
            "label": "paste-v1",
            "samples": {
                "weights_cid": "blake3-weights-cid",
                "eval_cid": "blake3-eval-cid",
                "eval_tpr": 0.97,
                "eval_fpr": 0.01,
                "version": "paste-v1",
            },
            "notes": "synthetic train, holdout TPR=0.97",
        })
    }

    #[test]
    fn model_kind_parse_matches_case() {
        assert_eq!(
            ModelKind::from_str("keystroke").unwrap(),
            ModelKind::Keystroke
        );
        assert_eq!(ModelKind::from_str("mouse").unwrap(), ModelKind::Mouse);
        assert_eq!(
            ModelKind::from_str("paste_classifier_weights").unwrap(),
            ModelKind::PasteClassifierWeights,
        );
        assert!(ModelKind::from_str("Keystroke").is_err());
        assert!(ModelKind::from_str("").is_err());
    }

    #[test]
    fn face_kind_is_rejected_with_explanatory_error() {
        let err = ModelKind::from_str("face").unwrap_err();
        assert!(err.contains("face"), "error should mention face: {err}");
        assert!(
            err.contains("decision 2"),
            "error should cite the decision: {err}"
        );
    }

    #[test]
    fn blob_validates_when_well_formed() {
        let json = valid_keystroke_blob().to_string();
        let blob = validate_prior_blob(&json).unwrap();
        assert_eq!(blob.model_kind, "keystroke");
        assert_eq!(blob.label, "paste_macro");
        assert_eq!(blob.schema_version, 1);
    }

    #[test]
    fn blob_validation_rejects_too_few_samples() {
        let mut bad = valid_keystroke_blob();
        bad["samples"] = synth_samples(MIN_SAMPLES - 1);
        let err = validate_prior_blob(&bad.to_string()).unwrap_err();
        assert!(err.contains("samples required"), "got {err}");
    }

    #[test]
    fn blob_validation_rejects_face_kind() {
        let mut bad = valid_keystroke_blob();
        bad["model_kind"] = json!("face");
        let err = validate_prior_blob(&bad.to_string()).unwrap_err();
        assert!(err.contains("face"), "got {err}");
    }

    #[test]
    fn blob_validation_rejects_empty_label() {
        let mut bad = valid_keystroke_blob();
        bad["label"] = json!("   ");
        let err = validate_prior_blob(&bad.to_string()).unwrap_err();
        assert!(err.contains("label"), "got {err}");
    }

    #[test]
    fn blob_validation_rejects_unknown_schema_version() {
        let mut bad = valid_keystroke_blob();
        bad["schema_version"] = json!(999);
        let err = validate_prior_blob(&bad.to_string()).unwrap_err();
        assert!(err.contains("schema_version"), "got {err}");
    }

    #[test]
    fn blob_validation_rejects_non_array_samples() {
        let mut bad = valid_keystroke_blob();
        bad["samples"] = json!("not an array");
        let err = validate_prior_blob(&bad.to_string()).unwrap_err();
        assert!(err.contains("array"), "got {err}");
    }

    #[test]
    fn weights_blob_validates() {
        let blob = validate_prior_blob(&valid_weights_blob().to_string()).unwrap();
        assert_eq!(blob.model_kind, "paste_classifier_weights");
        assert_eq!(blob.label, "paste-v1");
        let meta = validate_weights_meta(&blob.samples).unwrap();
        assert_eq!(meta.weights_cid, "blake3-weights-cid");
        assert_eq!(meta.eval_tpr, 0.97);
    }

    #[test]
    fn weights_blob_rejects_array_samples() {
        let mut bad = valid_weights_blob();
        bad["samples"] = json!([]);
        let err = validate_prior_blob(&bad.to_string()).unwrap_err();
        assert!(
            err.contains("WeightsBlobMeta") || err.contains("weights blob"),
            "got: {err}",
        );
    }

    #[test]
    fn weights_blob_rejects_out_of_range_tpr() {
        let mut bad = valid_weights_blob();
        bad["samples"]["eval_tpr"] = json!(1.5);
        let err = validate_prior_blob(&bad.to_string()).unwrap_err();
        assert!(err.contains("eval_tpr"), "got: {err}");
    }

    #[test]
    fn weights_blob_rejects_empty_weights_cid() {
        let mut bad = valid_weights_blob();
        bad["samples"]["weights_cid"] = json!("");
        let err = validate_prior_blob(&bad.to_string()).unwrap_err();
        assert!(err.contains("weights_cid"), "got: {err}");
    }
}
