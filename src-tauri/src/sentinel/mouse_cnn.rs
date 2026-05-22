//! Mouse-trajectory CNN — reservoir-style, candle for trainable dense.
//!
//! Replaces `src/utils/sentinel/mouse-trajectory-cnn.ts`. Same shape:
//!   Conv1D(3→8, k=5) → ReLU → MaxPool(2)
//! → Conv1D(8→16, k=3) → ReLU → MaxPool(2)
//! → Dense(160→32) → ReLU → Dense(32→1) → Sigmoid
//!
//! Conv layers stay random-initialised + frozen (reservoir computing
//! — the legacy doc rationale was on-device-training speed). Only the
//! two dense layers train; backprop is candle autograd. Input is a
//! 50-point trajectory segment with (dx, dy, dt) channels.

use anyhow::{anyhow, Result};
use candle_core::{DType, Device, Tensor};
use candle_nn::{linear, ops, Linear, Module, Optimizer, VarBuilder, VarMap};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use serde::{Deserialize, Serialize};

use super::types::MousePoint;

const SEGMENT_LEN: usize = 50;
const INPUT_CHANNELS: usize = 3;
const CONV1_OUT: usize = 8;
const CONV1_KSZ: usize = 5;
const CONV2_OUT: usize = 16;
const CONV2_KSZ: usize = 3;
// Per the TS impl: after Conv(k=5)+pool2 then Conv(k=3)+pool2 on len=50
// → 10 timesteps × 16 channels = 160 features for the dense head.
const DENSE_IN: usize = 160;
const DENSE_HIDDEN: usize = 32;
const LEARNING_RATE: f64 = 0.005;
const DEFAULT_EPOCHS: usize = 80;
const HUMAN_THRESHOLD: f32 = 0.5;

/// Serializable weights — dense layers are trainable; conv kernels
/// are deterministic given the seed so we don't need to store them.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MouseCnnWeights {
    pub conv_seed: u64,
    #[serde(rename = "denseW1")]
    pub dense_w1: Vec<Vec<f32>>,
    #[serde(rename = "denseB1")]
    pub dense_b1: Vec<f32>,
    #[serde(rename = "denseW2")]
    pub dense_w2: Vec<Vec<f32>>,
    #[serde(rename = "denseB2")]
    pub dense_b2: Vec<f32>,
    #[serde(rename = "trainedEpochs")]
    pub trained_epochs: usize,
    #[serde(rename = "trainingSamples")]
    pub training_samples: usize,
    #[serde(rename = "trainLoss")]
    pub train_loss: f32,
}

pub struct MouseTrajectoryCnn {
    device: Device,
    varmap: VarMap,
    // Conv weights live as plain f32 buffers — frozen, no autograd.
    conv1: Vec<f32>, // shape [CONV1_OUT, INPUT_CHANNELS, CONV1_KSZ]
    conv1_bias: Vec<f32>,
    conv2: Vec<f32>, // shape [CONV2_OUT, CONV1_OUT, CONV2_KSZ]
    conv2_bias: Vec<f32>,
    dense1: Linear,
    dense2: Linear,
    conv_seed: u64,
    trained_epochs: usize,
    training_samples: usize,
    train_loss: f32,
}

impl MouseTrajectoryCnn {
    pub fn new() -> Result<Self> {
        Self::with_conv_seed(0xC0FFEE_u64)
    }

    pub fn with_conv_seed(conv_seed: u64) -> Result<Self> {
        let device = Device::Cpu;
        let varmap = VarMap::new();
        let vb = VarBuilder::from_varmap(&varmap, DType::F32, &device);
        let dense1 = linear(DENSE_IN, DENSE_HIDDEN, vb.pp("dense1"))?;
        let dense2 = linear(DENSE_HIDDEN, 1, vb.pp("dense2"))?;
        let (conv1, conv1_bias) = init_conv(conv_seed, CONV1_OUT, INPUT_CHANNELS, CONV1_KSZ);
        let (conv2, conv2_bias) =
            init_conv(conv_seed ^ 0xA5A5_5A5A_u64, CONV2_OUT, CONV1_OUT, CONV2_KSZ);
        Ok(Self {
            device,
            varmap,
            conv1,
            conv1_bias,
            conv2,
            conv2_bias,
            dense1,
            dense2,
            conv_seed,
            trained_epochs: 0,
            training_samples: 0,
            train_loss: 1.0,
        })
    }

