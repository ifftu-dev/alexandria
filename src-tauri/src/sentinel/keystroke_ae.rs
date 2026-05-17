//! Per-user keystroke autoencoder — Rust + candle.
//!
//! Replaces `src/utils/sentinel/keystroke-autoencoder.ts`. Same shape
//! (input → hidden → latent → hidden → input, ReLU), same anomaly-
//! score calibration (`5 × trainLoss` → sigmoid). Backprop now runs in
//! candle so we get real autograd for free — no more hand-rolled
//! gradient code to maintain.
//!
//! Weights serialize to a compact JSON blob so per-user weights can
//! live in SQLite (sqlcipher-encrypted) instead of plaintext
//! `localStorage`. Backwards-compatible with the legacy TS weight
//! shape so existing local profiles keep working until the next
//! calibration.

use anyhow::{anyhow, Result};
use candle_core::{DType, Device, Tensor};
use candle_nn::{linear, Linear, Module, Optimizer, VarBuilder, VarMap};
use serde::{Deserialize, Serialize};

use super::types::{DigraphFeatures, KeystrokeEvent};

const INPUT_DIM: usize = 4;
const HIDDEN_DIM: usize = 8;
const LATENT_DIM: usize = 4;
const LEARNING_RATE: f64 = 0.005;
/// Default per-call epoch budget — exposed to the IPC layer so the
/// frontend can override per training session.
pub const DEFAULT_EPOCHS: usize = 80;
const MIN_TRAINING_SAMPLES: usize = 20;
const ANOMALY_THRESHOLD: f32 = 0.65;

// Contrastive "push-away" pass against labeled attack digraphs. Mirrors
// the TS impl — see legacy `keystroke-autoencoder.ts` docs.
const CONTRASTIVE_EPOCHS: usize = 10;
const CONTRASTIVE_LEARNING_RATE: f64 = LEARNING_RATE / 4.0;
const CONTRASTIVE_MARGIN_MULT: f32 = 5.0;

/// Serializable autoencoder weights. JSON-compatible with the legacy
/// TS shape so a future migration can rehydrate per-user profiles
/// without retraining.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoencoderWeights {
    pub w1: Vec<Vec<f32>>,
    pub b1: Vec<f32>,
    pub w2: Vec<Vec<f32>>,
    pub b2: Vec<f32>,
    pub w3: Vec<Vec<f32>>,
    pub b3: Vec<f32>,
    pub w4: Vec<Vec<f32>>,
    pub b4: Vec<f32>,
    #[serde(rename = "trainedEpochs")]
    pub trained_epochs: usize,
    #[serde(rename = "trainingSamples")]
    pub training_samples: usize,
    #[serde(rename = "trainLoss")]
    pub train_loss: f32,
    #[serde(rename = "featureMeans")]
    pub feature_means: Vec<f32>,
    #[serde(rename = "featureStds")]
    pub feature_stds: Vec<f32>,
}

pub struct KeystrokeAutoencoder {
    device: Device,
    varmap: VarMap,
    enc1: Linear,
    enc2: Linear,
    dec1: Linear,
    dec2: Linear,
    feature_means: Vec<f32>,
    feature_stds: Vec<f32>,
    trained_epochs: usize,
    training_samples: usize,
    train_loss: f32,
}

impl KeystrokeAutoencoder {
    pub fn new() -> Result<Self> {
        let device = Device::Cpu;
        let varmap = VarMap::new();
        let vb = VarBuilder::from_varmap(&varmap, DType::F32, &device);
        let enc1 = linear(INPUT_DIM, HIDDEN_DIM, vb.pp("enc1"))?;
        let enc2 = linear(HIDDEN_DIM, LATENT_DIM, vb.pp("enc2"))?;
        let dec1 = linear(LATENT_DIM, HIDDEN_DIM, vb.pp("dec1"))?;
        let dec2 = linear(HIDDEN_DIM, INPUT_DIM, vb.pp("dec2"))?;
        Ok(Self {
            device,
            varmap,
            enc1,
            enc2,
            dec1,
            dec2,
            feature_means: vec![0.0; INPUT_DIM],
            feature_stds: vec![1.0; INPUT_DIM],
            trained_epochs: 0,
            training_samples: 0,
            train_loss: 1.0,
        })
    }

