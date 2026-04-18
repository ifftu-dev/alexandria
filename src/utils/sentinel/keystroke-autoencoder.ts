/**
 * Keystroke Autoencoder — Per-user anomaly detection via reconstruction error.
 *
 * Architecture: A small autoencoder (input→8→4→8→input) trained on the user's
 * own digraph timing data collected during calibration and assessments.
 *
 * Input features (per digraph pair):
 *   [dwellMs1, dwellMs2, flightMs, speedRatio]
 *
 * The model learns to compress and reconstruct the user's typing patterns.
 * At inference time, high reconstruction error indicates the typing doesn't
 * match the enrolled user — yielding an anomaly score between 0 and 1.
 *
 * All computation happens client-side. Weights are stored in localStorage
 * alongside the behavioral profile. No dependencies — pure TypeScript linear
 * algebra.
 *
 * PRIVACY: No raw keystrokes are ever stored. Only anonymized timing features
 * (dwell time, flight time) are used. The trained weights encode statistical
 * patterns, not recoverable keystroke data.
 */

// ============================================================================
// Types
// ============================================================================

/** Serializable autoencoder weights */
export interface AutoencoderWeights {
  w1: number[][]
  b1: number[]
  w2: number[][]
  b2: number[]
  w3: number[][]
  b3: number[]
  w4: number[][]
  b4: number[]
  trainedEpochs: number
  trainingSamples: number
  trainLoss: number
  featureMeans: number[]
  featureStds: number[]
}

/** A single digraph timing feature vector */
export interface DigraphFeatures {
  dwellMs1: number
  dwellMs2: number
  flightMs: number
  speedRatio: number
}

/** Raw keystroke event used for feature extraction */
export interface KeystrokeEvent {
  key: string
  dwellMs: number
  flightMs: number
}

// ============================================================================
// Constants
// ============================================================================

const INPUT_DIM = 4
const HIDDEN_DIM = 8
const LATENT_DIM = 4
const LEARNING_RATE = 0.005
const DEFAULT_EPOCHS = 80
const MIN_TRAINING_SAMPLES = 20
const ANOMALY_THRESHOLD = 0.65

// Contrastive / "push-away" pass against labeled attack patterns.
//
// After a normal reconstruction training run, we do CONTRASTIVE_EPOCHS
// extra passes over negative samples with flipped gradients, so the AE
// learns to reconstruct attack patterns poorly (raising their anomaly
// scores). A quarter learning rate keeps things numerically stable —
// gradient ascent with a full-size step tends to blow the network up.
//
// The margin is `5 × trainLoss`, matching the sigmoid calibration in
// `scoreFeatures`: once a negative's reconstruction error exceeds this,
// its anomaly score is already ≥ 0.5 and further push-away adds nothing.
const CONTRASTIVE_EPOCHS = 10
const CONTRASTIVE_LEARNING_RATE = LEARNING_RATE / 4
const CONTRASTIVE_MARGIN_MULT = 5

// ============================================================================
// Linear algebra primitives
// ============================================================================

function initWeight(rows: number, cols: number): number[][] {
  const scale = Math.sqrt(2.0 / (rows + cols))
  return Array.from({ length: rows }, () =>
    Array.from({ length: cols }, () => (Math.random() * 2 - 1) * scale),
  )
}

function initBias(size: number): number[] {
  return new Array(size).fill(0)
}

function matVecMul(W: number[][], x: number[], b: number[]): number[] {
  const out = new Array(W.length)
  for (let i = 0; i < W.length; i++) {
    let sum = b[i]!
    const row = W[i]!
    for (let j = 0; j < x.length; j++) {
      sum += row[j]! * x[j]!
    }
    out[i] = sum
  }
  return out
}

function relu(x: number[]): number[] {
  return x.map(v => Math.max(0, v))
}

function reluGrad(x: number[]): number[] {
  return x.map(v => v > 0 ? 1 : 0)
}