    pub fn from_weights(w: &MouseCnnWeights) -> Result<Self> {
        let mut model = Self::with_conv_seed(w.conv_seed)?;
        load_linear(&mut model.dense1, &w.dense_w1, &w.dense_b1)?;
        load_linear(&mut model.dense2, &w.dense_w2, &w.dense_b2)?;
        model.trained_epochs = w.trained_epochs;
        model.training_samples = w.training_samples;
        model.train_loss = w.train_loss;
        Ok(model)
    }

    pub fn is_trained(&self) -> bool {
        self.trained_epochs > 0 && self.training_samples >= 1
    }

    pub fn is_human(prob: f32) -> bool {
        prob >= HUMAN_THRESHOLD
    }

    pub fn predict(&self, points: &[MousePoint]) -> Result<f32> {
        if !self.is_trained() {
            return Ok(-1.0);
        }
        let segments = build_segments(points);
        if segments.is_empty() {
            return Ok(-1.0);
        }
        let mut total = 0.0_f32;
        for seg in &segments {
            total += self.forward_segment(seg)?;
        }
        Ok((total / segments.len() as f32).clamp(0.0, 1.0))
    }

    /// Train on labeled trajectories. Positives are human points;
    /// negatives are synthetic bot patterns or DAO-ratified prior
    /// trajectories. Returns final BCE loss.
    pub fn train(
        &mut self,
        human_points: &[MousePoint],
        bot_segments: &[[f32; SEGMENT_LEN * INPUT_CHANNELS]],
        epochs: usize,
    ) -> Result<f32> {
        let human_segments = build_segments(human_points);
        if human_segments.is_empty() {
            return Ok(-1.0);
        }

        let mut samples: Vec<([f32; SEGMENT_LEN * INPUT_CHANNELS], f32)> = Vec::new();
        for seg in human_segments.iter() {
            samples.push((*seg, 1.0));
        }
        for seg in bot_segments.iter() {
            samples.push((*seg, 0.0));
        }
        if samples.is_empty() {
            return Ok(-1.0);
        }

        // Pre-extract dense features via the frozen conv path so we
        // only differentiate through the two trainable layers.
        let mut dense_feats: Vec<[f32; DENSE_IN]> = Vec::with_capacity(samples.len());
        let mut targets: Vec<f32> = Vec::with_capacity(samples.len());
        for (seg, y) in &samples {
            dense_feats.push(self.dense_features(seg)?);
            targets.push(*y);
        }

        let flat_feat: Vec<f32> = dense_feats.iter().flat_map(|r| r.iter().copied()).collect();
        let x = Tensor::from_vec(flat_feat, (samples.len(), DENSE_IN), &self.device)?;
        let y = Tensor::from_vec(targets, (samples.len(), 1), &self.device)?;

        let mut optimizer = candle_nn::optim::SGD::new(self.varmap.all_vars(), LEARNING_RATE)?;

        let mut last_loss = 1.0_f32;
        for _epoch in 0..epochs {
            let pred = self.head_forward(&x)?;
            // BCE: -(y·log(p) + (1-y)·log(1-p)). Epsilons protect log.
            let eps = 1e-7_f64;
            let p_clamped = pred.clamp(eps, 1.0 - eps)?;
            let one = Tensor::ones_like(&y)?;
            let term1 = y.mul(&p_clamped.log()?)?;
            let term2 = (&one - &y)?.mul(&(&one - &p_clamped)?.log()?)?;
            let loss = (&term1 + &term2)?.neg()?.mean_all()?;
            optimizer.backward_step(&loss)?;
            last_loss = loss.to_scalar::<f32>()?;
        }

        self.trained_epochs += epochs;
        self.training_samples = samples.len();
        self.train_loss = last_loss;
        Ok(last_loss)
    }

    pub fn export_weights(&self) -> Result<MouseCnnWeights> {
        Ok(MouseCnnWeights {
            conv_seed: self.conv_seed,
            dense_w1: linear_weight(&self.dense1)?,
            dense_b1: linear_bias(&self.dense1)?,
            dense_w2: linear_weight(&self.dense2)?,
            dense_b2: linear_bias(&self.dense2)?,
            trained_epochs: self.trained_epochs,
            training_samples: self.training_samples,
            train_loss: self.train_loss,
        })
    }

