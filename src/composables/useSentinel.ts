import { ref, readonly } from 'vue'
import { invoke as tauriInvoke } from '@tauri-apps/api/core'
import { useLocalApi } from './useLocalApi'
import { useAuth } from './useAuth'
import {
  FaceEmbedder,
  type EnrollmentEmbedding,
} from '@/utils/sentinel/face-embedder'
import type {
  SignalData,
  BehavioralProfile,
  StartSessionResponse,
  SentinelPrior,
  SentinelPriorBlob,
  ActivePasteClassifier,
  KeystrokeEvent,
  MousePoint,
  DigraphFeatures,
  ScorePasteResponse,
  LoadedClassifierInfo,
  UserModelStatus,
  TrainKeystrokeAeResponse,
  TrainMouseCnnResponse,
} from '@/types'

// Threshold helpers were previously inlined in the TS classifier. With
// the backend rewrite they're plain constants — keep them client-side
// so flag promotion can run synchronously in `computeScores`.
const PASTE_ANOMALY_THRESHOLD = 0.95
const PASTE_ANOMALY_CRITICAL_THRESHOLD = 0.99
const KEYSTROKE_ANOMALY_THRESHOLD = 0.65
const MOUSE_HUMAN_THRESHOLD = 0.5

function isAnomalousPasteScore(s: number): boolean {
  return s >= PASTE_ANOMALY_THRESHOLD
}
function isCriticalPasteScore(s: number): boolean {
  return s >= PASTE_ANOMALY_CRITICAL_THRESHOLD
}

/**
 * Sentinel Engine — Client-side integrity monitoring composable.
 *
 * Tauri v2 implementation (no server HTTP — all via IPC).
 *
 * PRIVACY GUARANTEE: All biometric and behavioral data is processed entirely
 * within this composable. Only derived scores (0-1) cross to the Rust backend
 * via Tauri IPC. Raw keystroke timings, mouse coordinates, and video frames
 * are NEVER transmitted or stored beyond in-memory buffers.
 *
 * Two parallel scoring systems:
 *   1. RULE-BASED — Deterministic threshold checks, always active
 *   2. AI-BASED — Per-user trained models (keystroke AE, mouse CNN, face LBP)
 */

// ============================================================================
// Singleton state
// ============================================================================

const sessionId = ref<string | null>(null)
const isActive = ref(false)
const integrityScore = ref(1.0)
const consistencyScore = ref(1.0)
const cameraOptedIn = ref(false)

// AI scoring is advisory until validated with labeled data (see
// docs/sentinel.md §AI Models). Off by default; can be toggled per-device
// via setAIScoringEnabled(). When enabled, each available AI signal
// contributes a small advisory weight to the integrity score.
const AI_SCORING_STORAGE_KEY = 'sentinel_ai_scoring_enabled'
const AI_ADVISORY_WEIGHT = 0.05
const aiScoringEnabled = ref<boolean>(readAIScoringPref())

// Per-signal opt-out for the paste classifier. Defaults to true so it
// contributes when the master AI toggle is on; users can disable it
// alone if they hit false positives without losing the other AI
// signals.
const PASTE_CLASSIFIER_STORAGE_KEY = 'sentinel_paste_classifier_enabled'
const pasteClassifierEnabled = ref<boolean>(readPasteClassifierPref())

function readAIScoringPref(): boolean {
  try { return localStorage.getItem(AI_SCORING_STORAGE_KEY) === '1' }
  catch { return false }
}

function readPasteClassifierPref(): boolean {
  try {
    const v = localStorage.getItem(PASTE_CLASSIFIER_STORAGE_KEY)
    // Default ON unless explicitly disabled.
    return v !== '0'
  } catch {
    return true
  }
}

/**
 * Reconcile the AI/paste-classifier flags with the per-profile
 * settings store. Call once after profile unlock so the toggles
 * reflect the canonical sync'd value (and a fresh profile inherits
 * the localStorage cache).
 */
export async function initSentinelFlagsFromSettings(): Promise<void> {
  const { useSettings } = await import('./useSettings')
  const { entries, initialize, setSetting } = useSettings()
  await initialize()
  const ai = entries.value.find((e) => e.key === 'sentinel.ai_scoring_enabled')
  const paste = entries.value.find((e) => e.key === 'sentinel.paste_classifier_enabled')

  if (ai) {
    if (ai.is_default) {
      const local = localStorage.getItem(AI_SCORING_STORAGE_KEY)
      if (local !== null) await setSetting('sentinel.ai_scoring_enabled', local === '1' ? 'true' : 'false')
    } else {
      aiScoringEnabled.value = ai.current_value === 'true'
    }
  }
  if (paste) {
    if (paste.is_default) {
      const local = localStorage.getItem(PASTE_CLASSIFIER_STORAGE_KEY)
      if (local !== null) await setSetting('sentinel.paste_classifier_enabled', local === '1' ? 'true' : 'false')
    } else {
      pasteClassifierEnabled.value = paste.current_value === 'true'
    }
  }
}

// ============================================================================
// Module-level internal state
// ============================================================================

let snapshotTimer: ReturnType<typeof setTimeout> | null = null
let snapshotWindowStartMs = 0
let currentElementId = ''
let currentElementType = ''

// Signal accumulators (reset per snapshot)
let keystrokeBuffer: { key: string; dwellMs: number; flightMs: number }[] = []
let mouseBuffer: { x: number; y: number; t: number; type: 'move' | 'click' }[] = []
let tabSwitchCount = 0
let totalUnfocusedMs = 0
let lastBlurTime = 0
let pasteEventCount = 0
let pastedCharCount = 0
let devtoolsDetected = false
let environmentChanged = false
let lastKeystrokeTime = 0

// Face detection state
let facePresent: boolean | undefined
let faceCount: number | undefined
let faceConsistency: number | undefined
let faceSimilarity: number | undefined
let faceMatch: boolean | undefined

// Continuous face-absence tracking
let consecutiveNoFaceChecks = 0
let totalFaceChecks = 0
let faceAbsentChecks = 0

// Behavioral profile
let profile: BehavioralProfile | null = null

// Face embedder stays a TS class — pure LBP pixel math, no ML
// framework required, never federated.
let faceEmbedder: FaceEmbedder | null = null

// Per-user model status mirrors what the backend reports via
// `sentinel_user_models_status`. Refreshed on session start so the
// snapshot path can decide whether to bother invoking the scorer.
const keystrokeAeStatus = ref<UserModelStatus | null>(null)
const mouseCnnStatus = ref<UserModelStatus | null>(null)

// Cap incoming ONNX weight blobs — defense against a malicious DAO
// envelope pointing at a huge CID. 50 MiB matches MAX_WEIGHTS_BYTES on
// the Rust side (commands/sentinel_priors.rs).
const MAX_DAO_WEIGHTS_BYTES = 50 * 1024 * 1024

// DAO upgrade is process-wide once-only: lifted to module scope so
// repeated `start()` calls in the same process don't re-fetch on every
// session.
let daoUpgradePromise: Promise<void> | null = null
const loadedClassifierInfo = ref<LoadedClassifierInfo>({ source: 'bundled', version: 'bundled-v1' })

export function getLoadedClassifierInfo(): LoadedClassifierInfo {
  return loadedClassifierInfo.value
}

