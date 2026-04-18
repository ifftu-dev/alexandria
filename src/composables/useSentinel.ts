import { ref, readonly } from 'vue'
import { useLocalApi } from './useLocalApi'
import { useAuth } from './useAuth'
import {
  KeystrokeAutoencoder,
  type AutoencoderWeights,
  type KeystrokeEvent as AEKeystrokeEvent,
} from '@/utils/sentinel/keystroke-autoencoder'
import {
  MouseTrajectoryCNN,
  type MouseCNNWeights,
  type MousePoint,
} from '@/utils/sentinel/mouse-trajectory-cnn'
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
} from '@/types'

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

function readAIScoringPref(): boolean {
  try { return localStorage.getItem(AI_SCORING_STORAGE_KEY) === '1' }
  catch { return false }
}

// ============================================================================
// Module-level internal state
// ============================================================================

let snapshotTimer: ReturnType<typeof setTimeout> | null = null
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

// AI models
let keystrokeAE: KeystrokeAutoencoder | null = null
let mouseCNN: MouseTrajectoryCNN | null = null
let faceEmbedder: FaceEmbedder | null = null

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
    if (!p.aiModels) return

    if (p.aiModels.keystrokeAutoencoder) {
      try { keystrokeAE = new KeystrokeAutoencoder(p.aiModels.keystrokeAutoencoder as unknown as AutoencoderWeights) }
      catch { keystrokeAE = null }
    }
    if (p.aiModels.mouseCNN) {
      try { mouseCNN = new MouseTrajectoryCNN(p.aiModels.mouseCNN as unknown as MouseCNNWeights) }
      catch { mouseCNN = null }
    }
    if (p.aiModels.faceEnrollment) {
      try { faceEmbedder = new FaceEmbedder(p.aiModels.faceEnrollment as EnrollmentEmbedding) }
      catch { faceEmbedder = null }
    }
  }

  const persistAIModels = (p: BehavioralProfile) => {
    if (!p.aiModels) p.aiModels = {}
    if (keystrokeAE?.isTrained) p.aiModels.keystrokeAutoencoder = keystrokeAE.exportWeights() as unknown as Record<string, unknown>
    if (mouseCNN?.isTrained) p.aiModels.mouseCNN = mouseCNN.exportWeights() as unknown as Record<string, unknown>
    if (faceEmbedder?.isEnrolled) p.aiModels.faceEnrollment = faceEmbedder.exportEnrollment() as EnrollmentEmbedding
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

  const computeScores = (): { signals: SignalData; integrity: number; consistency: number; anomalies: string[] } => {
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

    // AI scoring
    if (keystrokeAE?.isTrained && keystrokeBuffer.length >= 5) {
      const aeInput: AEKeystrokeEvent[] = keystrokeBuffer.map(k => ({ key: k.key, dwellMs: k.dwellMs, flightMs: k.flightMs }))
      const anomalyScore = keystrokeAE.score(aeInput)
      if (anomalyScore >= 0) {
        signals.ai_keystroke_anomaly = Math.round(anomalyScore * 1000) / 1000
        if (keystrokeAE.isAnomalous(anomalyScore)) anomalies.push('behavior_shift')
      }
    }

    if (mouseCNN?.isTrained) {
      const mousePoints: MousePoint[] = mouseBuffer.filter(m => m.type === 'move').map(m => ({ x: m.x, y: m.y, t: m.t }))
      if (mousePoints.length >= 51) {
        const humanProb = mouseCNN.predict(mousePoints)
        if (humanProb >= 0) {
          signals.ai_mouse_human_prob = Math.round(humanProb * 1000) / 1000
          if (!mouseCNN.isHuman(humanProb)) anomalies.push('bot_suspected')
        }
      }
    }

    if (cameraOptedIn.value && faceSimilarity !== undefined) {
      signals.ai_face_similarity = Math.round(faceSimilarity * 1000) / 1000
      signals.ai_face_match = faceMatch ?? false
      if (faceMatch === false && facePresent) anomalies.push('face_mismatch')
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

    snapshotTimer = setTimeout(async () => {
      if (!isActive.value || !sessionId.value) return

      const { signals, integrity, consistency, anomalies } = computeScores()
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
            anomaly_flags: anomalies,
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
      keystrokeAE: keystrokeAE?.isTrained ? keystrokeAE.getStats() : null,
      mouseCNN: mouseCNN?.isTrained ? mouseCNN.getStats() : null,
      faceEmbedder: faceEmbedder ? { enrolled: faceEmbedder.isEnrolled, progress: faceEmbedder.enrollmentProgress } : null,
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

    // Train AI models
    if (keystrokeBuffer.length >= 20) {
      const aeInput: AEKeystrokeEvent[] = keystrokeBuffer.map(k => ({ key: k.key, dwellMs: k.dwellMs, flightMs: k.flightMs }))
      if (!keystrokeAE) keystrokeAE = new KeystrokeAutoencoder()
      keystrokeAE.train(aeInput)
    }

    if (moves.length >= 51) {
      const mousePoints: MousePoint[] = moves.map(m => ({ x: m.x, y: m.y, t: m.t }))
      if (!mouseCNN) mouseCNN = new MouseTrajectoryCNN()
      mouseCNN.train(mousePoints)
    }

    profile.lastUpdated = Date.now()
    saveProfile(userId, deviceFp, profile)
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

  const trainAIModels = async (): Promise<{
    keystrokeAE: { trained: boolean; loss: number; samples: number }
    mouseCNN: { trained: boolean; loss: number; samples: number; priorTrajectories: number }
    faceEmbedder: { enrolled: boolean; progress: number }
  }> => {
    let aeLoss = -1, aeSamples = 0
    if (keystrokeBuffer.length >= 20) {
      const aeInput: AEKeystrokeEvent[] = keystrokeBuffer.map(k => ({ key: k.key, dwellMs: k.dwellMs, flightMs: k.flightMs }))
      if (!keystrokeAE) keystrokeAE = new KeystrokeAutoencoder()
      // Keystroke-prior contrastive training is a follow-up. The AE's
      // current training is reconstruction-only on user data; a
      // "push-away" pass against ratified paste-macro priors needs
      // margin-loss plumbing that doesn't exist yet.
      aeLoss = keystrokeAE.train(aeInput)
      aeSamples = keystrokeAE.getStats().samples
    }

    let cnnLoss = -1, cnnSamples = 0
    let priorTrajectories = 0
    const moves = mouseBuffer.filter(m => m.type === 'move')
    if (moves.length >= 51) {
      const mousePoints: MousePoint[] = moves.map(m => ({ x: m.x, y: m.y, t: m.t }))
      if (!mouseCNN) mouseCNN = new MouseTrajectoryCNN()
      // Hydrate ratified bot trajectories from the Sentinel DAO library.
      // Empty array on first run is fine — CNN falls back to its 5
      // hard-coded synthetic bot patterns.
      const ratifiedBots = await fetchPriorTrajectories()
      priorTrajectories = ratifiedBots.length
      cnnLoss = mouseCNN.train(mousePoints, undefined, ratifiedBots)
      cnnSamples = mouseCNN.getStats().samples
    }

    return {
      keystrokeAE: { trained: keystrokeAE?.isTrained ?? false, loss: aeLoss, samples: aeSamples },
      mouseCNN: { trained: mouseCNN?.isTrained ?? false, loss: cnnLoss, samples: cnnSamples, priorTrajectories },
      faceEmbedder: { enrolled: faceEmbedder?.isEnrolled ?? false, progress: faceEmbedder?.enrollmentProgress ?? 0 },
    }
  }

  const enrollFace = (video: HTMLVideoElement): boolean => {
    if (!faceEmbedder) faceEmbedder = new FaceEmbedder()
    return faceEmbedder.enroll(video)
  }

  const getAIModelStatus = () => ({
    keystrokeAE: keystrokeAE ? { trained: keystrokeAE.isTrained, ...keystrokeAE.getStats() } : null,
    mouseCNN: mouseCNN ? { trained: mouseCNN.isTrained, ...mouseCNN.getStats() } : null,
    faceEmbedder: faceEmbedder ? { enrolled: faceEmbedder.isEnrolled, progress: faceEmbedder.enrollmentProgress } : null,
  })

  const resetProfile = async () => {
    const userId = stakeAddress.value
    if (!userId) return
    const deviceFp = await computeDeviceFingerprint()
    const key = `sentinel_profile_${userId}_${deviceFp.substring(0, 16)}`
    try { localStorage.removeItem(key) } catch { /* ignore */ }
    profile = null
    keystrokeAE = null
    mouseCNN = null
    faceEmbedder = null
  }

  const setAIScoringEnabled = (enabled: boolean) => {
    aiScoringEnabled.value = enabled
    try { localStorage.setItem(AI_SCORING_STORAGE_KEY, enabled ? '1' : '0') }
    catch { /* localStorage not available */ }
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
    setAIScoringEnabled,
    setCameraOptedIn,
  }
}