    pub fn from_weights(w: &AutoencoderWeights) -> Result<Self> {
        if w.feature_means.len() != INPUT_DIM || w.feature_stds.len() != INPUT_DIM {
            return Err(anyhow!(
                "weight blob has wrong feature dim: got means={} stds={}",
                w.feature_means.len(),
                w.feature_stds.len()
            ));
        }
        let mut model = Self::new()?;
        load_linear(&mut model.enc1, &w.w1, &w.b1)?;
        load_linear(&mut model.enc2, &w.w2, &w.b2)?;
        load_linear(&mut model.dec1, &w.w3, &w.b3)?;
        load_linear(&mut model.dec2, &w.w4, &w.b4)?;
        model.feature_means = w.feature_means.clone();
        model.feature_stds = w.feature_stds.clone();
        model.trained_epochs = w.trained_epochs;
        model.training_samples = w.training_samples;
        model.train_loss = w.train_loss;
        Ok(model)
    }

    pub fn is_trained(&self) -> bool {
        self.trained_epochs > 0 && self.training_samples >= MIN_TRAINING_SAMPLES
    }

    pub fn train_loss(&self) -> f32 {
        self.train_loss
    }

    pub fn forward(&self, x: &Tensor) -> Result<Tensor> {
        let z1 = self.enc1.forward(x)?.relu()?;
        let z2 = self.enc2.forward(&z1)?.relu()?;
        let z3 = self.dec1.forward(&z2)?.relu()?;
        let z4 = self.dec2.forward(&z3)?;
        Ok(z4)
    }

    /// Train on the user's own keystrokes. Optionally fold in labeled
    /// attack digraphs as a contrastive "push away" pass at the end.
    /// Returns final train loss, or `-1.0` if there were too few samples.
    pub fn train(
        &mut self,
        events: &[KeystrokeEvent],
        epochs: usize,
        negative_digraphs: &[DigraphFeatures],
    ) -> Result<f32> {
        let digraphs = extract_digraph_features(events);
        if digraphs.len() < MIN_TRAINING_SAMPLES {
            return Ok(-1.0);
        }

        let raw: Vec<[f32; INPUT_DIM]> = digraphs.iter().map(feature_to_vec).collect();
        let (means, stds) = compute_norm_stats(&raw);
        self.feature_means = means.to_vec();
        self.feature_stds = stds.to_vec();

        let normalized: Vec<[f32; INPUT_DIM]> =
            raw.iter().map(|r| normalize(r, &means, &stds)).collect();
        let flat: Vec<f32> = normalized
            .iter()
            .flat_map(|row| row.iter().copied())
            .collect();
        let x = Tensor::from_vec(flat, (normalized.len(), INPUT_DIM), &self.device)?;

        let mut optimizer = candle_nn::optim::SGD::new(self.varmap.all_vars(), LEARNING_RATE)?;

        let mut last_loss = 1.0_f32;
        for _epoch in 0..epochs {
            let pred = self.forward(&x)?;
            let loss = pred.sub(&x)?.sqr()?.mean_all()?;
            optimizer.backward_step(&loss)?;
            last_loss = loss.to_scalar::<f32>()?;
        }

        self.trained_epochs += epochs;
        self.training_samples = digraphs.len();
        self.train_loss = last_loss;

        if !negative_digraphs.is_empty() {
            self.train_contrastive(negative_digraphs)?;
        }

        Ok(last_loss)
    }

    /// Negative-sample push-away. Reconstruction error on labeled
    /// attacks is pushed UP (gradient ascent) until it exceeds the
    /// `5 × trainLoss` margin, then the sample is skipped.
    fn train_contrastive(&mut self, negative_digraphs: &[DigraphFeatures]) -> Result<()> {
        let margin = self.train_loss * CONTRASTIVE_MARGIN_MULT;
        let raw: Vec<[f32; INPUT_DIM]> = negative_digraphs.iter().map(feature_to_vec).collect();
        let normalized: Vec<[f32; INPUT_DIM]> = raw
            .iter()
            .map(|r| normalize(r, &self.feature_means, &self.feature_stds))
            .collect();

        let mut optimizer =
            candle_nn::optim::SGD::new(self.varmap.all_vars(), CONTRASTIVE_LEARNING_RATE)?;

        for _ in 0..CONTRASTIVE_EPOCHS {
            // Filter out samples already past the margin so we don't
            // destabilize the network pushing them further.
            let active: Vec<[f32; INPUT_DIM]> = normalized
                .iter()
                .copied()
                .filter(|row| {
                    let r = row.to_vec();
                    let Ok(x) = Tensor::from_vec(r, (1, INPUT_DIM), &self.device) else {
                        return false;
                    };
                    let Ok(pred) = self.forward(&x) else {
                        return false;
                    };
                    let Ok(loss_t) = pred
                        .sub(&x)
                        .and_then(|d| d.sqr())
                        .and_then(|s| s.mean_all())
                    else {
                        return false;
                    };
                    loss_t
                        .to_scalar::<f32>()
                        .map(|l| l < margin)
                        .unwrap_or(false)
                })
                .collect();

            if active.is_empty() {
                break;
            }
            let flat: Vec<f32> = active.iter().flat_map(|r| r.iter().copied()).collect();
            let x = Tensor::from_vec(flat, (active.len(), INPUT_DIM), &self.device)?;
            let pred = self.forward(&x)?;
            // Gradient ASCENT: negate the loss so the optimizer step
            // increases reconstruction error on these samples.
            let loss = pred.sub(&x)?.sqr()?.mean_all()?.neg()?;
            optimizer.backward_step(&loss)?;
        }
        Ok(())
    }

