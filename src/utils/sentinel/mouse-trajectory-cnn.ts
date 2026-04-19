/**
 * Mouse Trajectory CNN — 1D Convolutional Neural Network for human vs bot
 * classification of mouse movement trajectories.
 *
 * Architecture:
 *   Input: 50-point trajectory segments, 3 channels (dx, dy, dt)
 *   Conv1D(3→8, kernel=5) + ReLU + MaxPool(2)
 *   Conv1D(8→16, kernel=3) + ReLU + MaxPool(2)
 *   Flatten → Dense(160→32) + ReLU → Dense(32→1) + Sigmoid
 *
 * Training: Human data from calibration + synthetic bot trajectories.
 * Conv layers use reservoir computing (random fixed features) for fast
 * on-device training. Dense layers learn the classification boundary.
 *
 * PRIVACY: Only derived movement deltas (dx, dy, dt) are processed.
 * No absolute coordinates are stored or transmitted.
 */

// ============================================================================
// Types
// ============================================================================

export interface MousePoint {
  x: number
  y: number
  t: number
}

export interface TrajectorySegment {
  channels: number[][]
  label: number
}

export interface MouseCNNWeights {
  conv1Weights: number[][][]
  conv1Bias: number[]
  conv2Weights: number[][][]
  conv2Bias: number[]
  dense1Weights: number[][]
  dense1Bias: number[]
  dense2Weights: number[][]
  dense2Bias: number[]
  trainedEpochs: number
  trainingSamples: number
  trainLoss: number
}

// ============================================================================
// Constants
// ============================================================================

const SEGMENT_LENGTH = 50
const CONV1_FILTERS = 8
const CONV1_KERNEL = 5
const CONV1_IN_CHANNELS = 3
const CONV2_FILTERS = 16
const CONV2_KERNEL = 3
const POOL_SIZE = 2
const DENSE1_OUT = 32
const LEARNING_RATE = 0.002
const DEFAULT_EPOCHS = 40
const BOT_PATTERNS = 5
const HUMAN_THRESHOLD = 0.5

const AFTER_CONV1 = SEGMENT_LENGTH - CONV1_KERNEL + 1
const AFTER_POOL1 = Math.floor(AFTER_CONV1 / POOL_SIZE)
const AFTER_CONV2 = AFTER_POOL1 - CONV2_KERNEL + 1
const AFTER_POOL2 = Math.floor(AFTER_CONV2 / POOL_SIZE)
const FLATTEN_DIM = AFTER_POOL2 * CONV2_FILTERS

// ============================================================================
// Initialization
// ============================================================================

function initConvFilters(numFilters: number, inChannels: number, kernelSize: number): number[][][] {
  const scale = Math.sqrt(2.0 / (inChannels * kernelSize))
  return Array.from({ length: numFilters }, () =>
    Array.from({ length: inChannels }, () =>
      Array.from({ length: kernelSize }, () => (Math.random() * 2 - 1) * scale),
    ),
  )
}

function initDense(rows: number, cols: number): number[][] {
  const scale = Math.sqrt(2.0 / (rows + cols))
  return Array.from({ length: rows }, () =>
    Array.from({ length: cols }, () => (Math.random() * 2 - 1) * scale),
  )
}

function zeros(n: number): number[] {
  return new Array(n).fill(0)
}

// ============================================================================
// CNN operations
// ============================================================================

function conv1d(input: number[][], filters: number[][][], bias: number[]): number[][] {
  const inLen = input[0]!.length
  const outChannels = filters.length
  const kernelSize = filters[0]![0]!.length
  const outLen = inLen - kernelSize + 1
  const output: number[][] = Array.from({ length: outChannels }, () => zeros(outLen))

  for (let oc = 0; oc < outChannels; oc++) {
    for (let pos = 0; pos < outLen; pos++) {
      let sum = bias[oc]!
      for (let ic = 0; ic < input.length; ic++) {
        for (let k = 0; k < kernelSize; k++) {
          sum += filters[oc]![ic]![k]! * input[ic]![pos + k]!
        }
      }
      output[oc]![pos] = sum
    }
  }
  return output
}

function relu2d(x: number[][]): number[][] {
  return x.map(row => row.map(v => Math.max(0, v)))
}

function maxPool1d(input: number[][], poolSize: number): { output: number[][]; indices: number[][] } {
  const channels = input.length
  const inLen = input[0]!.length
  const outLen = Math.floor(inLen / poolSize)
  const output: number[][] = Array.from({ length: channels }, () => zeros(outLen))
  const indices: number[][] = Array.from({ length: channels }, () => zeros(outLen))

  for (let c = 0; c < channels; c++) {
    for (let i = 0; i < outLen; i++) {
      let maxVal = -Infinity
      let maxIdx = 0
      for (let j = 0; j < poolSize; j++) {
        const idx = i * poolSize + j
        if (input[c]![idx]! > maxVal) {
          maxVal = input[c]![idx]!
          maxIdx = idx
        }
      }
      output[c]![i] = maxVal
      indices[c]![i] = maxIdx
    }
  }
  return { output, indices }
}