function mse(predicted: number[], target: number[]): number {
  let sum = 0
  for (let i = 0; i < predicted.length; i++) {
    const diff = predicted[i]! - target[i]!
    sum += diff * diff
  }
  return sum / predicted.length
}

// ============================================================================
// Feature extraction
// ============================================================================

export function extractDigraphFeatures(keystrokes: KeystrokeEvent[]): DigraphFeatures[] {
  const features: DigraphFeatures[] = []
  for (let i = 1; i < keystrokes.length; i++) {
    const prev = keystrokes[i - 1]!
    const curr = keystrokes[i]!
    if (prev.dwellMs <= 0 || curr.dwellMs <= 0) continue
    if (curr.flightMs <= 0) continue

    const totalPrev = prev.dwellMs + prev.flightMs
    const speedRatio = totalPrev > 0 ? curr.dwellMs / totalPrev : 1

    features.push({
      dwellMs1: prev.dwellMs,
      dwellMs2: curr.dwellMs,
      flightMs: curr.flightMs,
      speedRatio: Math.min(5, speedRatio),
    })
  }
  return features
}

function featureToVec(f: DigraphFeatures): number[] {
  return [f.dwellMs1, f.dwellMs2, f.flightMs, f.speedRatio]
}

function normalize(vec: number[], means: number[], stds: number[]): number[] {
  return vec.map((v, i) => {
    const std = stds[i]!
    return std > 0.001 ? (v - means[i]!) / std : 0
  })
}

function computeNormStats(data: number[][]): { means: number[]; stds: number[] } {
  const n = data.length
  const dim = data[0]!.length
  const means = new Array(dim).fill(0)
  const stds = new Array(dim).fill(0)

  for (const row of data) {
    for (let j = 0; j < dim; j++) {
      means[j] += row[j]!
    }
  }
  for (let j = 0; j < dim; j++) {
    means[j] /= n
  }

  for (const row of data) {
    for (let j = 0; j < dim; j++) {
      const diff = row[j]! - means[j]
      stds[j] += diff * diff
    }
  }
  for (let j = 0; j < dim; j++) {
    stds[j] = Math.sqrt(stds[j] / n)
  }

  return { means, stds }
}

// ============================================================================
// Autoencoder class
// ============================================================================

export class KeystrokeAutoencoder {
  private w1: number[][]
  private b1: number[]
  private w2: number[][]
  private b2: number[]
  private w3: number[][]
  private b3: number[]
  private w4: number[][]
  private b4: number[]
  private featureMeans: number[]
  private featureStds: number[]
  private trainedEpochs: number
  private trainingSamples: number
  private trainLoss: number

  constructor(weights?: AutoencoderWeights) {
    if (weights) {
      this.w1 = weights.w1
      this.b1 = weights.b1
      this.w2 = weights.w2
      this.b2 = weights.b2
      this.w3 = weights.w3
      this.b3 = weights.b3
      this.w4 = weights.w4
      this.b4 = weights.b4
      this.featureMeans = weights.featureMeans
      this.featureStds = weights.featureStds
      this.trainedEpochs = weights.trainedEpochs
      this.trainingSamples = weights.trainingSamples
      this.trainLoss = weights.trainLoss
    } else {
      this.w1 = initWeight(HIDDEN_DIM, INPUT_DIM)
      this.b1 = initBias(HIDDEN_DIM)
      this.w2 = initWeight(LATENT_DIM, HIDDEN_DIM)
      this.b2 = initBias(LATENT_DIM)
      this.w3 = initWeight(HIDDEN_DIM, LATENT_DIM)
      this.b3 = initBias(HIDDEN_DIM)
      this.w4 = initWeight(INPUT_DIM, HIDDEN_DIM)
      this.b4 = initBias(INPUT_DIM)
      this.featureMeans = new Array(INPUT_DIM).fill(0)
      this.featureStds = new Array(INPUT_DIM).fill(1)
      this.trainedEpochs = 0
      this.trainingSamples = 0
      this.trainLoss = 1.0
    }
  }