async function upgradePasteClassifierOnce(): Promise<void> {
  if (daoUpgradePromise) return daoUpgradePromise
  daoUpgradePromise = (async () => {
    try {
      // Pull the current backend-reported source so dashboard cards
      // can show "bundled" until / unless the DAO swap succeeds.
      try {
        loadedClassifierInfo.value = await tauriInvoke<LoadedClassifierInfo>(
          'sentinel_paste_classifier_info',
        )
      } catch { /* backend not ready yet */ }

      const active = await tauriInvoke<ActivePasteClassifier | null>(
        'sentinel_get_active_paste_classifier',
      )
      if (!active) return
      const bytes = await tauriInvoke<number[]>('content_resolve_bytes', {
        identifier: active.weights_cid,
      })
      if (bytes.length > MAX_DAO_WEIGHTS_BYTES) {
        console.warn(
          `[sentinel] DAO weights blob ${bytes.length} bytes exceeds ${MAX_DAO_WEIGHTS_BYTES}; staying on bundled`,
        )
        return
      }
      const info = await tauriInvoke<LoadedClassifierInfo>(
        'sentinel_load_dao_classifier',
        { req: { bytes, version: active.version } },
      )
      loadedClassifierInfo.value = info
      console.info(
        `[sentinel] paste classifier upgraded to DAO model ${active.version} (TPR=${active.eval_tpr} FPR=${active.eval_fpr})`,
      )
    } catch (err) {
      console.warn('[sentinel] DAO classifier upgrade skipped:', err)
    }
  })()
  return daoUpgradePromise
}

// ============================================================================
// Composable
// ============================================================================