function flatten(input: number[][]): number[] {
  const result: number[] = []
  for (const channel of input) {
    result.push(...channel)
  }
  return result
}

function dense(input: number[], weights: number[][], bias: number[]): number[] {
  const out = new Array(weights.length)
  for (let i = 0; i < weights.length; i++) {
    let sum = bias[i]!
    const row = weights[i]!
    for (let j = 0; j < input.length; j++) {
      sum += row[j]! * input[j]!
    }
    out[i] = sum
  }
  return out
}

function relu1d(x: number[]): number[] {
  return x.map(v => Math.max(0, v))
}

function sigmoidScalar(x: number): number {
  return 1 / (1 + Math.exp(-Math.max(-500, Math.min(500, x))))
}

// ============================================================================
// Trajectory feature extraction
// ============================================================================

export function extractTrajectorySegments(points: MousePoint[], label: number): TrajectorySegment[] {
  if (points.length < SEGMENT_LENGTH + 1) return []

  const segments: TrajectorySegment[] = []
  const stride = Math.floor(SEGMENT_LENGTH / 2)

  for (let start = 0; start + SEGMENT_LENGTH < points.length; start += stride) {
    const channels: number[][] = Array.from({ length: SEGMENT_LENGTH }, () => [0, 0, 0])

    let maxDist = 0
    let maxDt = 0

    for (let i = 0; i < SEGMENT_LENGTH; i++) {
      const curr = points[start + i + 1]!
      const prev = points[start + i]!
      const dx = curr.x - prev.x
      const dy = curr.y - prev.y
      const dt = Math.max(1, curr.t - prev.t)
      const dist = Math.sqrt(dx * dx + dy * dy)
      if (dist > maxDist) maxDist = dist
      if (dt > maxDt) maxDt = dt
      channels[i] = [dx, dy, dt]
    }

    const distScale = maxDist > 0 ? maxDist : 1
    const dtScale = maxDt > 0 ? maxDt : 1
    for (let i = 0; i < SEGMENT_LENGTH; i++) {
      const ch = channels[i]!
      ch[0] = ch[0]! / distScale
      ch[1] = ch[1]! / distScale
      ch[2] = ch[2]! / dtScale
    }

    segments.push({ channels, label })
  }

  return segments
}

export function generateBotTrajectories(count: number): TrajectorySegment[] {
  const segments: TrajectorySegment[] = []

  for (let i = 0; i < count; i++) {
    const pattern = i % BOT_PATTERNS
    const channels: number[][] = []

    for (let step = 0; step < SEGMENT_LENGTH; step++) {
      switch (pattern) {
        case 0: channels.push([0.5, 0.5, 0.5]); break
        case 1: channels.push([step / SEGMENT_LENGTH, step / SEGMENT_LENGTH, 1 / SEGMENT_LENGTH]); break
        case 2: channels.push([Math.sin(step * 0.2), Math.cos(step * 0.2), 0.5]); break
        case 3: channels.push([0.3 + (Math.random() - 0.5) * 0.02, 0.3 + (Math.random() - 0.5) * 0.02, 0.5 + (Math.random() - 0.5) * 0.01]); break
        case 4: channels.push([step % 10 === 0 ? 1.0 : 0.0, step % 10 === 0 ? 1.0 : 0.0, step % 10 === 0 ? 0.01 : 0.99]); break
      }
    }

    segments.push({ channels, label: 0 })
  }

  return segments
}

// ============================================================================
// CNN class
// ============================================================================

export class MouseTrajectoryCNN {
  private conv1W: number[][][]
  private conv1B: number[]
  private conv2W: number[][][]
  private conv2B: number[]
  private dense1W: number[][]
  private dense1B: number[]
  private dense2W: number[][]
  private dense2B: number[]
  private trainedEpochs: number
  private trainingSamples: number
  private trainLoss: number

  constructor(weights?: MouseCNNWeights) {
    if (weights) {
      this.conv1W = weights.conv1Weights
      this.conv1B = weights.conv1Bias
      this.conv2W = weights.conv2Weights
      this.conv2B = weights.conv2Bias
      this.dense1W = weights.dense1Weights
      this.dense1B = weights.dense1Bias
      this.dense2W = weights.dense2Weights
      this.dense2B = weights.dense2Bias
      this.trainedEpochs = weights.trainedEpochs
      this.trainingSamples = weights.trainingSamples
      this.trainLoss = weights.trainLoss
    } else {
      this.conv1W = initConvFilters(CONV1_FILTERS, CONV1_IN_CHANNELS, CONV1_KERNEL)
      this.conv1B = zeros(CONV1_FILTERS)
      this.conv2W = initConvFilters(CONV2_FILTERS, CONV1_FILTERS, CONV2_KERNEL)
      this.conv2B = zeros(CONV2_FILTERS)
      this.dense1W = initDense(DENSE1_OUT, FLATTEN_DIM)
      this.dense1B = zeros(DENSE1_OUT)
      this.dense2W = initDense(1, DENSE1_OUT)
      this.dense2B = zeros(1)
      this.trainedEpochs = 0
      this.trainingSamples = 0
      this.trainLoss = 1.0
    }
  }

