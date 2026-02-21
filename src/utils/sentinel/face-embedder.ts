/**
 * Face Embedder — Lightweight face verification using Local Binary Pattern
 * (LBP) histograms with spatial binning.
 *
 * Algorithm:
 *   1. Detect face region using skin-color segmentation in YCbCr
 *   2. Convert face ROI to grayscale
 *   3. Compute LBP for each pixel (8-bit code encoding local texture)
 *   4. Divide the face into a 4x4 spatial grid
 *   5. Compute a 59-bin uniform LBP histogram per grid cell
 *   6. Concatenate → 944-dimensional embedding
 *   7. L2-normalize
 *
 * Verification: cosine similarity between enrollment and live embedding.
 * Threshold of 0.70 for same-person match.
 *
 * PRIVACY: All processing happens on-device. Only the derived embedding
 * (a vector of floats with no visual meaning) is stored in localStorage.
 */

// ============================================================================
// Types
// ============================================================================

export interface FaceEmbedding {
  vector: number[]
  capturedAt: number
  faceDetected: boolean
  faceCount: number
  skinRatio: number
}

export interface EnrollmentEmbedding {
  vector: number[]
  frameCount: number
  updatedAt: number
}

// ============================================================================
// Constants
// ============================================================================

const FRAME_WIDTH = 320
const FRAME_HEIGHT = 240
const GRID_ROWS = 4
const GRID_COLS = 4
const UNIFORM_LBP_BINS = 59
const EMBEDDING_DIM = GRID_ROWS * GRID_COLS * UNIFORM_LBP_BINS // 944
const SIMILARITY_THRESHOLD = 0.70
const ENROLLMENT_FRAMES = 5
const FACE_MIN_RATIO = 0.04
const FACE_MAX_RATIO = 0.85

// ============================================================================
// Uniform LBP lookup table
// ============================================================================

function buildUniformLBPTable(): Uint8Array {
  const table = new Uint8Array(256)
  let uniformIndex = 0

  for (let i = 0; i < 256; i++) {
    let transitions = 0
    for (let bit = 0; bit < 8; bit++) {
      const curr = (i >> bit) & 1
      const next = (i >> ((bit + 1) % 8)) & 1
      if (curr !== next) transitions++
    }

    if (transitions <= 2) {
      table[i] = uniformIndex++
    } else {
      table[i] = UNIFORM_LBP_BINS - 1
    }
  }

  return table
}

const UNIFORM_TABLE = buildUniformLBPTable()

// ============================================================================
// Image processing
// ============================================================================

function toGrayscale(data: Uint8ClampedArray, width: number, height: number): Uint8Array {
  const gray = new Uint8Array(width * height)
  for (let i = 0; i < width * height; i++) {
    const r = data[i * 4]!
    const g = data[i * 4 + 1]!
    const b = data[i * 4 + 2]!
    gray[i] = Math.round(0.299 * r + 0.587 * g + 0.114 * b)
  }
  return gray
}

function detectFaceRegion(
  data: Uint8ClampedArray,
  width: number,
  height: number,
): { x: number; y: number; w: number; h: number; skinRatio: number; faceCount: number } | null {
  const skinMask = new Uint8Array(width * height)
  let skinPixels = 0

  for (let i = 0; i < width * height; i++) {
    const r = data[i * 4]!
    const g = data[i * 4 + 1]!
    const b = data[i * 4 + 2]!
    const y = 0.299 * r + 0.587 * g + 0.114 * b
    const cb = 128 - 0.168736 * r - 0.331264 * g + 0.5 * b
    const cr = 128 + 0.5 * r - 0.418688 * g - 0.081312 * b

    if (y > 80 && cb > 77 && cb < 127 && cr > 133 && cr < 173) {
      skinMask[i] = 1
      skinPixels++
    }
  }

  const skinRatio = skinPixels / (width * height)
  if (skinRatio < FACE_MIN_RATIO || skinRatio > FACE_MAX_RATIO) return null

  let minX = width, minY = height, maxX = 0, maxY = 0

  for (let row = 0; row < height; row++) {
    for (let col = 0; col < width; col++) {
      if (skinMask[row * width + col]) {
        if (col < minX) minX = col
        if (col > maxX) maxX = col
        if (row < minY) minY = row
        if (row > maxY) maxY = row
      }
    }
  }

  const stripWidth = Math.max(1, Math.floor(width / 8))
  let faceCount = 0
  let inFace = false
  let gapStrips = 0

  for (let strip = 0; strip < 8; strip++) {
    let stripSkin = 0
    const sx = strip * stripWidth
    for (let row = minY; row <= maxY; row++) {
      for (let col = sx; col < sx + stripWidth && col < width; col++) {
        if (skinMask[row * width + col]) stripSkin++
      }
    }
    const stripRatio = stripSkin / (stripWidth * Math.max(1, maxY - minY + 1))

    if (stripRatio > 0.15) {
      if (!inFace) { faceCount++; inFace = true }
      gapStrips = 0
    } else {
      gapStrips++
      if (gapStrips >= 2) inFace = false
    }
  }

  const bw = maxX - minX
  const bh = maxY - minY
  const padX = Math.floor(bw * 0.1)
  const padY = Math.floor(bh * 0.1)

  return {
    x: Math.max(0, minX - padX),
    y: Math.max(0, minY - padY),
    w: Math.min(width - minX + padX, bw + 2 * padX),
    h: Math.min(height - minY + padY, bh + 2 * padY),
    skinRatio,
    faceCount: Math.max(1, faceCount),
  }
}