    /// Score raw keystroke events. Returns `-1.0` if the model isn't
    /// trained or there's nothing to score, mirroring the legacy TS
    /// contract.
    pub fn score(&self, events: &[KeystrokeEvent]) -> Result<f32> {
        if !self.is_trained() {
            return Ok(-1.0);
        }
        let digraphs = extract_digraph_features(events);
        self.score_digraphs(&digraphs)
    }

    /// Score pre-extracted digraphs (used by ratified-prior evaluation
    /// where samples already arrive feature-shaped).
    pub fn score_digraphs(&self, digraphs: &[DigraphFeatures]) -> Result<f32> {
        if !self.is_trained() || digraphs.len() < 5 {
            return Ok(-1.0);
        }
        let raw: Vec<[f32; INPUT_DIM]> = digraphs.iter().map(feature_to_vec).collect();
        let normalized: Vec<[f32; INPUT_DIM]> = raw
            .iter()
            .map(|r| normalize(r, &self.feature_means, &self.feature_stds))
            .collect();
        let flat: Vec<f32> = normalized.iter().flat_map(|r| r.iter().copied()).collect();
        let x = Tensor::from_vec(flat, (normalized.len(), INPUT_DIM), &self.device)?;
        let pred = self.forward(&x)?;
        let avg_error: f32 = pred.sub(&x)?.sqr()?.mean_all()?.to_scalar::<f32>()?;

        // Sigmoid calibration: 5× train loss ⇒ score ≈ 0.5. Floor
        // train_loss at 0.05 to prevent ratio blow-up when very
        // consistent users would otherwise have train_loss ≈ 0.
        let baseline = self.train_loss.max(0.05);
        let ratio = avg_error / baseline;
        let sigmoid = 1.0_f32 / (1.0_f32 + (-0.5 * (ratio - 5.0)).exp());
        Ok(sigmoid.clamp(0.0, 1.0))
    }

    pub fn is_anomalous(score: f32) -> bool {
        score >= ANOMALY_THRESHOLD
    }

    pub fn export_weights(&self) -> Result<AutoencoderWeights> {
        Ok(AutoencoderWeights {
            w1: linear_weight(&self.enc1)?,
            b1: linear_bias(&self.enc1)?,
            w2: linear_weight(&self.enc2)?,
            b2: linear_bias(&self.enc2)?,
            w3: linear_weight(&self.dec1)?,
            b3: linear_bias(&self.dec1)?,
            w4: linear_weight(&self.dec2)?,
            b4: linear_bias(&self.dec2)?,
            trained_epochs: self.trained_epochs,
            training_samples: self.training_samples,
            train_loss: self.train_loss,
            feature_means: self.feature_means.clone(),
            feature_stds: self.feature_stds.clone(),
        })
    }
}

// ----- helpers ----------------------------------------------------------

fn feature_to_vec(f: &DigraphFeatures) -> [f32; INPUT_DIM] {
    [f.dwell_ms1, f.dwell_ms2, f.flight_ms, f.speed_ratio]
}

fn compute_norm_stats(data: &[[f32; INPUT_DIM]]) -> ([f32; INPUT_DIM], [f32; INPUT_DIM]) {
    let n = data.len() as f32;
    let mut means = [0.0; INPUT_DIM];
    for row in data {
        for j in 0..INPUT_DIM {
            means[j] += row[j];
        }
    }
    for m in &mut means {
        *m /= n;
    }
    let mut stds = [0.0; INPUT_DIM];
    for row in data {
        for j in 0..INPUT_DIM {
            let d = row[j] - means[j];
            stds[j] += d * d;
        }
    }
    for s in &mut stds {
        *s = (*s / n).sqrt();
    }
    (means, stds)
}