  private forward(segment: number[][]): {
    prob: number
    flatOut: number[]
    dense1Pre: number[]
    dense1Out: number[]
  } {
    const input: number[][] = Array.from({ length: CONV1_IN_CHANNELS }, (_, c) =>
      Array.from({ length: SEGMENT_LENGTH }, (__, s) => segment[s]![c]!),
    )

    const conv1Pre = conv1d(input, this.conv1W, this.conv1B)
    const conv1Out = relu2d(conv1Pre)
    const { output: pool1Out } = maxPool1d(conv1Out, POOL_SIZE)
    const conv2Pre = conv1d(pool1Out, this.conv2W, this.conv2B)
    const conv2Out = relu2d(conv2Pre)
    const { output: pool2Out } = maxPool1d(conv2Out, POOL_SIZE)
    const flatOut = flatten(pool2Out)
    const dense1Pre = dense(flatOut, this.dense1W, this.dense1B)
    const dense1Out = relu1d(dense1Pre)
    const dense2Pre = dense(dense1Out, this.dense2W, this.dense2B)[0]!
    const prob = sigmoidScalar(dense2Pre)

    return { prob, flatOut, dense1Pre, dense1Out }
  }

  train(
    humanPoints: MousePoint[],
    epochs: number = DEFAULT_EPOCHS,
    additionalBotTrajectories: MousePoint[][] = [],
  ): number {
    const humanSegments = extractTrajectorySegments(humanPoints, 1)
    if (humanSegments.length < 2) return -1

    const botSegments = generateBotTrajectories(humanSegments.length * BOT_PATTERNS)
    // Ratified adversarial priors from the Sentinel DAO contribute extra
    // bot-class segments (label=0). See docs/sentinel-adversarial-priors.md.
    // Falls back to synthetics alone when no priors have synced yet.
    const ratifiedBotSegments: TrajectorySegment[] = []
    for (const traj of additionalBotTrajectories) {
      ratifiedBotSegments.push(...extractTrajectorySegments(traj, 0))
    }
    const allSegments = [...humanSegments, ...botSegments, ...ratifiedBotSegments]
    const indices = Array.from({ length: allSegments.length }, (_, i) => i)
    let epochLoss = 1.0

    for (let epoch = 0; epoch < epochs; epoch++) {
      for (let i = indices.length - 1; i > 0; i--) {
        const j = Math.floor(Math.random() * (i + 1));
        [indices[i], indices[j]] = [indices[j]!, indices[i]!]
      }

      let totalLoss = 0

      for (const idx of indices) {
        const seg = allSegments[idx]!
        const target = seg.label
        const { prob, dense1Out, dense1Pre, flatOut } = this.forward(seg.channels)

        const eps = 1e-7
        const clampedProb = Math.max(eps, Math.min(1 - eps, prob))
        const loss = -(target * Math.log(clampedProb) + (1 - target) * Math.log(1 - clampedProb))
        totalLoss += loss

        const dDense2 = prob - target

        for (let j = 0; j < DENSE1_OUT; j++) {
          this.dense2W[0]![j]! -= LEARNING_RATE * dDense2 * dense1Out[j]!
        }
        this.dense2B[0]! -= LEARNING_RATE * dDense2

        const dDense1 = new Array(DENSE1_OUT)
        for (let i = 0; i < DENSE1_OUT; i++) {
          dDense1[i] = dDense2 * this.dense2W[0]![i]! * (dense1Pre[i]! > 0 ? 1 : 0)
        }

        for (let i = 0; i < DENSE1_OUT; i++) {
          for (let j = 0; j < FLATTEN_DIM; j++) {
            this.dense1W[i]![j]! -= LEARNING_RATE * dDense1[i] * flatOut[j]!
          }
          this.dense1B[i]! -= LEARNING_RATE * dDense1[i]
        }
        // Conv layers: reservoir computing (fixed random features, no backprop)
      }

      epochLoss = totalLoss / allSegments.length
    }

    this.trainedEpochs += epochs
    this.trainingSamples = allSegments.length
    this.trainLoss = epochLoss
    return epochLoss
  }

  predict(points: MousePoint[]): number {
    if (this.trainedEpochs === 0) return -1

    const segments = extractTrajectorySegments(points, 0)
    if (segments.length === 0) return -1

    let totalProb = 0
    for (const seg of segments) {
      const { prob } = this.forward(seg.channels)
      totalProb += prob
    }

    return totalProb / segments.length
  }

  isHuman(prob: number): boolean {
    return prob >= HUMAN_THRESHOLD
  }

  get isTrained(): boolean {
    return this.trainedEpochs > 0 && this.trainingSamples > 0
  }

  exportWeights(): MouseCNNWeights {
    return {
      conv1Weights: this.conv1W, conv1Bias: this.conv1B,
      conv2Weights: this.conv2W, conv2Bias: this.conv2B,
      dense1Weights: this.dense1W, dense1Bias: this.dense1B,
      dense2Weights: this.dense2W, dense2Bias: this.dense2B,
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