  private forward(x: number[]): {
    output: number[]
    z1Pre: number[]
    z1: number[]
    z2Pre: number[]
    z2: number[]
    z3Pre: number[]
    z3: number[]
  } {
    const z1Pre = matVecMul(this.w1, x, this.b1)
    const z1 = relu(z1Pre)
    const z2Pre = matVecMul(this.w2, z1, this.b2)
    const z2 = relu(z2Pre)
    const z3Pre = matVecMul(this.w3, z2, this.b3)
    const z3 = relu(z3Pre)
    const output = matVecMul(this.w4, z3, this.b4)
    return { output, z1Pre, z1, z2Pre, z2, z3Pre, z3 }
  }

  train(
    keystrokes: KeystrokeEvent[],
    epochs: number = DEFAULT_EPOCHS,
    negativeDigraphs: DigraphFeatures[] = [],
  ): number {
    const digraphs = extractDigraphFeatures(keystrokes)
    if (digraphs.length < MIN_TRAINING_SAMPLES) return -1

    const rawData = digraphs.map(featureToVec)
    const { means, stds } = computeNormStats(rawData)
    this.featureMeans = means
    this.featureStds = stds

    const data = rawData.map(row => normalize(row, means, stds))
    const indices = Array.from({ length: data.length }, (_, i) => i)
    let epochLoss = 1.0

    for (let epoch = 0; epoch < epochs; epoch++) {
      for (let i = indices.length - 1; i > 0; i--) {
        const j = Math.floor(Math.random() * (i + 1));
        [indices[i], indices[j]] = [indices[j]!, indices[i]!]
      }

      let totalLoss = 0

      for (const idx of indices) {
        const x = data[idx]!
        const { output, z1Pre, z1, z2Pre, z2, z3Pre, z3 } = this.forward(x)
        const dOutput = output.map((o, i) => (2 * (o - x[i]!)) / INPUT_DIM)
        totalLoss += mse(output, x)

        // Backprop through decoder layer 2
        const rg3 = reluGrad(z3Pre)
        const dz3 = new Array(HIDDEN_DIM).fill(0)
        for (let i = 0; i < HIDDEN_DIM; i++) {
          let sum = 0
          for (let j = 0; j < INPUT_DIM; j++) {
            sum += this.w4[j]![i]! * dOutput[j]!
          }
          dz3[i] = sum * rg3[i]!
        }
        for (let i = 0; i < INPUT_DIM; i++) {
          for (let j = 0; j < HIDDEN_DIM; j++) {
            this.w4[i]![j]! -= LEARNING_RATE * dOutput[i]! * z3[j]!
          }
          this.b4[i]! -= LEARNING_RATE * dOutput[i]!
        }

        // Backprop through decoder layer 1
        const rg2 = reluGrad(z2Pre)
        const dz2 = new Array(LATENT_DIM).fill(0)
        for (let i = 0; i < LATENT_DIM; i++) {
          let sum = 0
          for (let j = 0; j < HIDDEN_DIM; j++) {
            sum += this.w3[j]![i]! * dz3[j]!
          }
          dz2[i] = sum * rg2[i]!
        }
        for (let i = 0; i < HIDDEN_DIM; i++) {
          for (let j = 0; j < LATENT_DIM; j++) {
            this.w3[i]![j]! -= LEARNING_RATE * dz3[i]! * z2[j]!
          }
          this.b3[i]! -= LEARNING_RATE * dz3[i]!
        }

        // Backprop through encoder layer 2
        const rg1 = reluGrad(z1Pre)
        const dz1 = new Array(HIDDEN_DIM).fill(0)
        for (let i = 0; i < HIDDEN_DIM; i++) {
          let sum = 0
          for (let j = 0; j < LATENT_DIM; j++) {
            sum += this.w2[j]![i]! * dz2[j]!
          }
          dz1[i] = sum * rg1[i]!
        }
        for (let i = 0; i < LATENT_DIM; i++) {
          for (let j = 0; j < HIDDEN_DIM; j++) {
            this.w2[i]![j]! -= LEARNING_RATE * dz2[i]! * z1[j]!
          }
          this.b2[i]! -= LEARNING_RATE * dz2[i]!
        }

        // Backprop through encoder layer 1
        for (let i = 0; i < HIDDEN_DIM; i++) {
          for (let j = 0; j < INPUT_DIM; j++) {
            this.w1[i]![j]! -= LEARNING_RATE * dz1[i]! * x[j]!
          }
          this.b1[i]! -= LEARNING_RATE * dz1[i]!
        }
      }

      epochLoss = totalLoss / data.length
    }

    this.trainedEpochs += epochs
    this.trainingSamples = digraphs.length
    this.trainLoss = epochLoss

    // Optional contrastive pass against labeled attack patterns (see
    // docs/sentinel-adversarial-priors.md). Does nothing when the prior
    // library is empty — callers pass `[]` by default.
    if (negativeDigraphs.length > 0) {
      this.trainContrastive(negativeDigraphs)
    }

    return epochLoss
  }