export function useSentinel() {
  const { invoke } = useLocalApi()
  const { stakeAddress } = useAuth()

  // =========================================================================
  // Device fingerprint
  // =========================================================================

  const computeDeviceFingerprint = async (): Promise<string> => {
    const components: string[] = []

    try {
      const canvas = document.createElement('canvas')
      canvas.width = 200
      canvas.height = 50
      const ctx = canvas.getContext('2d')
      if (ctx) {
        ctx.textBaseline = 'top'
        ctx.font = '14px Arial'
        ctx.fillStyle = '#f60'
        ctx.fillRect(125, 1, 62, 20)
        ctx.fillStyle = '#069'
        ctx.fillText('Alexandria Sentinel', 2, 15)
        components.push(canvas.toDataURL())
      }
    } catch { /* canvas not available */ }

    try {
      const canvas = document.createElement('canvas')
      const gl = canvas.getContext('webgl')
      if (gl) {
        const debugInfo = gl.getExtension('WEBGL_debug_renderer_info')
        if (debugInfo) {
          components.push(gl.getParameter(debugInfo.UNMASKED_RENDERER_WEBGL))
        }
      }
    } catch { /* WebGL not available */ }

    components.push(`${screen.width}x${screen.height}x${screen.colorDepth}`)
    components.push(Intl.DateTimeFormat().resolvedOptions().timeZone)
    components.push(navigator.language)
    components.push(String(navigator.hardwareConcurrency || 0))

    const data = components.join('|')
    const encoder = new TextEncoder()
    const hashBuffer = await crypto.subtle.digest('SHA-256', encoder.encode(data))
    const hashArray = Array.from(new Uint8Array(hashBuffer))
    return hashArray.map(b => b.toString(16).padStart(2, '0')).join('')
  }

  // =========================================================================
  // Profile management (localStorage)
  // =========================================================================

  const loadProfile = async (userId: string, deviceFp: string): Promise<BehavioralProfile | null> => {
    try {
      const key = `sentinel_profile_${userId}_${deviceFp.substring(0, 16)}`
      const stored = localStorage.getItem(key)
      if (stored) {
        const p: BehavioralProfile = JSON.parse(stored)
        loadAIModels(p)
        return p
      }
    } catch { /* localStorage not available */ }
    return null
  }

  const saveProfile = (userId: string, deviceFp: string, p: BehavioralProfile) => {
    try {
      persistAIModels(p)
      const key = `sentinel_profile_${userId}_${deviceFp.substring(0, 16)}`
      localStorage.setItem(key, JSON.stringify(p))
    } catch { /* localStorage not available */ }
  }

  const loadAIModels = (p: BehavioralProfile) => {
    // Keystroke AE + mouse CNN weights now persist in the backend
    // `sentinel_user_models` table (encrypted SQLite). Only the face
    // enrollment remains client-side because the face embedder is
    // pure pixel math with no ML framework — see docs/sentinel.md.
    if (!p.aiModels) return
    if (p.aiModels.faceEnrollment) {
      try { faceEmbedder = new FaceEmbedder(p.aiModels.faceEnrollment as EnrollmentEmbedding) }
      catch { faceEmbedder = null }
    }
  }

  const persistAIModels = (p: BehavioralProfile) => {
    if (!p.aiModels) p.aiModels = {}
    if (faceEmbedder?.isEnrolled) p.aiModels.faceEnrollment = faceEmbedder.exportEnrollment() as EnrollmentEmbedding
    // Backend-stored model status is fetched on demand via
    // `sentinel_user_models_status`. Don't shadow it in localStorage.
    delete p.aiModels.keystrokeAutoencoder
    delete p.aiModels.mouseCNN
  }

  /**
   * Pull current per-user model status from the backend. Updates the
   * module-scoped refs that gate AI scoring in `computeScores()`.
   */
  const refreshUserModelsStatus = async () => {
    const userId = stakeAddress.value
    if (!userId) return
    const deviceFp = await computeDeviceFingerprint()
    try {
      const rows = await tauriInvoke<UserModelStatus[]>(
        'sentinel_user_models_status',
        { userAddress: userId, deviceFpPrefix: deviceFp.substring(0, 16) },
      )
      keystrokeAeStatus.value =
        rows.find(r => r.model_kind === 'keystroke_ae') ?? null
      mouseCnnStatus.value =
        rows.find(r => r.model_kind === 'mouse_cnn') ?? null
    } catch (err) {
      console.warn('[sentinel] user model status fetch failed:', err)
    }
  }

  // =========================================================================
  // Signal analyzers
  // =========================================================================

  const analyzeKeystrokes = (): { consistency: number; speedWpm: number } => {
    if (keystrokeBuffer.length < 5) return { consistency: 0.5, speedWpm: 0 }

    const dwellTimes = keystrokeBuffer.map(k => k.dwellMs)
    const flightTimes = keystrokeBuffer.filter(k => k.flightMs > 0).map(k => k.flightMs)

    const avgDwell = dwellTimes.reduce((a, b) => a + b, 0) / dwellTimes.length
    const avgFlight = flightTimes.length > 0
      ? flightTimes.reduce((a, b) => a + b, 0) / flightTimes.length
      : 100

    const lastKeystroke = keystrokeBuffer[keystrokeBuffer.length - 1]!
    const totalTime = lastKeystroke.flightMs > 0
      ? keystrokeBuffer.reduce((sum, k) => sum + k.flightMs + k.dwellMs, 0)
      : keystrokeBuffer.length * (avgDwell + avgFlight)
    const minutes = totalTime / 60000
    const words = keystrokeBuffer.length / 5
    const speedWpm = minutes > 0 ? Math.round(words / minutes) : 0

    let consistency = 0.7
    if (profile && profile.typingPattern.sampleCount > 10) {
      const dwellDiff = Math.abs(avgDwell - profile.typingPattern.avgDwellTime)
      const flightDiff = Math.abs(avgFlight - profile.typingPattern.avgFlightTime)
      const speedDiff = Math.abs(speedWpm - profile.typingPattern.speedWpm)
      const dwellScore = Math.max(0, 1 - dwellDiff / 100)
      const flightScore = Math.max(0, 1 - flightDiff / 200)
      const speedScore = Math.max(0, 1 - speedDiff / 50)
      consistency = (dwellScore * 0.3 + flightScore * 0.3 + speedScore * 0.4)
    }

    return { consistency: Math.min(1, Math.max(0, consistency)), speedWpm }
  }

  const analyzeMouse = (): { consistency: number; isHuman: boolean } => {
    if (mouseBuffer.length < 10) return { consistency: 0.5, isHuman: true }

    const moves = mouseBuffer.filter(m => m.type === 'move')
    const velocities: number[] = []
    for (let i = 1; i < moves.length; i++) {
      const curr = moves[i]!
      const prev = moves[i - 1]!
      const dx = curr.x - prev.x
      const dy = curr.y - prev.y
      const dt = curr.t - prev.t
      if (dt > 0) velocities.push(Math.sqrt(dx * dx + dy * dy) / dt)
    }
    if (velocities.length === 0) return { consistency: 0.5, isHuman: true }

    const avgVelocity = velocities.reduce((a, b) => a + b, 0) / velocities.length
    const velocityVariance = velocities.reduce((sum, v) => sum + (v - avgVelocity) ** 2, 0) / velocities.length
    const varianceScore = velocityVariance > 0.001 && velocityVariance < 100 ? 0.9 : 0.3
    const isHuman = velocityVariance > 0.0001 && avgVelocity < 50

    let consistency = 0.7
    if (profile && profile.mousePattern.sampleCount > 10) {
      const velDiff = Math.abs(avgVelocity - profile.mousePattern.avgVelocity)
      consistency = Math.max(0, 1 - velDiff / 10) * 0.6 + varianceScore * 0.4
    } else {
      consistency = varianceScore
    }

    return { consistency: Math.min(1, Math.max(0, consistency)), isHuman }
  }

  const computeScores = (opts?: {
    aiPasteAnomaly?: number
    aiKeystrokeAnomaly?: number
    aiMouseHumanProb?: number
  }): { signals: SignalData; integrity: number; consistency: number; anomalies: string[] } => {
    const { consistency: typingConsistency, speedWpm } = analyzeKeystrokes()
    const { consistency: mouseConsistency, isHuman } = analyzeMouse()
    const anomalies: string[] = []

    const signals: SignalData = {
      typing_consistency: typingConsistency,
      typing_speed_wpm: speedWpm,
      mouse_consistency: mouseConsistency,
      is_human_likely: isHuman,
      tab_switches: tabSwitchCount,
      unfocused_ms: totalUnfocusedMs,
      devtools_detected: devtoolsDetected,
      paste_events: pasteEventCount,
      pasted_chars: pastedCharCount,
      environment_changed: environmentChanged,
    }

    if (cameraOptedIn.value && facePresent !== undefined) {
      signals.face_present = facePresent
      signals.face_count = faceCount
      signals.face_consistency = faceConsistency
    }

    // AI scoring — all three signals are pre-computed by async IPC
    // calls in `scheduleNextSnapshot` (the only consumer of this
    // function that has access to the keystroke/mouse buffers). Other
    // consumers (debug state, stop()) call with `opts === undefined`
    // and simply skip the AI advisory terms.
    if (opts?.aiKeystrokeAnomaly !== undefined && opts.aiKeystrokeAnomaly >= 0) {
      signals.ai_keystroke_anomaly = Math.round(opts.aiKeystrokeAnomaly * 1000) / 1000
      if (signals.ai_keystroke_anomaly >= KEYSTROKE_ANOMALY_THRESHOLD) {
        anomalies.push('behavior_shift')
      }
    }
    if (opts?.aiMouseHumanProb !== undefined && opts.aiMouseHumanProb >= 0) {
      signals.ai_mouse_human_prob = Math.round(opts.aiMouseHumanProb * 1000) / 1000
      if (signals.ai_mouse_human_prob < MOUSE_HUMAN_THRESHOLD) {
        anomalies.push('bot_suspected')
      }
    }

    if (cameraOptedIn.value && faceSimilarity !== undefined) {
      signals.ai_face_similarity = Math.round(faceSimilarity * 1000) / 1000
      signals.ai_face_match = faceMatch ?? false
      if (faceMatch === false && facePresent) anomalies.push('face_mismatch')
    }

    if (opts?.aiPasteAnomaly !== undefined && opts.aiPasteAnomaly >= 0) {
      signals.ai_paste_anomaly = Math.round(opts.aiPasteAnomaly * 1000) / 1000
      if (isCriticalPasteScore(opts.aiPasteAnomaly)) {
        anomalies.push('paste_classifier_critical')
      } else if (isAnomalousPasteScore(opts.aiPasteAnomaly)) {
        anomalies.push('paste_classifier_anomaly')
      }
    }

    // Rule-based integrity score
    let integrity = 0
    let weights = 0

    integrity += typingConsistency * 0.20; weights += 0.20
    integrity += mouseConsistency * 0.15; weights += 0.15
    integrity += (isHuman ? 1 : 0) * 0.15; weights += 0.15

    const tabScore = Math.max(0, 1 - tabSwitchCount / 15)
    integrity += tabScore * 0.15; weights += 0.15

    const pasteScore = Math.max(0, 1 - pastedCharCount / 1000)
    integrity += pasteScore * 0.10; weights += 0.10

    integrity += (devtoolsDetected ? 0 : 1) * 0.10; weights += 0.10

    if (cameraOptedIn.value && facePresent !== undefined) {
      const faceScore = facePresent && faceCount === 1 ? (faceConsistency ?? 0.8) : 0.2
      integrity += faceScore * 0.15; weights += 0.15
    }

    // Advisory AI contributions — opt-in, small weights. Each feeds through
    // as "confidence this is a legit user" so the math stays consistent
    // with the rule-based terms above.
    if (aiScoringEnabled.value) {
      if (signals.ai_keystroke_anomaly !== undefined) {
        integrity += (1 - signals.ai_keystroke_anomaly) * AI_ADVISORY_WEIGHT
        weights += AI_ADVISORY_WEIGHT
      }
      if (signals.ai_mouse_human_prob !== undefined) {
        integrity += signals.ai_mouse_human_prob * AI_ADVISORY_WEIGHT
        weights += AI_ADVISORY_WEIGHT
      }
      if (cameraOptedIn.value && signals.ai_face_similarity !== undefined) {
        integrity += signals.ai_face_similarity * AI_ADVISORY_WEIGHT
        weights += AI_ADVISORY_WEIGHT
      }
      if (signals.ai_paste_anomaly !== undefined && pasteClassifierEnabled.value) {
        integrity += (1 - signals.ai_paste_anomaly) * AI_ADVISORY_WEIGHT
        weights += AI_ADVISORY_WEIGHT
      }
    }

    integrity = weights > 0 ? integrity / weights : 0.5

    let consistencyVal = (typingConsistency + mouseConsistency) / 2
    if (cameraOptedIn.value && faceConsistency !== undefined) {
      consistencyVal = (typingConsistency + mouseConsistency + faceConsistency) / 3
    }

    // Rule-based anomaly flags (see docs/sentinel.md §Flagging Logic)
    const boundedIntegrity = Math.min(1, Math.max(0, integrity))
    const boundedConsistency = Math.min(1, Math.max(0, consistencyVal))
    if (boundedIntegrity < 0.40) anomalies.push('low_integrity')
    if (boundedConsistency < 0.35) anomalies.push('behavior_shift')
    if (tabSwitchCount > 10) anomalies.push('tab_switching')
    if (pastedCharCount > 500) anomalies.push('paste_detected')
    if (devtoolsDetected) anomalies.push('devtools_detected')
    if (!isHuman) anomalies.push('bot_suspected')
    if (cameraOptedIn.value && facePresent === false) anomalies.push('no_face')
    if (cameraOptedIn.value && faceCount !== undefined && faceCount > 1) anomalies.push('multiple_faces')
    if (cameraOptedIn.value && consecutiveNoFaceChecks >= 5) anomalies.push('prolonged_absence')
    if (cameraOptedIn.value && totalFaceChecks > 0 && faceAbsentChecks / totalFaceChecks > 0.5) anomalies.push('frequent_absence')

    return {
      signals,
      integrity: boundedIntegrity,
      consistency: boundedConsistency,
      anomalies: [...new Set(anomalies)],
    }
  }

  // =========================================================================
  // Profile update (EMA)
  // =========================================================================

  const updateProfile = (deviceFp: string) => {
    const userId = stakeAddress.value
    if (!userId) return

    const { speedWpm } = analyzeKeystrokes()
    const moves = mouseBuffer.filter(m => m.type === 'move')
    const alpha = 0.2

    if (!profile) {
      profile = {
        userId,
        deviceFingerprint: deviceFp,
        typingPattern: { avgDwellTime: 80, avgFlightTime: 120, speedWpm: speedWpm || 60, sampleCount: 0 },
        mousePattern: { avgVelocity: 2, avgAcceleration: 0.5, clickPrecision: 0.9, sampleCount: 0 },
        lastUpdated: Date.now(),
      }
    }

    if (keystrokeBuffer.length >= 5) {
      const dwellTimes = keystrokeBuffer.map(k => k.dwellMs)
      const flightTimes = keystrokeBuffer.filter(k => k.flightMs > 0).map(k => k.flightMs)
      const avgDwell = dwellTimes.reduce((a, b) => a + b, 0) / dwellTimes.length
      const avgFlight = flightTimes.length > 0 ? flightTimes.reduce((a, b) => a + b, 0) / flightTimes.length : profile.typingPattern.avgFlightTime
      profile.typingPattern.avgDwellTime = profile.typingPattern.avgDwellTime * (1 - alpha) + avgDwell * alpha
      profile.typingPattern.avgFlightTime = profile.typingPattern.avgFlightTime * (1 - alpha) + avgFlight * alpha
      profile.typingPattern.speedWpm = profile.typingPattern.speedWpm * (1 - alpha) + speedWpm * alpha
      profile.typingPattern.sampleCount++
    }

    if (moves.length >= 10) {
      const velocities: number[] = []
      for (let i = 1; i < moves.length; i++) {
        const curr = moves[i]!
        const prev = moves[i - 1]!
        const dx = curr.x - prev.x
        const dy = curr.y - prev.y
        const dt = curr.t - prev.t
        if (dt > 0) velocities.push(Math.sqrt(dx * dx + dy * dy) / dt)
      }
      if (velocities.length > 0) {
        const avgV = velocities.reduce((a, b) => a + b, 0) / velocities.length
        profile.mousePattern.avgVelocity = profile.mousePattern.avgVelocity * (1 - alpha) + avgV * alpha
        profile.mousePattern.sampleCount++
      }
    }

    profile.lastUpdated = Date.now()
    saveProfile(profile.userId, deviceFp, profile)
  }

  // =========================================================================
  // Snapshot scheduling (random interval 15-45s)
  // =========================================================================

  const scheduleNextSnapshot = () => {
    if (!isActive.value || !sessionId.value) return

    const delay = 15000 + Math.random() * 30000
    if (snapshotWindowStartMs === 0) snapshotWindowStartMs = Date.now()

    snapshotTimer = setTimeout(async () => {
      if (!isActive.value || !sessionId.value) return

      // Run the ONNX paste classifier before the (sync) score path so the
      // signal is folded into the weighted integrity calculation rather
      // than tacked on after. A score of -1 means the model artifact
      // wasn't available; the signal is then simply absent, mirroring
      // the convention used by the other AI signals.
      const windowMs = snapshotWindowStartMs > 0
        ? Math.max(1, Date.now() - snapshotWindowStartMs)
        : 30_000

      // All three AI scores come from the backend now (tract for the
      // paste classifier, candle for the per-user keystroke AE + mouse
      // CNN). The frontend just shovels raw events across IPC. A
      // returned score of -1 means "model not yet trained / not
      // available" — handled the same way as before: signal absent.
      const userId = stakeAddress.value
      const deviceFp = userId ? (await computeDeviceFingerprint()).substring(0, 16) : ''

      let pasteAnomaly = -1
      try {
        const resp = await tauriInvoke<ScorePasteResponse>('sentinel_score_paste', {
          req: {
            events: keystrokeBuffer.map(k => ({ key: k.key, dwellMs: k.dwellMs, flightMs: k.flightMs })),
            paste_event_count: pasteEventCount,
            pasted_char_count: pastedCharCount,
            window_ms: windowMs,
          },
        })
        pasteAnomaly = resp.score
        loadedClassifierInfo.value = resp.classifier
      } catch (err) {
        console.warn('[sentinel] paste score IPC failed', err)
      }

      let keystrokeAnomaly = -1
      if (userId && keystrokeAeStatus.value && keystrokeAeStatus.value.trained_epochs > 0 && keystrokeBuffer.length >= 5) {
        try {
          keystrokeAnomaly = await tauriInvoke<number>('sentinel_score_keystroke_ae', {
            req: {
              user_address: userId,
              device_fp_prefix: deviceFp,
              events: keystrokeBuffer.map(k => ({ key: k.key, dwellMs: k.dwellMs, flightMs: k.flightMs })),
            },
          })
        } catch (err) {
          console.warn('[sentinel] keystroke AE score IPC failed', err)
        }
      }

      let mouseHumanProb = -1
      const movePoints = mouseBuffer
        .filter(m => m.type === 'move')
        .map(m => ({ x: m.x, y: m.y, t: m.t }))
      if (userId && mouseCnnStatus.value && mouseCnnStatus.value.trained_epochs > 0 && movePoints.length >= 51) {
        try {
          mouseHumanProb = await tauriInvoke<number>('sentinel_score_mouse_cnn', {
            req: {
              user_address: userId,
              device_fp_prefix: deviceFp,
              points: movePoints,
            },
          })
        } catch (err) {
          console.warn('[sentinel] mouse CNN score IPC failed', err)
        }
      }

      const { signals, integrity, consistency, anomalies } = computeScores({
        aiPasteAnomaly: pasteAnomaly,
        aiKeystrokeAnomaly: keystrokeAnomaly,
        aiMouseHumanProb: mouseHumanProb,
      })
      const deduped = [...new Set(anomalies)]

      integrityScore.value = integrity
      consistencyScore.value = consistency

      try {
        await invoke('integrity_submit_snapshot', {
          req: {
            session_id: sessionId.value,
            element_id: currentElementId,
            integrity_score: integrity,
            consistency_score: consistency,
            typing_score: signals.typing_consistency,
            mouse_score: signals.mouse_consistency,
            human_score: signals.is_human_likely ? 1.0 : 0.0,
            tab_score: Math.max(0, 1 - signals.tab_switches / 15),
            paste_score: Math.max(0, 1 - signals.pasted_chars / 1000),
            devtools_score: signals.devtools_detected ? 0.0 : 1.0,
            camera_score: signals.face_consistency ?? null,
            ai_paste_anomaly: signals.ai_paste_anomaly ?? null,
            anomaly_flags: deduped,
          },
        })
      } catch { /* best effort */ }

      // Reset per-snapshot accumulators
      keystrokeBuffer = []
      mouseBuffer = []
      tabSwitchCount = 0
      totalUnfocusedMs = 0
      pasteEventCount = 0
      pastedCharCount = 0
      devtoolsDetected = false
      environmentChanged = false
      totalFaceChecks = 0
      faceAbsentChecks = 0
      snapshotWindowStartMs = Date.now()

      scheduleNextSnapshot()
    }, delay)
  }

  // =========================================================================
  // Event listeners
  // =========================================================================

  const onKeyDown = (e: KeyboardEvent) => {
    if (!isActive.value) return
    const now = performance.now()
    const flightMs = lastKeystrokeTime > 0 ? now - lastKeystrokeTime : 0
    keystrokeBuffer.push({
      key: e.key.length === 1 ? 'char' : e.key,
      dwellMs: 0,
      flightMs,
    })
    lastKeystrokeTime = now
  }

  const onKeyUp = (_e: KeyboardEvent) => {
    if (!isActive.value) return
    const now = performance.now()
    if (keystrokeBuffer.length > 0) {
      const last = keystrokeBuffer[keystrokeBuffer.length - 1]!
      last.dwellMs = now - (lastKeystrokeTime - last.flightMs)
    }
  }

  const onMouseMove = (e: MouseEvent) => {
    if (!isActive.value) return
    const now = performance.now()
    if (mouseBuffer.length > 0 && now - mouseBuffer[mouseBuffer.length - 1]!.t < 50) return
    mouseBuffer.push({ x: e.clientX, y: e.clientY, t: now, type: 'move' })
    if (mouseBuffer.length > 200) mouseBuffer = mouseBuffer.slice(-100)
  }

  const onMouseClick = (e: MouseEvent) => {
    if (!isActive.value) return
    mouseBuffer.push({ x: e.clientX, y: e.clientY, t: performance.now(), type: 'click' })
  }

  const onVisibilityChange = () => {
    if (!isActive.value) return
    if (document.hidden) {
      lastBlurTime = Date.now()
      tabSwitchCount++
    } else if (lastBlurTime > 0) {
      totalUnfocusedMs += Date.now() - lastBlurTime
      lastBlurTime = 0
    }
  }

  const onPaste = (e: ClipboardEvent) => {
    if (!isActive.value) return
    pasteEventCount++
    const text = e.clipboardData?.getData('text') || ''
    pastedCharCount += text.length
  }

  const onDevToolsCheck = () => {
    const widthThreshold = window.outerWidth - window.innerWidth > 160
    const heightThreshold = window.outerHeight - window.innerHeight > 160
    if (widthThreshold || heightThreshold) devtoolsDetected = true
  }

  // =========================================================================
  // Public API
  // =========================================================================

  const start = async (enrollmentId: string, optInCamera = false) => {
    if (isActive.value) return

    cameraOptedIn.value = optInCamera

    const deviceFp = await computeDeviceFingerprint()
    const userId = stakeAddress.value
    if (userId) {
      profile = await loadProfile(userId, deviceFp)
    }

    try {
      const response = await invoke<StartSessionResponse>('integrity_start_session', { enrollmentId })
      sessionId.value = response.session_id
      isActive.value = true
      snapshotWindowStartMs = Date.now()

      // Best-effort upgrade to the latest DAO-ratified paste classifier.
      // Failures fall through to the bundled artifact — never block
      // session start on a model swap.
      void tryUpgradePasteClassifier()

      // Refresh per-user model status so the snapshot path knows whether
      // to call the AE / CNN scoring IPCs.
      void refreshUserModelsStatus()

      document.addEventListener('keydown', onKeyDown, { passive: true })
      document.addEventListener('keyup', onKeyUp, { passive: true })
      document.addEventListener('mousemove', onMouseMove, { passive: true })
      document.addEventListener('click', onMouseClick, { passive: true })
      document.addEventListener('visibilitychange', onVisibilityChange)
      document.addEventListener('paste', onPaste)
      window.addEventListener('resize', onDevToolsCheck)

      scheduleNextSnapshot()
    } catch (e) {
      console.warn('Sentinel: failed to start session', e)
    }
  }

  const tryUpgradePasteClassifier = () => upgradePasteClassifierOnce()

  const setElement = (elementId: string, elementType: string) => {
    currentElementId = elementId
    currentElementType = elementType
  }

  const isAssessmentElement = (elementType: string): boolean => {
    return ['quiz', 'assessment', 'interactive'].includes(elementType)
  }

  const reportFaceDetection = (present: boolean, count: number, consistency: number, similarity?: number, match?: boolean) => {
    facePresent = present
    faceCount = count
    faceConsistency = consistency
    if (similarity !== undefined) faceSimilarity = similarity
    if (match !== undefined) faceMatch = match

    totalFaceChecks++
    if (!present) { faceAbsentChecks++; consecutiveNoFaceChecks++ }
    else { consecutiveNoFaceChecks = 0 }
  }

  const verifyFace = (video: HTMLVideoElement): {
    present: boolean; count: number; consistency: number
    similarity?: number; match?: boolean
  } => {
    if (!video || video.readyState < 2) return { present: false, count: 0, consistency: 0.2 }

    if (faceEmbedder?.isEnrolled) {
      const result = faceEmbedder.verify(video)
      if (result) {
        const present = result.faceDetected
        const count = result.faceCount
        const consistency = present ? Math.max(0.2, result.similarity) : 0.2
        reportFaceDetection(present, count, consistency, result.similarity, result.isMatch)
        return { present, count, consistency, similarity: result.similarity, match: result.isMatch }
      }
    }

    if (faceEmbedder) {
      const embedding = faceEmbedder.embed(video)
      if (embedding) {
        reportFaceDetection(embedding.faceDetected, embedding.faceCount, embedding.faceDetected ? 0.8 : 0.2)
        return { present: embedding.faceDetected, count: embedding.faceCount, consistency: embedding.faceDetected ? 0.8 : 0.2 }
      }
    }

    reportFaceDetection(false, 0, 0.2)
    return { present: false, count: 0, consistency: 0.2 }
  }

  const stop = async () => {
    if (!isActive.value || !sessionId.value) return

    isActive.value = false

    if (snapshotTimer) { clearTimeout(snapshotTimer); snapshotTimer = null }

    document.removeEventListener('keydown', onKeyDown)
    document.removeEventListener('keyup', onKeyUp)
    document.removeEventListener('mousemove', onMouseMove)
    document.removeEventListener('click', onMouseClick)
    document.removeEventListener('visibilitychange', onVisibilityChange)
    document.removeEventListener('paste', onPaste)
    window.removeEventListener('resize', onDevToolsCheck)

    const { integrity, consistency } = computeScores()
    integrityScore.value = integrity
    consistencyScore.value = consistency

    const deviceFp = await computeDeviceFingerprint()
    updateProfile(deviceFp)

    try {
      await invoke('integrity_end_session', {
        sessionId: sessionId.value,
        req: {
          overall_integrity_score: integrity,
          overall_consistency_score: consistency,
        },
      })
    } catch {
      console.warn('Sentinel: failed to end session')
    }

    const currentSessionId = sessionId.value
    sessionId.value = null
    keystrokeBuffer = []
    mouseBuffer = []
    tabSwitchCount = 0
    totalUnfocusedMs = 0
    pasteEventCount = 0
    pastedCharCount = 0
    devtoolsDetected = false
    environmentChanged = false
    lastKeystrokeTime = 0
    snapshotWindowStartMs = 0
    facePresent = undefined
    faceCount = undefined
    faceConsistency = undefined
    faceSimilarity = undefined
    faceMatch = undefined
    consecutiveNoFaceChecks = 0
    totalFaceChecks = 0
    faceAbsentChecks = 0

    return currentSessionId
  }

  /** Returns the final integrity score for evidence attachment */
  const getFinalScore = (): number => integrityScore.value

  /** Returns the session ID for linking to evidence records */
  const getSessionId = (): string | null => sessionId.value

  const getDebugState = () => ({
    currentElementId,
    currentElementType,
    keystrokeBufferSize: keystrokeBuffer.length,
    mouseBufferSize: mouseBuffer.length,
    tabSwitchCount,
    totalUnfocusedMs,
    pasteEventCount,
    pastedCharCount,
    devtoolsDetected,
    facePresent,
    faceCount,
    faceConsistency,
    faceSimilarity,
    faceMatch,
    consecutiveNoFaceChecks,
    profile: profile ? { ...profile } : null,
    hasSnapshotTimer: snapshotTimer !== null,
    aiModels: {
      keystrokeAE: keystrokeAeStatus.value,
      mouseCNN: mouseCnnStatus.value,
      faceEmbedder: faceEmbedder
        ? { enrolled: faceEmbedder.isEnrolled, progress: faceEmbedder.enrollmentProgress }
        : null,
    },
    ...(keystrokeBuffer.length >= 5 || mouseBuffer.length >= 10
      ? computeScores()
      : { signals: null, integrity: null, consistency: null, anomalies: [] }),
  })

  // =========================================================================
  // Training API (calibration wizard)
  // =========================================================================

  const startTrainingKeystrokes = () => {
    keystrokeBuffer = []
    lastKeystrokeTime = 0
    document.addEventListener('keydown', onKeyDown, { passive: true })
    document.addEventListener('keyup', onKeyUp, { passive: true })
    return () => {
      document.removeEventListener('keydown', onKeyDown)
      document.removeEventListener('keyup', onKeyUp)
    }
  }

  const startTrainingMouse = () => {
    mouseBuffer = []
    document.addEventListener('mousemove', onMouseMove, { passive: true })
    document.addEventListener('click', onMouseClick, { passive: true })
    return () => {
      document.removeEventListener('mousemove', onMouseMove)
      document.removeEventListener('click', onMouseClick)
    }
  }

  const getTrainingMetrics = () => {
    const typing = analyzeKeystrokes()
    const mouse = analyzeMouse()
    const dwellTimes = keystrokeBuffer.map(k => k.dwellMs).filter(d => d > 0)
    const flightTimes = keystrokeBuffer.filter(k => k.flightMs > 0).map(k => k.flightMs)

    return {
      keystrokeCount: keystrokeBuffer.length,
      mouseEventCount: mouseBuffer.length,
      mouseMoveCount: mouseBuffer.filter(m => m.type === 'move').length,
      mouseClickCount: mouseBuffer.filter(m => m.type === 'click').length,
      typing: {
        consistency: typing.consistency,
        speedWpm: typing.speedWpm,
        avgDwellMs: dwellTimes.length > 0 ? dwellTimes.reduce((a, b) => a + b, 0) / dwellTimes.length : 0,
        avgFlightMs: flightTimes.length > 0 ? flightTimes.reduce((a, b) => a + b, 0) / flightTimes.length : 0,
      },
      mouse: { consistency: mouse.consistency, isHuman: mouse.isHuman },
    }
  }

  const clearTrainingBuffers = () => {
    keystrokeBuffer = []
    mouseBuffer = []
    lastKeystrokeTime = 0
  }

  const getProfile = () => profile ? { ...profile } as BehavioralProfile : null

  const saveTrainingProfile = async () => {
    const userId = stakeAddress.value
    if (!userId) return
    const deviceFp = await computeDeviceFingerprint()
    const { speedWpm } = analyzeKeystrokes()
    const moves = mouseBuffer.filter(m => m.type === 'move')
    const alpha = profile && profile.typingPattern.sampleCount > 0 ? 0.5 : 1.0

    if (!profile) {
      profile = {
        userId,
        deviceFingerprint: deviceFp,
        typingPattern: { avgDwellTime: 80, avgFlightTime: 120, speedWpm: speedWpm || 60, sampleCount: 0 },
        mousePattern: { avgVelocity: 2, avgAcceleration: 0.5, clickPrecision: 0.9, sampleCount: 0 },
        lastUpdated: Date.now(),
      }
    }

    if (keystrokeBuffer.length >= 5) {
      const dwellTimes = keystrokeBuffer.map(k => k.dwellMs).filter(d => d > 0)
      const flightTimes = keystrokeBuffer.filter(k => k.flightMs > 0).map(k => k.flightMs)
      if (dwellTimes.length > 0) {
        const avgDwell = dwellTimes.reduce((a, b) => a + b, 0) / dwellTimes.length
        const avgFlight = flightTimes.length > 0 ? flightTimes.reduce((a, b) => a + b, 0) / flightTimes.length : 120
        profile.typingPattern.avgDwellTime = profile.typingPattern.avgDwellTime * (1 - alpha) + avgDwell * alpha
        profile.typingPattern.avgFlightTime = profile.typingPattern.avgFlightTime * (1 - alpha) + avgFlight * alpha
        profile.typingPattern.speedWpm = profile.typingPattern.speedWpm * (1 - alpha) + (speedWpm || 60) * alpha
        profile.typingPattern.sampleCount++
      }
    }

    if (moves.length >= 10) {
      const velocities: number[] = []
      for (let i = 1; i < moves.length; i++) {
        const curr = moves[i]!
        const prev = moves[i - 1]!
        const dx = curr.x - prev.x
        const dy = curr.y - prev.y
        const dt = curr.t - prev.t
        if (dt > 0) velocities.push(Math.sqrt(dx * dx + dy * dy) / dt)
      }
      if (velocities.length > 0) {
        const avgV = velocities.reduce((a, b) => a + b, 0) / velocities.length
        profile.mousePattern.avgVelocity = profile.mousePattern.avgVelocity * (1 - alpha) + avgV * alpha
        profile.mousePattern.sampleCount++
      }
    }

    // Per-user AI training runs in the Rust backend now (candle).
    // Fire-and-forget IPCs — failures are logged and don't block the
    // profile save. Status refs refresh on success.
    const fpPrefix = deviceFp.substring(0, 16)
    if (keystrokeBuffer.length >= 20) {
      try {
        const r = await tauriInvoke<TrainKeystrokeAeResponse>('sentinel_train_keystroke_ae', {
          req: {
            user_address: userId,
            device_fp_prefix: fpPrefix,
            events: keystrokeBuffer.map(k => ({ key: k.key, dwellMs: k.dwellMs, flightMs: k.flightMs })),
          },
        })
        keystrokeAeStatus.value = {
          model_kind: 'keystroke_ae',
          trained_epochs: r.trained_epochs,
          training_samples: r.training_samples,
          train_loss: r.train_loss,
          updated_at: new Date().toISOString(),
        }
      } catch (err) {
        console.warn('[sentinel] keystroke AE train IPC failed', err)
      }
    }

    if (moves.length >= 51) {
      try {
        const r = await tauriInvoke<TrainMouseCnnResponse>('sentinel_train_mouse_cnn', {
          req: {
            user_address: userId,
            device_fp_prefix: fpPrefix,
            points: moves.map(m => ({ x: m.x, y: m.y, t: m.t })),
          },
        })
        mouseCnnStatus.value = {
          model_kind: 'mouse_cnn',
          trained_epochs: r.trained_epochs,
          training_samples: r.training_samples,
          train_loss: r.train_loss,
          updated_at: new Date().toISOString(),
        }
      } catch (err) {
        console.warn('[sentinel] mouse CNN train IPC failed', err)
      }
    }

    profile.lastUpdated = Date.now()
    saveProfile(userId, deviceFp, profile)
  }

  /**
   * Score a candidate prior blob against the current local classifier.
   *
   * Used by the propose-prior UX to self-check before submitting: if
   * the classifier already flags the blob as strongly anomalous, the
   * prior is genuinely adversarial and worth proposing. If it scores
   * like a legit human, ratifying it would teach the model to flag
   * honest users — the proposer (and later DAO voters) should reject.
   *
   * Returns null if the local classifier isn't trained yet (no signal
   * to compare against). Returns `meanScore` on [0,1]:
   *   - keystroke: average reconstruction-error anomaly score (higher
   *     = more anomalous; > 0.65 is the ratify signal)
   *   - mouse: 1 - average human probability (higher = more bot-like;
   *     > 0.50 is the ratify signal since the CNN is symmetric)
   *
   * `adversarialFraction` is the share of samples that individually
   * cross the per-model anomaly threshold — useful for picking up
   * priors that are a mix of good/bad examples.
   */
  /**
   * Score a candidate prior blob against the user's current model.
   *
   * `keystroke` mode: builds synthetic event streams that reproduce
   * the blob's digraph timings, scores them through the backend AE,
   * and reports mean anomaly + fraction over the 0.65 threshold.
   *
   * `mouse` mode: feeds raw trajectories to the backend CNN and
   * reports `mean(1 - human_prob)`. Higher = more bot-like.
   *
   * Returns `null` if the user's model isn't trained yet.
   */
  const testBlobAgainstClassifier = async (
    modelKind: 'keystroke' | 'mouse',
    samples: unknown[],
  ): Promise<{ meanScore: number; adversarialFraction: number; sampleCount: number } | null> => {
    const userId = stakeAddress.value
    if (!userId) return null
    const deviceFp = (await computeDeviceFingerprint()).substring(0, 16)

    if (modelKind === 'keystroke') {
      if (!keystrokeAeStatus.value || keystrokeAeStatus.value.trained_epochs === 0) return null
      const digraphs = samples.filter((s): s is DigraphFeatures =>
        typeof s === 'object' && s !== null
        && typeof (s as DigraphFeatures).dwellMs1 === 'number'
        && typeof (s as DigraphFeatures).dwellMs2 === 'number'
        && typeof (s as DigraphFeatures).flightMs === 'number'
        && typeof (s as DigraphFeatures).speedRatio === 'number',
      )
      if (digraphs.length < 5) return null

      const WINDOW = 10
      const scores: number[] = []
      for (let i = 0; i + 5 <= digraphs.length; i += WINDOW) {
        const window = digraphs.slice(i, Math.min(i + WINDOW, digraphs.length))
        if (window.length < 5) continue
        const events = digraphsToEvents(window)
        try {
          const s = await tauriInvoke<number>('sentinel_score_keystroke_ae', {
            req: { user_address: userId, device_fp_prefix: deviceFp, events },
          })
          if (s >= 0) scores.push(s)
        } catch { /* ignore single-window failure */ }
      }
      if (scores.length === 0) return null
      const mean = scores.reduce((a, b) => a + b, 0) / scores.length
      const anomalous = scores.filter(s => s >= 0.65).length
      return { meanScore: mean, adversarialFraction: anomalous / scores.length, sampleCount: digraphs.length }
    }

    // mouse
    if (!mouseCnnStatus.value || mouseCnnStatus.value.trained_epochs === 0) return null
    const trajectories = samples.filter((s): s is { trajectory: MousePoint[] } =>
      typeof s === 'object' && s !== null
      && Array.isArray((s as { trajectory?: unknown }).trajectory),
    )
    if (trajectories.length === 0) return null
    const botScores: number[] = []
    for (const t of trajectories) {
      if (t.trajectory.length < 51) continue
      try {
        const humanProb = await tauriInvoke<number>('sentinel_score_mouse_cnn', {
          req: { user_address: userId, device_fp_prefix: deviceFp, points: t.trajectory },
        })
        if (humanProb >= 0) botScores.push(1 - humanProb)
      } catch { /* ignore single-trajectory failure */ }
    }
    if (botScores.length === 0) return null
    const mean = botScores.reduce((a, b) => a + b, 0) / botScores.length
    const adversarial = botScores.filter(s => s >= 0.5).length
    return { meanScore: mean, adversarialFraction: adversarial / botScores.length, sampleCount: trajectories.length }
  }

  /** Reconstruct minimal keystroke events from digraph features so the
   * backend AE can score them. Mirrors the inverse of the legacy TS
   * `extractDigraphFeatures` so blobs that came from `DigraphFeatures`
   * still produce meaningful AE inputs. */
  function digraphsToEvents(digraphs: DigraphFeatures[]): KeystrokeEvent[] {
    if (digraphs.length === 0) return []
    const out: KeystrokeEvent[] = [
      { key: 'char', dwellMs: digraphs[0]!.dwellMs1, flightMs: 0 },
    ]
    for (const d of digraphs) {
      out.push({ key: 'char', dwellMs: d.dwellMs2, flightMs: d.flightMs })
    }
    return out
  }

  const fetchPriorTrajectories = async (): Promise<MousePoint[][]> => {
    try {
      const priors = await invoke<SentinelPrior[]>('sentinel_priors_list', { modelKind: 'mouse' })
      const blobs = await Promise.all(priors.map(p =>
        invoke<SentinelPriorBlob>('sentinel_priors_load', { priorId: p.id }).catch(() => null),
      ))
      const out: MousePoint[][] = []
      for (const blob of blobs) {
        if (!blob || blob.model_kind !== 'mouse') continue
        for (const entry of blob.samples as Array<{ trajectory?: MousePoint[] }>) {
          if (Array.isArray(entry?.trajectory) && entry.trajectory.length >= 51) {
            out.push(entry.trajectory)
          }
        }
      }
      return out
    } catch {
      return []
    }
  }

  const fetchKeystrokeNegatives = async (): Promise<DigraphFeatures[]> => {
    try {
      const priors = await invoke<SentinelPrior[]>('sentinel_priors_list', { modelKind: 'keystroke' })
      const blobs = await Promise.all(priors.map(p =>
        invoke<SentinelPriorBlob>('sentinel_priors_load', { priorId: p.id }).catch(() => null),
      ))
      const out: DigraphFeatures[] = []
      for (const blob of blobs) {
        if (!blob || blob.model_kind !== 'keystroke') continue
        for (const s of blob.samples) {
          const d = s as Partial<DigraphFeatures>
          if (typeof d.dwellMs1 === 'number' && typeof d.dwellMs2 === 'number'
              && typeof d.flightMs === 'number' && typeof d.speedRatio === 'number') {
            out.push(d as DigraphFeatures)
          }
        }
      }
      return out
    } catch {
      return []
    }
  }

  const trainAIModels = async (): Promise<{
    keystrokeAE: { trained: boolean; loss: number; samples: number; priorDigraphs: number }
    mouseCNN: { trained: boolean; loss: number; samples: number; priorTrajectories: number }
    faceEmbedder: { enrolled: boolean; progress: number }
  }> => {
    const userId = stakeAddress.value
    if (!userId) {
      return {
        keystrokeAE: { trained: false, loss: -1, samples: 0, priorDigraphs: 0 },
        mouseCNN: { trained: false, loss: -1, samples: 0, priorTrajectories: 0 },
        faceEmbedder: {
          enrolled: faceEmbedder?.isEnrolled ?? false,
          progress: faceEmbedder?.enrollmentProgress ?? 0,
        },
      }
    }
    const deviceFp = (await computeDeviceFingerprint()).substring(0, 16)

    let aeLoss = -1
    let aeSamples = 0
    let aeTrained = false
    let priorDigraphs = 0
    if (keystrokeBuffer.length >= 20) {
      // Hydrate ratified keystroke priors (labeled attack digraphs) to
      // drive the AE's contrastive "push-away" pass.
      const ratifiedNegatives = await fetchKeystrokeNegatives()
      priorDigraphs = ratifiedNegatives.length
      try {
        const r = await tauriInvoke<TrainKeystrokeAeResponse>('sentinel_train_keystroke_ae', {
          req: {
            user_address: userId,
            device_fp_prefix: deviceFp,
            events: keystrokeBuffer.map(k => ({ key: k.key, dwellMs: k.dwellMs, flightMs: k.flightMs })),
            negative_digraphs: ratifiedNegatives,
          },
        })
        aeLoss = r.train_loss
        aeSamples = r.training_samples
        aeTrained = r.trained_epochs > 0 && r.training_samples >= 20
        keystrokeAeStatus.value = {
          model_kind: 'keystroke_ae',
          trained_epochs: r.trained_epochs,
          training_samples: r.training_samples,
          train_loss: r.train_loss,
          updated_at: new Date().toISOString(),
        }
      } catch (err) {
        console.warn('[sentinel] keystroke AE training failed', err)
      }
    }

    let cnnLoss = -1
    let cnnSamples = 0
    let cnnTrained = false
    let priorTrajectories = 0
    const moves = mouseBuffer.filter(m => m.type === 'move')
    if (moves.length >= 51) {
      const ratifiedBots = await fetchPriorTrajectories()
      priorTrajectories = ratifiedBots.length
      try {
        const r = await tauriInvoke<TrainMouseCnnResponse>('sentinel_train_mouse_cnn', {
          req: {
            user_address: userId,
            device_fp_prefix: deviceFp,
            points: moves.map(m => ({ x: m.x, y: m.y, t: m.t })),
          },
        })
        cnnLoss = r.train_loss
        cnnSamples = r.training_samples
        cnnTrained = r.trained_epochs > 0 && r.training_samples >= 1
        mouseCnnStatus.value = {
          model_kind: 'mouse_cnn',
          trained_epochs: r.trained_epochs,
          training_samples: r.training_samples,
          train_loss: r.train_loss,
          updated_at: new Date().toISOString(),
        }
      } catch (err) {
        console.warn('[sentinel] mouse CNN training failed', err)
      }
    }

    return {
      keystrokeAE: { trained: aeTrained, loss: aeLoss, samples: aeSamples, priorDigraphs },
      mouseCNN: { trained: cnnTrained, loss: cnnLoss, samples: cnnSamples, priorTrajectories },
      faceEmbedder: { enrolled: faceEmbedder?.isEnrolled ?? false, progress: faceEmbedder?.enrollmentProgress ?? 0 },
    }
  }

  const enrollFace = (video: HTMLVideoElement): boolean => {
    if (!faceEmbedder) faceEmbedder = new FaceEmbedder()
    return faceEmbedder.enroll(video)
  }

  const getAIModelStatus = () => ({
    keystrokeAE: keystrokeAeStatus.value
      ? {
          trained: keystrokeAeStatus.value.trained_epochs > 0,
          epochs: keystrokeAeStatus.value.trained_epochs,
          samples: keystrokeAeStatus.value.training_samples,
          loss: keystrokeAeStatus.value.train_loss ?? 0,
        }
      : null,
    mouseCNN: mouseCnnStatus.value
      ? {
          trained: mouseCnnStatus.value.trained_epochs > 0,
          epochs: mouseCnnStatus.value.trained_epochs,
          samples: mouseCnnStatus.value.training_samples,
          loss: mouseCnnStatus.value.train_loss ?? 0,
        }
      : null,
    faceEmbedder: faceEmbedder
      ? { enrolled: faceEmbedder.isEnrolled, progress: faceEmbedder.enrollmentProgress }
      : null,
  })

  const resetProfile = async () => {
    const userId = stakeAddress.value
    if (!userId) return
    const deviceFp = await computeDeviceFingerprint()
    const key = `sentinel_profile_${userId}_${deviceFp.substring(0, 16)}`
    try { localStorage.removeItem(key) } catch { /* ignore */ }
    profile = null
    faceEmbedder = null
    keystrokeAeStatus.value = null
    mouseCnnStatus.value = null
    try {
      await tauriInvoke('sentinel_reset_user_models', {
        userAddress: userId,
        deviceFpPrefix: deviceFp.substring(0, 16),
      })
    } catch (err) {
      console.warn('[sentinel] reset user models IPC failed', err)
    }
  }

  const setAIScoringEnabled = (enabled: boolean) => {
    aiScoringEnabled.value = enabled
    try { localStorage.setItem(AI_SCORING_STORAGE_KEY, enabled ? '1' : '0') }
    catch { /* localStorage not available */ }
    // Persist to per-profile settings (scope=sync) so the toggle
    // propagates to the user's other devices.
    void (async () => {
      const { useSettings } = await import('./useSettings')
      useSettings()
        .setSetting('sentinel.ai_scoring_enabled', enabled ? 'true' : 'false')
        .catch(() => { /* no profile yet */ })
    })()
  }

  const setPasteClassifierEnabled = (enabled: boolean) => {
    pasteClassifierEnabled.value = enabled
    try { localStorage.setItem(PASTE_CLASSIFIER_STORAGE_KEY, enabled ? '1' : '0') }
    catch { /* localStorage not available */ }
    void (async () => {
      const { useSettings } = await import('./useSettings')
      useSettings()
        .setSetting('sentinel.paste_classifier_enabled', enabled ? 'true' : 'false')
        .catch(() => { /* no profile yet */ })
    })()
  }

  /** Toggle camera opt-in mid-session. Caller is responsible for acquiring
   * the MediaStream, attaching an HTMLVideoElement, and driving the 3s
   * face-verification loop (see docs/sentinel.md §Camera). This only flips
   * the flag that gates face-related signals in computeScores(). */
  const setCameraOptedIn = (opted: boolean) => {
    cameraOptedIn.value = opted
    if (!opted) {
      facePresent = undefined
      faceCount = undefined
      faceConsistency = undefined
      faceSimilarity = undefined
      faceMatch = undefined
      consecutiveNoFaceChecks = 0
      faceAbsentChecks = 0
      totalFaceChecks = 0
    }
  }

  return {
    // State
    sessionId: readonly(sessionId),
    isActive: readonly(isActive),
    integrityScore: readonly(integrityScore),
    consistencyScore: readonly(consistencyScore),
    cameraOptedIn: readonly(cameraOptedIn),
    aiScoringEnabled: readonly(aiScoringEnabled),
    pasteClassifierEnabled: readonly(pasteClassifierEnabled),
    keystrokeAeStatus: readonly(keystrokeAeStatus),
    mouseCnnStatus: readonly(mouseCnnStatus),
    loadedClassifierInfo: readonly(loadedClassifierInfo),

    // Session controls
    start,
    stop,
    setElement,
    isAssessmentElement,
    reportFaceDetection,
    verifyFace,
    getDebugState,
    getFinalScore,
    getSessionId,

    // Training API
    startTrainingKeystrokes,
    startTrainingMouse,
    getTrainingMetrics,
    clearTrainingBuffers,
    getProfile,
    saveTrainingProfile,
    resetProfile,

    // AI model training
    trainAIModels,
    enrollFace,
    getAIModelStatus,
    refreshUserModelsStatus,
    setAIScoringEnabled,
    setPasteClassifierEnabled,
    setCameraOptedIn,
    testBlobAgainstClassifier,
  }
}