    /// Run a single segment through the full network. Returns the
    /// final sigmoid probability.
    fn forward_segment(&self, seg: &[f32; SEGMENT_LEN * INPUT_CHANNELS]) -> Result<f32> {
        let feats = self.dense_features(seg)?;
        let x = Tensor::from_vec(feats.to_vec(), (1, DENSE_IN), &self.device)?;
        let pred = self.head_forward(&x)?;
        Ok(pred.to_vec2::<f32>()?[0][0])
    }

    fn head_forward(&self, x: &Tensor) -> Result<Tensor> {
        let h1 = self.dense1.forward(x)?.relu()?;
        let h2 = self.dense2.forward(&h1)?;
        Ok(ops::sigmoid(&h2)?)
    }

    /// Conv path on a 50-point segment → 160-feature vector. Pure
    /// hand-rolled (no autograd needed because conv weights are frozen).
    fn dense_features(&self, seg: &[f32; SEGMENT_LEN * INPUT_CHANNELS]) -> Result<[f32; DENSE_IN]> {
        // Input shape: [50, 3] in row-major channel-last layout.
        // Conv1: stride 1, no padding → output length 46.
        let conv1_out_len = SEGMENT_LEN - CONV1_KSZ + 1;
        let mut c1 = vec![0.0_f32; CONV1_OUT * conv1_out_len];
        for f in 0..CONV1_OUT {
            for t in 0..conv1_out_len {
                let mut acc = self.conv1_bias[f];
                for k in 0..CONV1_KSZ {
                    for ch in 0..INPUT_CHANNELS {
                        let kw = self.conv1[((f * INPUT_CHANNELS) + ch) * CONV1_KSZ + k];
                        let x = seg[(t + k) * INPUT_CHANNELS + ch];
                        acc += kw * x;
                    }
                }
                c1[f * conv1_out_len + t] = acc.max(0.0); // ReLU
            }
        }
        // MaxPool(2) on length axis → 23.
        let pool1_len = conv1_out_len / 2;
        let mut p1 = vec![0.0_f32; CONV1_OUT * pool1_len];
        for f in 0..CONV1_OUT {
            for t in 0..pool1_len {
                let a = c1[f * conv1_out_len + t * 2];
                let b = c1[f * conv1_out_len + t * 2 + 1];
                p1[f * pool1_len + t] = a.max(b);
            }
        }
        // Conv2: k=3, output length 21.
        let conv2_out_len = pool1_len - CONV2_KSZ + 1;
        let mut c2 = vec![0.0_f32; CONV2_OUT * conv2_out_len];
        for f in 0..CONV2_OUT {
            for t in 0..conv2_out_len {
                let mut acc = self.conv2_bias[f];
                for k in 0..CONV2_KSZ {
                    for ch in 0..CONV1_OUT {
                        let kw = self.conv2[((f * CONV1_OUT) + ch) * CONV2_KSZ + k];
                        let x = p1[ch * pool1_len + t + k];
                        acc += kw * x;
                    }
                }
                c2[f * conv2_out_len + t] = acc.max(0.0);
            }
        }
        // MaxPool(2) → length 10 → 16 × 10 = 160 features.
        let pool2_len = conv2_out_len / 2;
        let mut p2 = [0.0_f32; DENSE_IN];
        for f in 0..CONV2_OUT {
            for t in 0..pool2_len {
                let a = c2[f * conv2_out_len + t * 2];
                let b = c2[f * conv2_out_len + t * 2 + 1];
                p2[f * pool2_len + t] = a.max(b);
            }
        }
        Ok(p2)
    }
}

pub fn default_epochs() -> usize {
    DEFAULT_EPOCHS
}

fn init_conv(seed: u64, out_ch: usize, in_ch: usize, k: usize) -> (Vec<f32>, Vec<f32>) {
    let mut rng = StdRng::seed_from_u64(seed);
    let scale = (2.0_f32 / (out_ch + in_ch * k) as f32).sqrt();
    let len = out_ch * in_ch * k;
    let mut w = Vec::with_capacity(len);
    for _ in 0..len {
        let u: f32 = rng.gen();
        w.push((u * 2.0 - 1.0) * scale);
    }
    let bias = vec![0.0; out_ch];
    (w, bias)
}