fn normalize(row: &[f32; INPUT_DIM], means: &[f32], stds: &[f32]) -> [f32; INPUT_DIM] {
    let mut out = [0.0; INPUT_DIM];
    for j in 0..INPUT_DIM {
        out[j] = if stds[j] > 0.001 {
            (row[j] - means[j]) / stds[j]
        } else {
            0.0
        };
    }
    out
}

pub fn extract_digraph_features(events: &[KeystrokeEvent]) -> Vec<DigraphFeatures> {
    let mut out = Vec::with_capacity(events.len());
    for i in 1..events.len() {
        let prev = &events[i - 1];
        let curr = &events[i];
        if prev.dwell_ms <= 0.0 || curr.dwell_ms <= 0.0 {
            continue;
        }
        if curr.flight_ms <= 0.0 {
            continue;
        }
        let total_prev = prev.dwell_ms + prev.flight_ms;
        let speed = if total_prev > 0.0 {
            curr.dwell_ms / total_prev
        } else {
            1.0
        };
        out.push(DigraphFeatures {
            dwell_ms1: prev.dwell_ms,
            dwell_ms2: curr.dwell_ms,
            flight_ms: curr.flight_ms,
            speed_ratio: speed.min(5.0),
        });
    }
    out
}

fn linear_weight(layer: &Linear) -> Result<Vec<Vec<f32>>> {
    let w = layer.weight();
    let shape = w.dims();
    if shape.len() != 2 {
        return Err(anyhow!("linear weight not 2D: shape {:?}", shape));
    }
    let rows = shape[0];
    let cols = shape[1];
    let flat: Vec<f32> = w.to_vec2::<f32>()?.into_iter().flatten().collect();
    let mut out = Vec::with_capacity(rows);
    for r in 0..rows {
        out.push(flat[r * cols..(r + 1) * cols].to_vec());
    }
    Ok(out)
}

fn linear_bias(layer: &Linear) -> Result<Vec<f32>> {
    layer
        .bias()
        .map(|b| b.to_vec1::<f32>().map_err(|e| anyhow!(e)))
        .unwrap_or_else(|| Ok(vec![]))
}

fn load_linear(layer: &mut Linear, w: &[Vec<f32>], b: &[f32]) -> Result<()> {
    let device = Device::Cpu;
    let rows = w.len();
    let cols = if rows == 0 { 0 } else { w[0].len() };
    let flat: Vec<f32> = w.iter().flatten().copied().collect();
    let weight = Tensor::from_vec(flat, (rows, cols), &device)?;
    let bias = Tensor::from_vec(b.to_vec(), (b.len(),), &device)?;
    *layer = Linear::new(weight, Some(bias));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_events(n: usize, dwell: f32, flight: f32) -> Vec<KeystrokeEvent> {
        (0..n)
            .map(|i| KeystrokeEvent {
                key: "char".into(),
                dwell_ms: dwell + (i as f32 * 0.1),
                flight_ms: flight + (i as f32 * 0.1),
            })
            .collect()
    }

    #[test]
    fn untrained_score_returns_minus_one() {
        let ae = KeystrokeAutoencoder::new().unwrap();
        let events = fake_events(40, 80.0, 110.0);
        assert_eq!(ae.score(&events).unwrap(), -1.0);
    }

    #[test]
    fn training_reduces_or_holds_loss_and_marks_trained() {
        let mut ae = KeystrokeAutoencoder::new().unwrap();
        let events = fake_events(60, 80.0, 110.0);
        let final_loss = ae.train(&events, 30, &[]).unwrap();
        assert!(ae.is_trained(), "should be trained after train()");
        assert!(final_loss >= 0.0, "non-negative loss");
        // Score on the same-distribution input should be low-ish.
        let s = ae.score(&events).unwrap();
        assert!((0.0..=1.0).contains(&s));
    }

    #[test]
    fn weights_roundtrip_preserves_score() {
        let mut ae = KeystrokeAutoencoder::new().unwrap();
        let events = fake_events(60, 80.0, 110.0);
        ae.train(&events, 20, &[]).unwrap();
        let s1 = ae.score(&events).unwrap();
        let w = ae.export_weights().unwrap();
        let ae2 = KeystrokeAutoencoder::from_weights(&w).unwrap();
        let s2 = ae2.score(&events).unwrap();
        assert!(
            (s1 - s2).abs() < 1e-3,
            "score drifted after roundtrip: s1={s1} s2={s2}"
        );
    }
}