function computeLBP(gray: Uint8Array, width: number, row: number, col: number): number {
  const center = gray[row * width + col]!
  let code = 0
  const offsets = [[-1, -1], [-1, 0], [-1, 1], [0, 1], [1, 1], [1, 0], [1, -1], [0, -1]]
  for (let i = 0; i < 8; i++) {
    const nr = row + offsets[i]![0]!
    const nc = col + offsets[i]![1]!
    if (nr >= 0 && nr < Math.floor(gray.length / width) && nc >= 0 && nc < width) {
      if (gray[nr * width + nc]! >= center) {
        code |= (1 << i)
      }
    }
  }
  return code
}

function l2Normalize(vec: number[]): number[] {
  let norm = 0
  for (const v of vec) norm += v * v
  norm = Math.sqrt(norm)
  if (norm < 1e-10) return vec
  return vec.map(v => v / norm)
}

function cosineSimilarity(a: number[], b: number[]): number {
  if (a.length !== b.length) return 0
  let dot = 0
  for (let i = 0; i < a.length; i++) {
    dot += a[i]! * b[i]!
  }
  return dot
}

// ============================================================================
// Face Embedder class
// ============================================================================

export class FaceEmbedder {
  private enrollmentEmbedding: EnrollmentEmbedding | null = null
  private canvas: HTMLCanvasElement | null = null
  private ctx: CanvasRenderingContext2D | null = null

  constructor(enrollment?: EnrollmentEmbedding) {
    if (enrollment) this.enrollmentEmbedding = enrollment
  }

  private ensureCanvas(): { canvas: HTMLCanvasElement; ctx: CanvasRenderingContext2D } {
    if (!this.canvas || !this.ctx) {
      this.canvas = document.createElement('canvas')
      this.canvas.width = FRAME_WIDTH
      this.canvas.height = FRAME_HEIGHT
      this.ctx = this.canvas.getContext('2d', { willReadFrequently: true })!
    }
    return { canvas: this.canvas, ctx: this.ctx }
  }

  embed(video: HTMLVideoElement): FaceEmbedding | null {
    if (!video || video.readyState < 2) return null

    const { ctx } = this.ensureCanvas()
    ctx.drawImage(video, 0, 0, FRAME_WIDTH, FRAME_HEIGHT)
    const imageData = ctx.getImageData(0, 0, FRAME_WIDTH, FRAME_HEIGHT)
    return this.embedFromImageData(imageData)
  }

  embedFromImageData(imageData: ImageData): FaceEmbedding | null {
    const { data, width, height } = imageData

    const faceRegion = detectFaceRegion(data, width, height)
    if (!faceRegion) {
      return {
        vector: new Array(EMBEDDING_DIM).fill(0),
        capturedAt: Date.now(),
        faceDetected: false,
        faceCount: 0,
        skinRatio: 0,
      }
    }

    const gray = toGrayscale(data, width, height)
    const histogram = this.computeSpatialLBPHistogram(
      gray, width,
      faceRegion.x, faceRegion.y, faceRegion.w, faceRegion.h,
    )
    const embedding = l2Normalize(histogram)

    return {
      vector: embedding,
      capturedAt: Date.now(),
      faceDetected: true,
      faceCount: faceRegion.faceCount,
      skinRatio: faceRegion.skinRatio,
    }
  }