  /**
   * "Push away" the AE's reconstruction of labeled attack patterns.
   *
   * For each negative sample, we run a normal forward+backprop but
   * flip the gradient sign (gradient ascent) with a reduced learning
   * rate. A margin gate skips samples that already reconstruct poorly
   * enough — no point destabilizing the network pushing something
   * even further from the manifold.
   */
  private trainContrastive(negativeDigraphs: DigraphFeatures[]): void {
    if (negativeDigraphs.length === 0) return
    const margin = this.trainLoss * CONTRASTIVE_MARGIN_MULT
    const rawData = negativeDigraphs.map(featureToVec)
    const data = rawData.map(row => normalize(row, this.featureMeans, this.featureStds))

    for (let epoch = 0; epoch < CONTRASTIVE_EPOCHS; epoch++) {
      for (const x of data) {
        const { output, z1Pre, z1, z2Pre, z2, z3Pre, z3 } = this.forward(x)
        const currentLoss = mse(output, x)
        // Margin gate: once the negative is already "far enough" in
        // reconstruction-error terms, stop pushing — further ascent
        // risks degrading legitimate-user reconstruction.
        if (currentLoss >= margin) continue

        const dOutput = output.map((o, i) => (2 * (o - x[i]!)) / INPUT_DIM)

        // Backprop through decoder layer 2, gradient-ASCEND (sign flipped).
        const rg3 = reluGrad(z3Pre)
        const dz3 = new Array(HIDDEN_DIM).fill(0)
        for (let i = 0; i < HIDDEN_DIM; i++) {
          let sum = 0
          for (let j = 0; j < INPUT_DIM; j++) {
            sum += this.w4[j]![i]! * dOutput[j]!
          }
          dz3[i] = sum * rg3[i]!
        }
        for (let i = 0; i < INPUT_DIM; i++) {
          for (let j = 0; j < HIDDEN_DIM; j++) {
            this.w4[i]![j]! += CONTRASTIVE_LEARNING_RATE * dOutput[i]! * z3[j]!
          }
          this.b4[i]! += CONTRASTIVE_LEARNING_RATE * dOutput[i]!
        }

        // Backprop through decoder layer 1
        const rg2 = reluGrad(z2Pre)
        const dz2 = new Array(LATENT_DIM).fill(0)
        for (let i = 0; i < LATENT_DIM; i++) {
          let sum = 0
          for (let j = 0; j < HIDDEN_DIM; j++) {
            sum += this.w3[j]![i]! * dz3[j]!
          }
          dz2[i] = sum * rg2[i]!
        }
        for (let i = 0; i < HIDDEN_DIM; i++) {
          for (let j = 0; j < LATENT_DIM; j++) {
            this.w3[i]![j]! += CONTRASTIVE_LEARNING_RATE * dz3[i]! * z2[j]!
          }
          this.b3[i]! += CONTRASTIVE_LEARNING_RATE * dz3[i]!
        }

        // Backprop through encoder layer 2
        const rg1 = reluGrad(z1Pre)
        const dz1 = new Array(HIDDEN_DIM).fill(0)
        for (let i = 0; i < HIDDEN_DIM; i++) {
          let sum = 0
          for (let j = 0; j < LATENT_DIM; j++) {
            sum += this.w2[j]![i]! * dz2[j]!
          }
          dz1[i] = sum * rg1[i]!
        }
        for (let i = 0; i < LATENT_DIM; i++) {
          for (let j = 0; j < HIDDEN_DIM; j++) {
            this.w2[i]![j]! += CONTRASTIVE_LEARNING_RATE * dz2[i]! * z1[j]!
          }
          this.b2[i]! += CONTRASTIVE_LEARNING_RATE * dz2[i]!
        }

        // Backprop through encoder layer 1
        for (let i = 0; i < HIDDEN_DIM; i++) {
          for (let j = 0; j < INPUT_DIM; j++) {
            this.w1[i]![j]! += CONTRASTIVE_LEARNING_RATE * dz1[i]! * x[j]!
          }
          this.b1[i]! += CONTRASTIVE_LEARNING_RATE * dz1[i]!
        }
      }
    }
  }