fn build_segments(points: &[MousePoint]) -> Vec<[f32; SEGMENT_LEN * INPUT_CHANNELS]> {
    let mut segments = Vec::new();
    if points.len() < SEGMENT_LEN + 1 {
        return segments;
    }
    let mut start = 0;
    while start + SEGMENT_LEN < points.len() {
        let mut seg = [0.0_f32; SEGMENT_LEN * INPUT_CHANNELS];
        // Normalize per-segment so absolute coordinates don't leak.
        let mut max_d = 1e-3_f32;
        let mut max_t = 1e-3_f32;
        for i in 0..SEGMENT_LEN {
            let dx = points[start + i + 1].x - points[start + i].x;
            let dy = points[start + i + 1].y - points[start + i].y;
            let dt = (points[start + i + 1].t - points[start + i].t).max(0.0);
            max_d = max_d.max(dx.abs()).max(dy.abs());
            max_t = max_t.max(dt);
        }
        for i in 0..SEGMENT_LEN {
            let dx = points[start + i + 1].x - points[start + i].x;
            let dy = points[start + i + 1].y - points[start + i].y;
            let dt = (points[start + i + 1].t - points[start + i].t).max(0.0);
            seg[i * INPUT_CHANNELS] = dx / max_d;
            seg[i * INPUT_CHANNELS + 1] = dy / max_d;
            seg[i * INPUT_CHANNELS + 2] = dt / max_t;
        }
        segments.push(seg);
        start += SEGMENT_LEN;
    }
    segments
}

fn linear_weight(layer: &Linear) -> Result<Vec<Vec<f32>>> {
    let w = layer.weight();
    let shape = w.dims();
    if shape.len() != 2 {
        return Err(anyhow!("linear weight not 2D"));
    }
    Ok(w.to_vec2::<f32>()?)
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

    fn human_path(n: usize) -> Vec<MousePoint> {
        (0..n)
            .map(|i| MousePoint {
                x: i as f32 * 1.5 + (i as f32 * 0.1).sin() * 5.0,
                y: i as f32 * 0.8 + (i as f32 * 0.13).cos() * 3.0,
                t: i as f32 * 16.0,
            })
            .collect()
    }

    fn bot_segment_constant() -> [f32; SEGMENT_LEN * INPUT_CHANNELS] {
        let mut seg = [0.0_f32; SEGMENT_LEN * INPUT_CHANNELS];
        for i in 0..SEGMENT_LEN {
            seg[i * INPUT_CHANNELS] = 0.5;
            seg[i * INPUT_CHANNELS + 1] = 0.0;
            seg[i * INPUT_CHANNELS + 2] = 1.0;
        }
        seg
    }

    #[test]
    fn untrained_predict_returns_minus_one() {
        let cnn = MouseTrajectoryCnn::new().unwrap();
        let points = human_path(60);
        assert_eq!(cnn.predict(&points).unwrap(), -1.0);
    }

    #[test]
    fn training_converges_to_finite_loss() {
        let mut cnn = MouseTrajectoryCnn::new().unwrap();
        let human = human_path(120);
        let bots = vec![bot_segment_constant(); 4];
        let loss = cnn.train(&human, &bots, 40).unwrap();
        assert!(loss.is_finite());
        assert!(cnn.is_trained());
    }

    #[test]
    fn weights_roundtrip() {
        let mut cnn = MouseTrajectoryCnn::new().unwrap();
        let human = human_path(120);
        let bots = vec![bot_segment_constant(); 2];
        cnn.train(&human, &bots, 20).unwrap();
        let prob_before = cnn.predict(&human).unwrap();
        let w = cnn.export_weights().unwrap();
        let cnn2 = MouseTrajectoryCnn::from_weights(&w).unwrap();
        let prob_after = cnn2.predict(&human).unwrap();
        assert!(
            (prob_before - prob_after).abs() < 1e-3,
            "prob drifted after roundtrip: {prob_before} vs {prob_after}"
        );
    }

    /// Guard the F32-only invariant. The Android build forces
    /// `-C target-feature=+fullfp16` so `gemm-f16` compiles (its fp16 NEON
    /// intrinsics lack `#[target_feature]`). That is only safe because no
    /// f16 kernel ever executes — an f16 dtype here would make the fp16
    /// path live and SIGILL on arm64 devices without ARMv8.2-FP16.
    /// See `scripts/android-build.sh`.
    #[test]
    fn model_params_are_f32_only() {
        let cnn = MouseTrajectoryCnn::new().unwrap();
        for var in cnn.varmap.all_vars() {
            assert_eq!(var.dtype(), DType::F32, "sentinel ML must stay F32-only");
        }
    }
}