  private computeSpatialLBPHistogram(
    gray: Uint8Array,
    imgWidth: number,
    roiX: number,
    roiY: number,
    roiW: number,
    roiH: number,
  ): number[] {
    const cellW = Math.max(1, Math.floor(roiW / GRID_COLS))
    const cellH = Math.max(1, Math.floor(roiH / GRID_ROWS))
    const histogram = new Array(EMBEDDING_DIM).fill(0)

    for (let gr = 0; gr < GRID_ROWS; gr++) {
      for (let gc = 0; gc < GRID_COLS; gc++) {
        const cellStartY = roiY + gr * cellH
        const cellStartX = roiX + gc * cellW
        const cellEndY = Math.min(cellStartY + cellH, roiY + roiH)
        const cellEndX = Math.min(cellStartX + cellW, roiX + roiW)
        const histOffset = (gr * GRID_COLS + gc) * UNIFORM_LBP_BINS

        for (let row = cellStartY + 1; row < cellEndY - 1; row++) {
          for (let col = cellStartX + 1; col < cellEndX - 1; col++) {
            const lbpCode = computeLBP(gray, imgWidth, row, col)
            const bin = UNIFORM_TABLE[lbpCode]!
            histogram[histOffset + bin]!++
          }
        }

        let cellSum = 0
        for (let b = 0; b < UNIFORM_LBP_BINS; b++) {
          cellSum += histogram[histOffset + b]!
        }
        if (cellSum > 0) {
          for (let b = 0; b < UNIFORM_LBP_BINS; b++) {
            histogram[histOffset + b]! /= cellSum
          }
        }
      }
    }

    return histogram
  }

  enroll(video: HTMLVideoElement): boolean {
    const embedding = this.embed(video)
    if (!embedding || !embedding.faceDetected) return false

    if (!this.enrollmentEmbedding) {
      this.enrollmentEmbedding = {
        vector: embedding.vector,
        frameCount: 1,
        updatedAt: Date.now(),
      }
    } else {
      const n = this.enrollmentEmbedding.frameCount
      const newVec = new Array(EMBEDDING_DIM)
      for (let i = 0; i < EMBEDDING_DIM; i++) {
        newVec[i] = (this.enrollmentEmbedding.vector[i]! * n + embedding.vector[i]!) / (n + 1)
      }
      this.enrollmentEmbedding.vector = l2Normalize(newVec)
      this.enrollmentEmbedding.frameCount = n + 1
      this.enrollmentEmbedding.updatedAt = Date.now()
    }

    return true
  }

  verify(video: HTMLVideoElement): {
    similarity: number
    isMatch: boolean
    faceDetected: boolean
    faceCount: number
  } | null {
    if (!this.enrollmentEmbedding || this.enrollmentEmbedding.frameCount < 1) return null

    const embedding = this.embed(video)
    if (!embedding) return null

    if (!embedding.faceDetected) {
      return { similarity: 0, isMatch: false, faceDetected: false, faceCount: 0 }
    }

    const similarity = cosineSimilarity(this.enrollmentEmbedding.vector, embedding.vector)
    return {
      similarity,
      isMatch: similarity >= SIMILARITY_THRESHOLD,
      faceDetected: true,
      faceCount: embedding.faceCount,
    }
  }

  get isEnrolled(): boolean {
    return this.enrollmentEmbedding !== null && this.enrollmentEmbedding.frameCount >= ENROLLMENT_FRAMES
  }

  get enrollmentProgress(): number {
    if (!this.enrollmentEmbedding) return 0
    return Math.min(1, this.enrollmentEmbedding.frameCount / ENROLLMENT_FRAMES)
  }

  exportEnrollment(): EnrollmentEmbedding | null {
    return this.enrollmentEmbedding ? { ...this.enrollmentEmbedding } : null
  }

  static get threshold(): number {
    return SIMILARITY_THRESHOLD
  }

  static get embeddingDim(): number {
    return EMBEDDING_DIM
  }
}