  score(keystrokes: KeystrokeEvent[]): number {
    if (this.trainedEpochs === 0) return -1
    const digraphs = extractDigraphFeatures(keystrokes)
    return this.scoreFeatures(digraphs)
  }

  /**
   * Score pre-extracted digraph features directly, bypassing the
   * KeystrokeEvent → DigraphFeatures conversion. Useful for scoring
   * blobs from the Sentinel DAO prior library whose samples are
   * already feature-shaped.
   *
   * Returns the same [0,1] anomaly score as `score()`: higher means
   * the input reconstructs poorly, i.e. doesn't look like the
   * enrolled user.
   */
  scoreFeatures(digraphs: DigraphFeatures[]): number {
    if (this.trainedEpochs === 0) return -1
    if (digraphs.length < 5) return -1

    const rawData = digraphs.map(featureToVec)
    const data = rawData.map(row => normalize(row, this.featureMeans, this.featureStds))

    let totalError = 0
    for (const x of data) {
      const { output } = this.forward(x)
      totalError += mse(output, x)
    }

    const avgError = totalError / data.length
    // Floor prevents ratio blow-up when trainLoss is near-zero (users with
    // very consistent typing) — without this, every legitimate keystroke
    // would score as anomalous.
    const baseline = Math.max(this.trainLoss, 0.05)
    const ratio = avgError / baseline
    const anomalyScore = 1 / (1 + Math.exp(-0.5 * (ratio - 5)))
    return Math.min(1, Math.max(0, anomalyScore))
  }

  get isTrained(): boolean {
    return this.trainedEpochs > 0 && this.trainingSamples >= MIN_TRAINING_SAMPLES
  }

  isAnomalous(score: number): boolean {
    return score >= ANOMALY_THRESHOLD
  }

  exportWeights(): AutoencoderWeights {
    return {
      w1: this.w1, b1: this.b1,
      w2: this.w2, b2: this.b2,
      w3: this.w3, b3: this.b3,
      w4: this.w4, b4: this.b4,
      featureMeans: this.featureMeans,
      featureStds: this.featureStds,
      trainedEpochs: this.trainedEpochs,
      trainingSamples: this.trainingSamples,
      trainLoss: this.trainLoss,
    }
  }

  getStats(): { epochs: number; samples: number; loss: number } {
    return {
      epochs: this.trainedEpochs,
      samples: this.trainingSamples,
      loss: this.trainLoss,
    }
  }
}
