import { ref, readonly, reactive } from 'vue'

type SentinelSessionPurpose = 'assessment' | 'interview'

import { listen as tauriListen, type UnlistenFn } from '@tauri-apps/api/event'
import { useLocalApi } from './useLocalApi'
import { useAuth } from './useAuth'
import { getProfileSessionToken } from './profileSession'
import {
  FaceEmbedder,
  type EnrollmentEmbedding,
} from '@/utils/sentinel/face-embedder'
import { isVirtualCameraLabel } from '@/utils/sentinel/virtual-camera'
import { downscaledSize, frameHeaders, frameSize, rgbaBody, type SentinelFrame } from '@/utils/sentinel/frame'
import { isDroppedGaze } from '@/types'
import { normaliseWebviewCombo, phantomCombos, type WebviewCombo, type BlurInterval } from '@/utils/sentinel/hotkeys'
import { createResponseTracker } from '@/utils/sentinel/responseProcess'
import { useCompanionPhone } from './useCompanionPhone'
import { isMobilePlatform } from './usePlatform'
import type {
  WindowFeatures,
  BehavioralProfile,
  StartSessionResponse,
  AttestSessionResponse,
  ProfileRequest,
  SelectedProfile,
  KeystrokeEvent,
  MousePoint,
  DigraphFeatures,
  ScorePasteResponse,
  LoadedClassifierInfo,
  UserModelStatus,
  TrainKeystrokeAeResponse,
  TrainMouseCnnResponse,
  GazeEstimate,
  ScoreGazeReply,
  GazeFeatures,
  GazeCalibSample,
  TrainGazeCalibResponse,
  DisplayTopology,
  AndroidEnvironment,
  OverlayScan,
  ProcessScan,
  HotkeyStatus,
  HotkeyEvent,
  IntegritySnapshot,
} from '@/types'

const { invoke: tauriInvoke } = useLocalApi()

// Minimum typed keystrokes before the paste classifier's timing features
// are meaningful. Below this (e.g. a click-only MCQ with no typing) the
// 12-dim feature vector is degenerate — near-zero flight reads as a paste
// burst — so the classifier does not run and its feature is absent. A
// genuine paste (pasteEventCount > 0) still runs it regardless.
const MIN_PASTE_KEYSTROKES = 12

/**
 * Sentinel Engine — the integrity collector.
 *
 * Tauri v2 implementation (no server HTTP — all via IPC).
 *
 * The composable buffers events and, once per snapshot window, submits raw
 * window features (`WindowFeatures`: counts, means, native probe readings,
 * model outputs). The backend derives every score and flag and decides the
 * session's status (`alexandria_studio::integrity`); nothing here scores.
 *
 * PRIVACY GUARANTEE: Raw keystroke timings, mouse coordinates, and video
 * frames stay in in-memory buffers. Only per-window aggregates are persisted.
 */

// ============================================================================
// Singleton state
// ============================================================================

const sessionId = ref<string | null>(null)
const isActive = ref(false)
// Set when a snapshot reports the backend suspended this session. It takes
// no more snapshots after that; the learner can still submit, and the
// session ends as suspended.
const suspended = ref(false)
const integrityScore = ref(1.0)
const consistencyScore = ref(1.0)

// Live debug snapshot — populated each snapshot dispatch so the dev-only
// Sentinel PiP can mirror exactly what the engine is computing. Module-
// scoped + reactive so any useSentinel() consumer shares one view.
function emptySentinelDebug() {
  return {
    active: false,
    cameraOptedIn: false,
    lastSnapshotAt: 0,
    integrity: 1.0,
    consistency: 1.0,
    flags: [] as string[],
    tabSwitches: 0,
    pastedChars: 0,
    keystrokeBufferLen: 0,
    mouseBufferLen: 0,
    gazeOffscreenRatio: null as number | null,
    gazeTotalChecks: 0,
    gazeOffscreenChecks: 0,
    gazeOccludedChecks: 0,
    gazeDownGlances: 0,
    aiPasteAnomaly: -1,
    aiKeystrokeAnomaly: -1,
    aiMouseHumanProb: -1,
    suspended: false,
    facePresent: false,
    faceCount: 0,
    appFocusLostCount: 0,
    appFocusLostMs: 0,
    lastApp: '' as string,
    // Native display topology (latest sample this window).
    displayCount: 0,
    externalDisplay: false,
    splitScreen: false,
    screenCaptured: false,
    displayChanges: 0,
    displaySource: '' as string,
    // Android environment (latest sample this window). An empty source
    // means no report came back: the probe does not exist off Android.
    androidEnvSource: '' as string,
    foreignAccessibility: [] as string[],
    adbEnabled: false,
    developmentSettings: false,
    overlayHidingSupported: false,
    androidSdk: 0,
    shieldRequested: false,
    shieldActive: false,
    obscuredTouches: 0,
    // Desktop hidden-overlay scan (latest sample this window). Empty
    // source = no scan on this platform (mobile, Wayland).
    overlaySource: '' as string,
    overlayScanned: 0,
    overlayAllowlisted: 0,
    overlaySuspicious: [] as string[],
    // Camera device (dev view only; never persisted).
    cameraDeviceLabel: '' as string,
    cameraDeviceVirtual: false,
    // Process watchlist (latest sample this window). Empty source = no
    // scan on this platform (mobile).
    processSource: '' as string,
    processesScanned: 0,
    watchedProcesses: [] as string[],
    // Global hotkey monitor. `hotkeysSupported` is null until the session
    // has asked (only assessment-purpose sessions do).
    hotkeysSupported: null as boolean | null,
    hotkeysSource: '' as string,
    hotkeysRunning: false,
    hotkeysPermission: false,
    nativeHotkeys: 0,
    phantomHotkeys: [] as string[],
    // Raw features submitted for the last window, and what the backend
    // derived from them (scores, flags, advisory signals).
    features: null as WindowFeatures | null,
    lastSnapshot: null as IntegritySnapshot | null,
    // Live session-gaze mirror (every camera tick, not just at snapshot) —
    // lets the dev PiP show the real session sampling rate + latest read.
    sessionGazeChecks: 0,
    lastGazeAt: 0,
    sessionGazeYaw: 0,
    sessionGazePitch: 0,
    sessionGazeOnScreen: true,
    sessionGazeOccluded: false,
    // Latest liveness read (every camera tick) and the window ratio.
    sessionLivenessRealProb: null as number | null,
    sessionLivenessSpoof: false,
    livenessSpoofRatio: null as number | null,
    // Device attestation outcome for this session (dev view).
    attestation: '' as string,
  }
}
const sentinelDebug = reactive(emptySentinelDebug())

let profileStateGeneration = 0

function profileStateGuard(signal?: AbortSignal): () => void {
  const generation = profileStateGeneration
  const token = getProfileSessionToken()
  return () => {
    if (signal?.aborted) throw new Error('Sentinel operation was cancelled')
    if (generation !== profileStateGeneration || token !== getProfileSessionToken()) {
      throw new Error('Sentinel profile operation was cancelled by locking')
    }
  }
}

// Fast timer (dev PiP) mirroring live event buffers between snapshots so
// typing / mouse activity is visible in real time, not just at snapshot
// cadence.
let liveTimer: ReturnType<typeof setInterval> | null = null

/**
 * Set when a session ends flagged, so the host can offer the learner the
 * evidence-consent decision. Cleared once they answer.
 *
 * Module-level rather than per-instance because the prompt must survive the
 * component that was running the assessment being torn down at session end.
 */
const pendingEvidenceConsent = ref<{ sessionId: string; reasons: string[] } | null>(null)
const cameraOptedIn = ref(false)

/**
 * The trust profile the backend chose for the live session (locked for the
 * session), with the inputs it judged and the policy floor result. Null
 * when no session is live or the choice failed.
 */
const trustProfile = ref<SelectedProfile | null>(null)

/** Options for `start` beyond the enrollment, camera and purpose. */
export interface SentinelStartOptions {
  /** Assessments only: the sponsor role whose policy floor applies. */
  roleAssessmentId?: string | null
  /**
   * Desktop only: pair the learner's attested phone before the trust
   * profile is chosen, so the session can be `device_attested`. Pairing is
   * the learner's consent; a skipped or failed pairing continues without.
   */
  companionPhone?: boolean
}

// AI scoring is advisory until validated with labeled data (see
// docs/sentinel.md §AI Models). Off by default; can be toggled per profile
// via setAIScoringEnabled(). The setting travels with each window's features;
// the backend adds the small advisory weights.
const AI_SCORING_STORAGE_KEY = 'sentinel_ai_scoring_enabled'
const aiScoringEnabled = ref(false)

// Per-signal opt-out for the paste classifier. Defaults to true so it
// contributes when the master AI toggle is on; users can disable it
// alone if they hit false positives without losing the other AI
// signals.
const PASTE_CLASSIFIER_STORAGE_KEY = 'sentinel_paste_classifier_enabled'
const pasteClassifierEnabled = ref(true)

/**
 * Reconcile the AI/paste-classifier flags with the per-profile
 * settings store. Call once after profile unlock.
 *
 * The legacy localStorage cache is intentionally NOT imported into
 * a fresh profile — doing so would leak the previously-active
 * profile's toggles. Each profile owns its own Sentinel
 * preferences and falls back to the registry defaults.
 */
export async function initSentinelFlagsFromSettings(): Promise<void> {
  const requireCurrent = profileStateGuard()
  const { useSettings } = await import('./useSettings')
  requireCurrent()
  const { entries, initialize } = useSettings()
  await initialize()
  requireCurrent()
  const ai = entries.value.find((e) => e.key === 'sentinel.ai_scoring_enabled')
  const paste = entries.value.find((e) => e.key === 'sentinel.paste_classifier_enabled')
  // Reset to defaults, then apply explicit overrides if present.
  aiScoringEnabled.value = ai && !ai.is_default ? ai.current_value === 'true' : false
  pasteClassifierEnabled.value =
    paste && !paste.is_default ? paste.current_value === 'true' : true
  // Clear stale localStorage cache so a hard reload does not show
  // the wrong toggle state before this hook fires again.
  try {
    localStorage.removeItem(AI_SCORING_STORAGE_KEY)
    localStorage.removeItem(PASTE_CLASSIFIER_STORAGE_KEY)
  } catch {
    /* localStorage disabled */
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

// Native app-focus tracking — driven by the Rust `sentinel://focus`
// event (window blur/focus + the OS app that took the foreground).
// Per-snapshot-window counters; `focusLostAt` carries an open blur.
let appFocusLostCount = 0
let appFocusLostMs = 0
let focusLostAt = 0
let lastFocusApp = ''
let blurStartedAt = 0   // performance.now() of the open blur, for hotkey correlation
let unlistenFocus: UnlistenFn | null = null

// Native display topology — sampled from the Rust `sentinel_display_topology`
// IPC once at session start (baseline) and once per snapshot window. The
// webview cannot see a second monitor, an iPad Split View pane, or an
// AirPlay mirror; window resizes deliberately produce no signal here.
// `displayChangeCount` counts topology transitions inside the window.
let displayBaseline: DisplayTopology | null = null
let displayLatest: DisplayTopology | null = null
let displayChangeCount = 0
// Native-reported transitions seen so far; the delta between samples
// counts toggles that began and ended between two samples.
let displayNativeTransitions = -1
let displayNativeToggle = false
let unlistenDisplay: UnlistenFn | null = null
let displayResampleTimer: ReturnType<typeof setTimeout> | null = null

/** Stable identity for "did the arrangement change" (ignores `source`). */
const displaySignature = (t: DisplayTopology): string =>
  `${t.display_count}|${t.external_display ? 1 : 0}|${t.mirrored ? 1 : 0}|${t.split_screen ? 1 : 0}`

/**
 * Fold a fresh topology sample into the per-window state. Returns the
 * sample; the window's display features read the state it leaves.
 */
const recordDisplaySample = (sample: DisplayTopology | null): DisplayTopology | null => {
  if (!sample || typeof sample.display_count !== 'number') return null
  if (!displayBaseline) displayBaseline = sample
  if (displayLatest && displaySignature(displayLatest) !== displaySignature(sample)) {
    displayChangeCount++
  }
  const native = sample.native_transitions ?? 0
  if (displayNativeTransitions >= 0 && native > displayNativeTransitions) {
    displayChangeCount += native - displayNativeTransitions
    displayNativeToggle = true
  }
  displayNativeTransitions = native
  displayLatest = sample
  return sample
}

// Android environment — sampled per snapshot window from the Rust
// `sentinel_android_environment` IPC (null everywhere else). The
// obscured-touch counter is drained with it. The assessment shield
// (FLAG_SECURE + setHideOverlayWindows) is engaged only while an
// assessment element is current, so course reading stays screenshot-able.
let androidEnvLatest: AndroidEnvironment | null = null
let obscuredTouchesWindow = 0
let shieldRequested = false
let sessionPurpose: SentinelSessionPurpose = 'assessment'

// Desktop hidden-overlay scan — windows excluded from screen capture
// (Cluely-style), override-redirect (X11) or click-through topmost
// (Windows), minus our own and an allowlist. Sampled per snapshot window.
let overlayLatest: OverlayScan | null = null

// Desktop process watchlist — only watched entries cross IPC. Sampled
// per snapshot window; the window reports the categories seen and the
// backend decides what each means.
let processLatest: ProcessScan | null = null

// Global hotkeys — native listen-only monitor of modifier combos (desktop,
// assessment purpose only, only when the OS already granted listening).
// Per window: combos the OS saw but the webview never received, while our
// window was focused, are "phantom": an overlay tool's hotkey.
let hotkeyStatus: HotkeyStatus | null = null
let hotkeysRunning = false
let hotkeyEpoch = 0                       // performance.now() at monitor t=0
let webviewCombos: WebviewCombo[] = []    // combos the webview received this window
let blurIntervals: BlurInterval[] = []    // [start,end] in performance.now() space
let phantomWindow: string[] = []
let nativeHotkeyCount = 0
const WEBVIEW_COMBO_CAP = 512

// Camera device identity — the video track label the player / interview
// session obtained from getUserMedia. Only the virtual-camera verdict
// reaches scores and snapshots; the label stays in the dev debug view.
let cameraDeviceLabel = ''
let cameraDeviceVirtual = false

// Gaze / second-device tracking, accumulated per snapshot window by
// scoreGaze() and drained in the snapshot dispatch. All gaze inference
// runs in the Rust backend (YuNet + head-pose); the frontend only
// forwards downscaled frames and tallies the verdicts.
let gazeTotalChecks = 0
let gazeOffscreenChecks = 0
let gazeOccludedChecks = 0
let gazeDownGlances = 0
// Liveness (MiniFASNet) accumulators for the current snapshot window.
let livenessChecks = 0
let livenessSpoofChecks = 0
let livenessRealProbSum = 0
// Reusable offscreen canvas for frame downscaling to the detector size.
let gazeCanvas: HTMLCanvasElement | null = null

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

const loadedClassifierInfo = ref<LoadedClassifierInfo>({ source: 'bundled', version: 'bundled-v1' })

export function getLoadedClassifierInfo(): LoadedClassifierInfo {
  return loadedClassifierInfo.value
}

async function refreshPasteClassifierInfo(): Promise<void> {
  const requireCurrent = profileStateGuard()
  try {
    const info = await tauriInvoke<LoadedClassifierInfo>('sentinel_paste_classifier_info')
    requireCurrent()
    loadedClassifierInfo.value = info
  } catch { /* backend not ready yet, or the profile changed */ }
}

// ============================================================================
// Composable
// ============================================================================

let sentinelService: ReturnType<typeof createSentinelService> | undefined

export function useSentinel() {
  return sentinelService ??= createSentinelService()
}

function createSentinelService() {
  const { invoke } = useLocalApi()
  const { stakeAddress } = useAuth()
  let lifecycleGeneration = 0
  let cameraGeneration = 0
  let lifecycleTransition: Promise<unknown> = Promise.resolve()
  let trainingKeystrokesCleanup: (() => void) | null = null
  // Response-process timing per item (`responses.present` / `.input` /
  // `.cue`); reset with every session.
  const responseTracker = createResponseTracker()
  let trainingMouseCleanup: (() => void) | null = null

  const serializeTransition = <T>(operation: () => Promise<T>): Promise<T> => {
    const result = lifecycleTransition.then(operation)
    lifecycleTransition = result.catch(() => undefined)
    return result
  }

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
  // Profile management (profile-scoped SQLCipher storage)
  // =========================================================================

  const legacyProfileKey = (userId: string, deviceFp: string) =>
    `sentinel_profile_${userId}_${deviceFp.substring(0, 16)}`

  const loadProfile = async (userId: string, deviceFp: string): Promise<BehavioralProfile | null> => {
    const deviceFpPrefix = deviceFp.substring(0, 16)
    const stored = await tauriInvoke<BehavioralProfile | null>('sentinel_load_behavioral_profile', {
      userAddress: userId,
      deviceFpPrefix,
    })
    // Pre-SQLCipher builds kept one browser-storage key per exact learner and
    // device fingerprint. That format is no longer read: the key is erased so
    // a private behavioural record does not sit in browser storage, and the
    // profile is rebuilt from training rather than imported unverified.
    try { localStorage.removeItem(legacyProfileKey(userId, deviceFp)) }
    catch { /* localStorage not available */ }
    return stored
  }

  const saveProfile = async (userId: string, deviceFp: string, p: BehavioralProfile): Promise<void> => {
    if (p.userId !== userId || p.deviceFingerprint !== deviceFp) {
      throw new Error('Sentinel profile ownership changed before it was saved')
    }
    persistAIModels(p)
    await tauriInvoke('sentinel_save_behavioral_profile', { profile: p })
    try { localStorage.removeItem(legacyProfileKey(userId, deviceFp)) }
    catch { /* localStorage not available */ }
  }

  const loadAIModels = (p: BehavioralProfile) => {
    // Keystroke AE + mouse CNN weights now persist in the backend
    // `sentinel_user_models` table (encrypted SQLite). Only the face
    // enrollment shares the profile's encrypted database row with the
    // behavioral baseline because the face embedder itself runs in the webview.
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
    // `sentinel_user_models_status`. Don't duplicate those weights here.
    const legacyModels = p.aiModels as BehavioralProfile['aiModels'] & {
      keystrokeAutoencoder?: unknown
      mouseCNN?: unknown
    }
    delete legacyModels.keystrokeAutoencoder
    delete legacyModels.mouseCNN
  }

  /**
   * Pull current per-user model status from the backend. Updates the
   * module-scoped refs that decide whether a window runs the AE / CNN.
   */
  const refreshUserModelsStatus = async (signal?: AbortSignal) => {
    const requireCurrent = profileStateGuard(signal)
    const userId = stakeAddress.value
    if (!userId) return
    const deviceFp = await computeDeviceFingerprint()
    try {
      requireCurrent()
      const rows = await tauriInvoke<UserModelStatus[]>(
        'sentinel_user_models_status',
        { userAddress: userId, deviceFpPrefix: deviceFp.substring(0, 16) },
      )
      requireCurrent()
      keystrokeAeStatus.value =
        rows.find(r => r.model_kind === 'keystroke_ae') ?? null
      mouseCnnStatus.value =
        rows.find(r => r.model_kind === 'mouse_cnn') ?? null
    } catch (err) {
      console.warn('[sentinel] user model status fetch failed:', err)
    }
  }

  // =========================================================================
  // Calibration analyzers (wizard metrics and typing speed only; session
  // scores are the backend's)
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

  const mean = (values: number[]): number =>
    values.length > 0 ? values.reduce((a, b) => a + b, 0) / values.length : 0

  /** A model score of -1 (or anything negative) means the model did not run. */
  const modelScore = (score: number | undefined): number | null =>
    score !== undefined && score >= 0 ? score : null

  /**
   * The raw features of the current window. Measurements only: the backend
   * derives every score and flag from these (`integrity_submit_snapshot`).
   */
  const collectFeatures = (models?: {
    pasteAnomaly?: number
    keystrokeAnomaly?: number
    mouseHumanProb?: number
  }): WindowFeatures => {
    const flights = keystrokeBuffer.filter(k => k.flightMs > 0).map(k => k.flightMs)
    const moves = mouseBuffer.filter(m => m.type === 'move')
    const velocities: number[] = []
    for (let i = 1; i < moves.length; i++) {
      const curr = moves[i]!
      const prev = moves[i - 1]!
      const dt = curr.t - prev.t
      if (dt > 0) velocities.push(Math.hypot(curr.x - prev.x, curr.y - prev.y) / dt)
    }
    const meanVelocity = mean(velocities)
    const watched = processLatest?.watched ?? []

    return {
      typing: {
        keystrokes: keystrokeBuffer.length,
        mean_dwell_ms: Math.max(0, mean(keystrokeBuffer.map(k => k.dwellMs))),
        mean_flight_ms: flights.length > 0 ? mean(flights) : null,
        speed_wpm: analyzeKeystrokes().speedWpm,
      },
      mouse: {
        points: mouseBuffer.length,
        velocity_samples: velocities.length,
        mean_velocity: meanVelocity,
        velocity_variance: mean(velocities.map(v => (v - meanVelocity) ** 2)),
      },
      baseline: profile
        ? {
            typing_samples: profile.typingPattern.sampleCount,
            avg_dwell_ms: profile.typingPattern.avgDwellTime,
            avg_flight_ms: profile.typingPattern.avgFlightTime,
            speed_wpm: profile.typingPattern.speedWpm,
            mouse_samples: profile.mousePattern.sampleCount,
            avg_velocity: profile.mousePattern.avgVelocity,
          }
        : null,
      focus: {
        tab_switches: tabSwitchCount,
        unfocused_ms: totalUnfocusedMs,
        app_switches: appFocusLostCount,
        app_unfocused_ms: appFocusLostMs,
        environment_changed: environmentChanged,
      },
      clipboard: { paste_events: pasteEventCount, pasted_chars: pastedCharCount },
      display: displayLatest
        ? {
            display_count: displayLatest.display_count,
            external_display: displayLatest.external_display,
            split_screen: displayLatest.split_screen,
            screen_captured: displayLatest.mirrored,
            native_toggle: displayNativeToggle,
          }
        : null,
      display_changes: displayChangeCount,
      android: androidEnvLatest
        ? {
            foreign_accessibility_services: androidEnvLatest.foreign_accessibility?.length ?? 0,
            adb_enabled: androidEnvLatest.adb_enabled,
          }
        : null,
      obscured_touches: obscuredTouchesWindow,
      overlay: overlayLatest
        ? {
            suspicious: overlayLatest.suspicious?.length ?? 0,
            scanned: overlayLatest.scanned,
            allowlisted: overlayLatest.allowlisted,
          }
        : null,
      processes: processLatest
        ? {
            categories: [...new Set(watched.map(w => w.category))],
            watched: watched.length,
            scanned: processLatest.scanned,
          }
        : null,
      hotkeys: hotkeysRunning ? { native: nativeHotkeyCount, phantom: phantomWindow.length } : null,
      camera: cameraOptedIn.value
        ? {
            device_virtual: cameraDeviceLabel ? cameraDeviceVirtual : null,
            face_present: facePresent ?? null,
            face_count: faceCount ?? null,
            face_consistency: faceConsistency ?? null,
            face_similarity: faceSimilarity ?? null,
            face_match: faceMatch ?? null,
            consecutive_no_face: consecutiveNoFaceChecks,
            face_checks: totalFaceChecks,
            face_absent_checks: faceAbsentChecks,
            gaze_checks: gazeTotalChecks,
            gaze_offscreen_checks: gazeOffscreenChecks,
            gaze_occluded_checks: gazeOccludedChecks,
            gaze_down_glances: gazeDownGlances,
            liveness_checks: livenessChecks,
            liveness_spoof_checks: livenessSpoofChecks,
            liveness_real_prob: livenessChecks > 0 ? livenessRealProbSum / livenessChecks : null,
          }
        : null,
      models: {
        paste_anomaly: modelScore(models?.pasteAnomaly),
        keystroke_anomaly: modelScore(models?.keystrokeAnomaly),
        mouse_human_prob: modelScore(models?.mouseHumanProb),
        advisory_enabled: aiScoringEnabled.value,
        paste_classifier_enabled: pasteClassifierEnabled.value,
      },
      response: responseTracker.pending(),
    }
  }

  // =========================================================================
  // Profile update (EMA)
  // =========================================================================

  const updateProfile = async (deviceFp: string): Promise<void> => {
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

    const responseHistory = responseTracker.historyAfterSession()
    if (responseHistory.length > 0) profile.responsePattern = { recent: responseHistory }

    profile.lastUpdated = Date.now()
    await saveProfile(profile.userId, deviceFp, profile)
  }

  // =========================================================================
  // Snapshot scheduling (random interval 15-45s)
  // =========================================================================

  let snapshotWork: Promise<void> | null = null

  const captureSnapshot = async (strict: boolean) => {
    const generation = lifecycleGeneration
    const snapshotSessionId = sessionId.value
    const isCurrent = () => isActive.value && !!snapshotSessionId
      && generation === lifecycleGeneration && sessionId.value === snapshotSessionId
      if (!isCurrent()) return

      // The model outputs below travel with the window's features. A score
      // of -1 means the model artifact wasn't available; the feature is then
      // simply absent, mirroring the convention used by the other models.
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
      if (!isCurrent()) return

      let pasteAnomaly = -1
      // Only run the paste classifier when there's enough typing for its
      // timing features to be meaningful, or an actual paste occurred.
      // Otherwise the signal is absent (-1) — avoids false positives on
      // low-/no-typing snapshots (e.g. MCQ clicking).
      if (keystrokeBuffer.length >= MIN_PASTE_KEYSTROKES || pasteEventCount > 0) {
        try {
          const resp = await tauriInvoke<ScorePasteResponse>('sentinel_score_paste', {
            req: {
              events: keystrokeBuffer.map(k => ({ key: k.key, dwellMs: k.dwellMs, flightMs: k.flightMs })),
              paste_event_count: pasteEventCount,
              pasted_char_count: pastedCharCount,
              window_ms: windowMs,
            },
          })
          if (!isCurrent()) return
          pasteAnomaly = resp.score
          loadedClassifierInfo.value = resp.classifier
        } catch (err) {
          console.warn('[sentinel] paste score IPC failed', err)
        }
      }

      let keystrokeAnomaly = -1
      if (!isCurrent()) return
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
      if (!isCurrent()) return
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

      // Native display topology: one sample per window. A transition
      // since the previous sample (monitor plugged in, Split View
      // entered, mirroring started) is counted; the latest state drives
      // the window's display features.
      if (!isCurrent()) return
      try {
        const topo = await tauriInvoke<DisplayTopology | null>('sentinel_display_topology')
        if (!isCurrent()) return
        recordDisplaySample(topo)
      } catch (err) {
        console.warn('[sentinel] display topology IPC failed', err)
      }

      // Android environment + obscured-touch drain (both inert off Android).
      if (!isCurrent()) return
      try {
        const env = await tauriInvoke<AndroidEnvironment | null>('sentinel_android_environment')
        // Shape-check native payloads: a stub or mismatched backend must read as absent, not crash scoring.
        androidEnvLatest = env && Array.isArray(env.foreign_accessibility) ? env : null
        if (!isCurrent()) return
        const touches = await tauriInvoke<number>('sentinel_take_obscured_touches')
        obscuredTouchesWindow = typeof touches === 'number' ? touches : 0
      } catch (err) {
        console.warn('[sentinel] android environment IPC failed', err)
      }

      // Desktop hidden-overlay scan (null on mobile / Wayland).
      if (!isCurrent()) return
      try {
        const overlay = await tauriInvoke<OverlayScan | null>('sentinel_hidden_overlay')
        overlayLatest = overlay && Array.isArray(overlay.suspicious) ? overlay : null
      } catch (err) {
        console.warn('[sentinel] hidden overlay IPC failed', err)
      }

      // Desktop process watchlist (null on mobile).
      if (!isCurrent()) return
      try {
        const procs = await tauriInvoke<ProcessScan | null>('sentinel_process_scan')
        processLatest = procs && Array.isArray(procs.watched) ? procs : null
      } catch (err) {
        console.warn('[sentinel] process scan IPC failed', err)
      }

      // Global hotkeys: drain the native ring and subtract what the webview saw.
      if (!isCurrent()) return
      if (hotkeysRunning && hotkeyStatus) {
        try {
          const drained = await tauriInvoke<HotkeyEvent[]>('sentinel_hotkeys_drain')
          const native = Array.isArray(drained) ? drained : []
          if (!isCurrent()) return
          nativeHotkeyCount = native.length
          if (blurStartedAt) blurIntervals.push({ start: blurStartedAt, end: performance.now() })
          phantomWindow = phantomCombos({
            native, epoch: hotkeyEpoch, webview: webviewCombos, blurs: blurIntervals,
            osCombos: hotkeyStatus.os_combos, cmdIsSystem: hotkeyStatus.cmd_is_system,
          }).map(e => e.combo)
        } catch (err) {
          console.warn('[sentinel] hotkey drain IPC failed', err)
        }
      }

      if (!isCurrent()) return
      // The last item's visit ends with the attempt.
      if (strict) responseTracker.settle()
      // Native app-switch: roll any still-open blur into the elapsed total.
      if (focusLostAt) { appFocusLostMs += Date.now() - focusLostAt; focusLostAt = Date.now() }
      const features = collectFeatures({
        pasteAnomaly,
        keystrokeAnomaly,
        mouseHumanProb,
      })
      const gazeOffscreenRatio = features.camera && gazeTotalChecks > 0
        ? gazeOffscreenChecks / gazeTotalChecks
        : null
      sentinelDebug.livenessSpoofRatio = features.camera && livenessChecks > 0
        ? livenessSpoofChecks / livenessChecks
        : null

      // Mirror this window into the live debug view (dev PiP).
      sentinelDebug.active = isActive.value
      sentinelDebug.cameraOptedIn = cameraOptedIn.value
      sentinelDebug.lastSnapshotAt = Date.now()
      sentinelDebug.tabSwitches = tabSwitchCount
      sentinelDebug.pastedChars = pastedCharCount
      sentinelDebug.keystrokeBufferLen = keystrokeBuffer.length
      sentinelDebug.mouseBufferLen = mouseBuffer.length
      sentinelDebug.gazeOffscreenRatio = gazeOffscreenRatio
      sentinelDebug.gazeTotalChecks = gazeTotalChecks
      sentinelDebug.gazeOffscreenChecks = gazeOffscreenChecks
      sentinelDebug.gazeOccludedChecks = gazeOccludedChecks
      sentinelDebug.gazeDownGlances = gazeDownGlances
      sentinelDebug.aiPasteAnomaly = pasteAnomaly
      sentinelDebug.aiKeystrokeAnomaly = keystrokeAnomaly
      sentinelDebug.aiMouseHumanProb = mouseHumanProb
      sentinelDebug.facePresent = facePresent ?? false
      sentinelDebug.faceCount = faceCount ?? 0
      sentinelDebug.displayCount = displayLatest?.display_count ?? 0
      sentinelDebug.externalDisplay = displayLatest?.external_display ?? false
      sentinelDebug.splitScreen = displayLatest?.split_screen ?? false
      sentinelDebug.screenCaptured = displayLatest?.mirrored ?? false
      sentinelDebug.displayChanges = displayChangeCount
      sentinelDebug.displaySource = displayLatest?.source ?? ''
      sentinelDebug.androidEnvSource = androidEnvLatest?.source ?? ''
      sentinelDebug.foreignAccessibility = androidEnvLatest?.foreign_accessibility ?? []
      sentinelDebug.adbEnabled = androidEnvLatest?.adb_enabled ?? false
      sentinelDebug.developmentSettings = androidEnvLatest?.development_settings_enabled ?? false
      sentinelDebug.overlayHidingSupported = androidEnvLatest?.overlay_hiding_supported ?? false
      sentinelDebug.androidSdk = androidEnvLatest?.sdk_int ?? 0
      sentinelDebug.shieldRequested = shieldRequested
      sentinelDebug.shieldActive = androidEnvLatest?.shield_active ?? false
      sentinelDebug.obscuredTouches = obscuredTouchesWindow
      sentinelDebug.overlaySource = overlayLatest?.source ?? ''
      sentinelDebug.overlayScanned = overlayLatest?.scanned ?? 0
      sentinelDebug.overlayAllowlisted = overlayLatest?.allowlisted ?? 0
      sentinelDebug.overlaySuspicious = overlayLatest?.suspicious.map(w => `${w.owner}:${w.reason}`) ?? []
      sentinelDebug.cameraDeviceLabel = cameraDeviceLabel
      sentinelDebug.cameraDeviceVirtual = cameraDeviceVirtual
      sentinelDebug.processSource = processLatest?.source ?? ''
      sentinelDebug.processesScanned = processLatest?.scanned ?? 0
      sentinelDebug.watchedProcesses = processLatest?.watched.map(w => `${w.name}:${w.category}`) ?? []
      sentinelDebug.hotkeysSupported = hotkeyStatus ? hotkeyStatus.supported : null
      sentinelDebug.hotkeysSource = hotkeyStatus?.source ?? ''
      sentinelDebug.hotkeysRunning = hotkeysRunning
      sentinelDebug.hotkeysPermission = hotkeyStatus?.permission_granted ?? false
      sentinelDebug.nativeHotkeys = nativeHotkeyCount
      sentinelDebug.phantomHotkeys = phantomWindow
      sentinelDebug.appFocusLostCount = appFocusLostCount
      sentinelDebug.appFocusLostMs = appFocusLostMs
      sentinelDebug.lastApp = lastFocusApp
      sentinelDebug.features = features

      try {
        // Features only: the backend derives the scores and flags, and says
        // what the session's status became.
        const stored = await invoke<IntegritySnapshot | null>('integrity_submit_snapshot', {
          req: {
            session_id: snapshotSessionId,
            element_id: currentElementId,
            features,
          },
        })
        if (stored) {
          if (typeof stored.composite_score === 'number') integrityScore.value = stored.composite_score
          if (typeof stored.consistency_score === 'number') consistencyScore.value = stored.consistency_score
          sentinelDebug.integrity = integrityScore.value
          sentinelDebug.consistency = consistencyScore.value
          sentinelDebug.flags = stored.anomaly_flags ?? []
          sentinelDebug.lastSnapshot = stored
        }
        if (stored?.session_status === 'suspended') {
          suspended.value = true
          sentinelDebug.suspended = true
        }
      } catch (error) { if (strict) throw error }
      if (!isCurrent()) return

      // Reset per-snapshot accumulators
      gazeTotalChecks = 0
      gazeOffscreenChecks = 0
      gazeOccludedChecks = 0
      gazeDownGlances = 0
      livenessChecks = 0
      livenessSpoofChecks = 0
      livenessRealProbSum = 0
      appFocusLostCount = 0
      appFocusLostMs = 0
      displayChangeCount = 0
      displayNativeToggle = false
      obscuredTouchesWindow = 0
      webviewCombos = []
      blurIntervals = []
      phantomWindow = []
      nativeHotkeyCount = 0
      keystrokeBuffer = []
      mouseBuffer = []
      tabSwitchCount = 0
      totalUnfocusedMs = 0
      pasteEventCount = 0
      pastedCharCount = 0
      environmentChanged = false
      totalFaceChecks = 0
      faceAbsentChecks = 0
      gazeTotalChecks = 0
      gazeOffscreenChecks = 0
      gazeOccludedChecks = 0
      gazeDownGlances = 0
      livenessChecks = 0
      livenessSpoofChecks = 0
      livenessRealProbSum = 0
      responseTracker.clearWindow()
      snapshotWindowStartMs = Date.now()

  }

  const scheduleNextSnapshot = () => {
    if (!isActive.value || !sessionId.value || suspended.value) return
    if (snapshotWindowStartMs === 0) snapshotWindowStartMs = Date.now()
    snapshotTimer = setTimeout(() => {
      snapshotWork = captureSnapshot(false).finally(() => {
        snapshotWork = null
        scheduleNextSnapshot()
      })
    }, 15000 + Math.random() * 30000)
  }

  // =========================================================================
  // Event listeners
  // =========================================================================

  const recordKeyDown = (e: KeyboardEvent) => {
    const now = performance.now()
    if (hotkeysRunning) {
      const combo = normaliseWebviewCombo(e, hotkeyStatus?.cmd_is_system ?? false)
      if (combo) {
        webviewCombos.push({ at: now, combo })
        if (webviewCombos.length > WEBVIEW_COMBO_CAP) webviewCombos.shift()
      }
    }
    const flightMs = lastKeystrokeTime > 0 ? now - lastKeystrokeTime : 0
    keystrokeBuffer.push({
      key: e.key.length === 1 ? 'char' : e.key,
      dwellMs: 0,
      flightMs,
    })
    lastKeystrokeTime = now
  }

  const recordKeyUp = () => {
    const now = performance.now()
    if (keystrokeBuffer.length > 0) {
      const last = keystrokeBuffer[keystrokeBuffer.length - 1]!
      last.dwellMs = now - (lastKeystrokeTime - last.flightMs)
    }
  }

  const recordMouseMove = (e: MouseEvent) => {
    const now = performance.now()
    if (mouseBuffer.length > 0 && now - mouseBuffer[mouseBuffer.length - 1]!.t < 50) return
    mouseBuffer.push({ x: e.clientX, y: e.clientY, t: now, type: 'move' })
    if (mouseBuffer.length > 200) mouseBuffer = mouseBuffer.slice(-100)
  }

  const recordMouseClick = (e: MouseEvent) => {
    mouseBuffer.push({ x: e.clientX, y: e.clientY, t: performance.now(), type: 'click' })
  }

  const onKeyDown = (e: KeyboardEvent) => { if (isActive.value) recordKeyDown(e) }
  const onKeyUp = () => { if (isActive.value) recordKeyUp() }
  const onMouseMove = (e: MouseEvent) => { if (isActive.value) recordMouseMove(e) }
  const onMouseClick = (e: MouseEvent) => { if (isActive.value) recordMouseClick(e) }

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

  // =========================================================================
  // Public API
  // =========================================================================

  const startSession = async (
    enrollmentId: string | null,
    optInCamera: boolean,
    purpose: SentinelSessionPurpose,
    generation: number,
    options: SentinelStartOptions,
  ) => {
    if (isActive.value || generation !== lifecycleGeneration) return
    if (sessionId.value) throw new Error('Finish closing the previous Sentinel session before starting another')

    cameraGeneration++
    cameraOptedIn.value = optInCamera

    const deviceFp = await computeDeviceFingerprint()
    if (generation !== lifecycleGeneration) return
    const userId = stakeAddress.value
    faceEmbedder = null
    profile = userId ? await loadProfile(userId, deviceFp) : null
    if (profile) loadAIModels(profile)
    if (generation !== lifecycleGeneration) return
    responseTracker.reset()
    responseTracker.setHistory(profile?.responsePattern?.recent ?? [])

    try {
      const response = await invoke<StartSessionResponse>('integrity_start_session', { enrollmentId, purpose })
      sessionId.value = response.session_id
      clearTrainingBuffers()
      suspended.value = false
      // A new session starts from a clean dev view: the previous session's
      // readings would otherwise show until this one's first snapshot. The
      // camera device can be reported before the session starts, so it stays.
      Object.assign(sentinelDebug, emptySentinelDebug(), { cameraDeviceLabel, cameraDeviceVirtual })
      isActive.value = true
      // Device attestation (iOS App Attest / Android Play Integrity) binds
      // the platform's word to this session's nonce. The backend records the
      // outcome and desktop reports unsupported; a failure leaves it local.
      try {
        const r = await invoke<AttestSessionResponse>('integrity_attest_session', { sessionId: response.session_id })
        sentinelDebug.attestation = r.supported ? (r.stored ? r.assurance_level : 'unavailable') : 'unsupported'
      } catch (err) {
        console.warn('[sentinel] device attestation failed', err)
        sentinelDebug.attestation = 'error'
      }
      if (generation !== lifecycleGeneration) return
      // Desktops have no attestation of their own; a paired phone lends its.
      const companion = options.companionPhone && !isMobilePlatform
        ? await useCompanionPhone().pairPhone(response.session_id)
        : null
      if (companion) sentinelDebug.attestation = `${companion.assurance_level} (phone)`
      if (generation !== lifecycleGeneration) return
      // The backend chooses the trust profile from the attestation result,
      // this report, a device benchmark, the role's floor and the consent,
      // and locks it before the first snapshot. The camera runs only when
      // the chosen profile includes it.
      trustProfile.value = await selectProfile(response.session_id, optInCamera, options, companion !== null)
      if (generation !== lifecycleGeneration) return
      if (optInCamera && trustProfile.value?.not_run.includes('camera')) setCameraOptedIn(false)
      // A queued stop owns cleanup of an already-created backend session.
      // Do not attach new listeners while that cleanup is pending.
      if (generation !== lifecycleGeneration) return
      sentinelDebug.active = true
      snapshotWindowStartMs = Date.now()

      // Report the bundled paste classifier version to dashboard cards.
      void refreshPasteClassifierInfo()

      // Refresh per-user model status so the snapshot path knows whether
      // to call the AE / CNN scoring IPCs.
      void refreshUserModelsStatus()

      document.addEventListener('keydown', onKeyDown, { passive: true })
      document.addEventListener('keyup', onKeyUp, { passive: true })
      document.addEventListener('mousemove', onMouseMove, { passive: true })
      document.addEventListener('click', onMouseClick, { passive: true })
      document.addEventListener('visibilitychange', onVisibilityChange)
      document.addEventListener('paste', onPaste)

      // Native window-focus signal — fires when the OS switches the
      // foreground app away from the assessment (webview can't see this).
      appFocusLostCount = 0
      appFocusLostMs = 0
      focusLostAt = 0
      lastFocusApp = ''
      try {
        unlistenFocus = await tauriListen<{ focused: boolean; app?: { name: string; identifier: string } | null }>(
          'sentinel://focus',
          (e) => {
            if (!isActive.value) return
            if (e.payload.focused) {
              if (focusLostAt) { appFocusLostMs += Date.now() - focusLostAt; focusLostAt = 0 }
              if (blurStartedAt) { blurIntervals.push({ start: blurStartedAt, end: performance.now() }); blurStartedAt = 0 }
            } else {
              appFocusLostCount++
              focusLostAt = Date.now()
              blurStartedAt = performance.now()
              if (e.payload.app?.name) {
                lastFocusApp = e.payload.app.name
                sentinelDebug.lastApp = lastFocusApp // live PiP feedback
              }
            }
          },
        )
      } catch (err) {
        console.warn('[sentinel] focus listener failed', err)
      }
      if (generation !== lifecycleGeneration) return

      // Shield: a standalone assessment (no enrollment) is an assessment
      // from the first frame; the course player engages it via setElement.
      sessionPurpose = purpose
      androidEnvLatest = null
      obscuredTouchesWindow = 0
      overlayLatest = null
      processLatest = null
      if (purpose === 'assessment' && enrollmentId === null) void setAssessmentShield(true)
      // The Android counter runs since process start; discard touches that
      // predate this session so window 1 cannot inherit an old overlay tap.
      void tauriInvoke<number>('sentinel_take_obscured_touches').catch(() => { /* inert off Android */ })
      if (generation !== lifecycleGeneration) return

      // Global hotkey monitor: assessments only, and only when the OS has
      // already granted listening (the permission prompt lives in the
      // wizard). A refusal just leaves the signal absent.
      hotkeyStatus = null
      hotkeysRunning = false
      webviewCombos = []
      blurIntervals = []
      phantomWindow = []
      nativeHotkeyCount = 0
      if (purpose === 'assessment') {
        try {
          const st = await tauriInvoke<HotkeyStatus>('sentinel_hotkeys_status')
          if (generation !== lifecycleGeneration) return
          hotkeyStatus = st
          if (st.supported && st.permission_granted) {
            const started = await tauriInvoke<HotkeyStatus>('sentinel_hotkeys_start')
            if (generation !== lifecycleGeneration) { void tauriInvoke('sentinel_hotkeys_stop').catch(() => undefined); return }
            hotkeyEpoch = performance.now()
            hotkeyStatus = started
            hotkeysRunning = started.running
          }
        } catch (err) {
          console.warn('[sentinel] hotkey monitor unavailable', err)
          hotkeysRunning = false
        }
      }
      if (generation !== lifecycleGeneration) return

      // Display-topology baseline, so the first snapshot can tell a
      // pre-existing second monitor from one plugged in mid-session.
      displayBaseline = null
      displayLatest = null
      displayChangeCount = 0
      displayNativeTransitions = -1
      displayNativeToggle = false
      try {
        const topo = await tauriInvoke<DisplayTopology | null>('sentinel_display_topology')
        if (generation !== lifecycleGeneration) return
        recordDisplaySample(topo)
      } catch (err) {
        console.warn('[sentinel] display topology baseline failed', err)
      }
      if (generation !== lifecycleGeneration) return

      // Native nudge (window moved to another monitor / scale changed):
      // resample right away instead of waiting for the snapshot. Debounced
      // because a drag emits a stream of moves.
      try {
        unlistenDisplay = await tauriListen<{ reason: string }>('sentinel://display', () => {
          if (!isActive.value) return
          if (displayResampleTimer) clearTimeout(displayResampleTimer)
          displayResampleTimer = setTimeout(async () => {
            displayResampleTimer = null
            if (!isActive.value || generation !== lifecycleGeneration) return
            try {
              const topo = await tauriInvoke<DisplayTopology | null>('sentinel_display_topology')
              if (isActive.value && generation === lifecycleGeneration) recordDisplaySample(topo)
            } catch (err) {
              console.warn('[sentinel] display resample failed', err)
            }
          }, 300)
        })
      } catch (err) {
        console.warn('[sentinel] display listener failed', err)
      }
      if (generation !== lifecycleGeneration) return

      // Live activity mirror for the dev PiP (typing / mouse between snapshots).
      if (liveTimer) clearInterval(liveTimer)
      liveTimer = setInterval(() => {
        sentinelDebug.keystrokeBufferLen = keystrokeBuffer.length
        sentinelDebug.mouseBufferLen = mouseBuffer.length
      }, 400)

      scheduleNextSnapshot()
    } catch (e) {
      console.warn('Sentinel: failed to start session', e)
      throw e
    }
  }

  const setElement = (elementId: string, elementType: string) => {
    currentElementId = elementId
    currentElementType = elementType
    if (isActive.value && sessionPurpose === 'assessment') {
      void setAssessmentShield(isAssessmentElement(elementType))
    }
  }

  const isAssessmentElement = (elementType: string): boolean => {
    return ['quiz', 'assessment', 'interactive'].includes(elementType)
  }

  // Engage / release the Android assessment shield. Calls are serialised so
  // an on/off pair cannot land out of order on the native side, and the
  // recorded state only moves on success so a failed call is retried by
  // the next request. Best effort: a failure is logged, never thrown.
  let shieldChain: Promise<void> = Promise.resolve()
  const setAssessmentShield = (on: boolean): Promise<void> => {
    shieldChain = shieldChain.then(async () => {
      if (shieldRequested === on) return
      try {
        await tauriInvoke('sentinel_set_assessment_shield', { on })
        shieldRequested = on
      } catch (err) {
        console.warn('[sentinel] assessment shield IPC failed', err)
      }
    })
    return shieldChain
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

  const verifyFace = (video: SentinelFrame): {
    present: boolean; count: number; consistency: number
    similarity?: number; match?: boolean
  } => {
    if (!video || !frameSize(video)) return { present: false, count: 0, consistency: 0.2 }

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

  // Backend gaze / second-device check. Draws a downscaled frame and
  // forwards it to the Rust YuNet + head-pose pipeline; tallies the
  // verdict into the per-window gaze accumulators. Returns the estimate
  // for callers that want to surface it live, or null on any failure
  // (gaze is advisory — failures never break the monitoring loop).
  const scoreGaze = async (
    video: SentinelFrame,
  ): Promise<GazeEstimate | null> => {
    const size = video ? frameSize(video) : null
    if (!isActive.value || !sessionId.value || !cameraOptedIn.value || !size || document.hidden) return null
    const generation = lifecycleGeneration
    const captureGeneration = cameraGeneration
    const gazeSessionId = sessionId.value
    const isCurrent = () => isActive.value && generation === lifecycleGeneration
      && sessionId.value === gazeSessionId && cameraOptedIn.value && captureGeneration === cameraGeneration
    const userId = stakeAddress.value
    if (!userId) return null
    try {
      // Cap the longest side at 224px — the backend detects at the
      // frame's own size, so this sets both IPC payload and model cost.
      const { width: w, height: h } = downscaledSize(size)
      if (!gazeCanvas) gazeCanvas = document.createElement('canvas')
      gazeCanvas.width = w
      gazeCanvas.height = h
      const ctx = gazeCanvas.getContext('2d', { willReadFrequently: true })
      if (!ctx) return null
      ctx.drawImage(video, 0, 0, w, h)
      const img = ctx.getImageData(0, 0, w, h)
      const deviceFp = (await computeDeviceFingerprint()).substring(0, 16)
      if (!isCurrent()) return null
      // Raw RGBA body, size + identity in headers. The backend may drop
      // this frame for a newer one; a dropped frame is not a check.
      const resp = await tauriInvoke<ScoreGazeReply>(
        'sentinel_score_gaze',
        rgbaBody(img),
        frameHeaders(w, h, { user: userId, device: deviceFp }),
      )
      if (!isCurrent() || isDroppedGaze(resp)) return null
      const est = resp.estimate
      gazeTotalChecks++
      // Live mirror so the dev PiP shows the real session sampling rate
      // + latest read (distinct from the PiP's own preview loop).
      sentinelDebug.sessionGazeChecks++
      sentinelDebug.lastGazeAt = Date.now()
      sentinelDebug.sessionGazeYaw = est.yaw
      sentinelDebug.sessionGazePitch = est.pitch
      sentinelDebug.sessionGazeOnScreen = est.onScreen
      sentinelDebug.sessionGazeOccluded = est.occluded
      if (resp.liveness) {
        livenessChecks++
        livenessRealProbSum += resp.liveness.real_prob
        if (resp.liveness.spoof_suspected) livenessSpoofChecks++
        sentinelDebug.sessionLivenessRealProb = resp.liveness.real_prob
        sentinelDebug.sessionLivenessSpoof = resp.liveness.spoof_suspected
      }
      if (est.occluded) {
        gazeOccludedChecks++
      } else if (!est.onScreen) {
        gazeOffscreenChecks++
        // Down-glance heuristic: calibrated → predicted point below the
        // screen; uncalibrated → positive pitch proxy (head tilted down).
        const lookingDown =
          est.screenY != null ? est.screenY > 1.0 : est.pitch > 0.12
        if (lookingDown) {
          gazeDownGlances++
          responseTracker.cue('gaze_down')
        }
      }
      return est
    } catch (err) {
      console.warn('[sentinel] gaze score IPC failed', err)
      return null
    }
  }

  // Draw a downscaled frame and extract gaze features (head-pose +
  // iris) for the highest-confidence face. Used by the wizard's
  // 9-point calibration capture. Returns null if no usable face.
  const extractGazeFeatures = async (
    video: HTMLVideoElement,
    signal?: AbortSignal,
  ): Promise<GazeFeatures | null> => {
    if (!video || video.readyState < 2) return null
    const requireCurrent = profileStateGuard(signal)
    try {
      requireCurrent()
      const { width: w, height: h } = downscaledSize({
        width: video.videoWidth || 640,
        height: video.videoHeight || 480,
      })
      if (!gazeCanvas) gazeCanvas = document.createElement('canvas')
      gazeCanvas.width = w
      gazeCanvas.height = h
      const ctx = gazeCanvas.getContext('2d', { willReadFrequently: true })
      if (!ctx) return null
      ctx.drawImage(video, 0, 0, w, h)
      const img = ctx.getImageData(0, 0, w, h)
      const features = await tauriInvoke<GazeFeatures | null>(
        'sentinel_extract_gaze_features',
        rgbaBody(img),
        frameHeaders(w, h),
      )
      requireCurrent()
      return features
    } catch (err) {
      console.warn('[sentinel] extract gaze features IPC failed', err)
      return null
    }
  }

  // Fit the per-user gaze calibration MLP from collected samples.
  const trainGazeCalibration = async (
    samples: GazeCalibSample[],
    signal?: AbortSignal,
  ): Promise<TrainGazeCalibResponse | null> => {
    const userId = stakeAddress.value
    if (!userId || samples.length === 0) return null
    const requireCurrent = profileStateGuard(signal)
    try {
      const deviceFp = (await computeDeviceFingerprint()).substring(0, 16)
      requireCurrent()
      const resp = await tauriInvoke<TrainGazeCalibResponse>('sentinel_train_gaze_calib', {
        req: { user_address: userId, device_fp_prefix: deviceFp, samples },
      })
      requireCurrent()
      await refreshUserModelsStatus(signal)
      requireCurrent()
      return resp
    } catch (err) {
      console.warn('[sentinel] train gaze calibration IPC failed', err)
      return null
    }
  }

  const detachMonitoringListeners = () => {
    if (snapshotTimer) { clearTimeout(snapshotTimer); snapshotTimer = null }
    trainingKeystrokesCleanup?.()
    trainingMouseCleanup?.()

    document.removeEventListener('keydown', onKeyDown)
    document.removeEventListener('keyup', onKeyUp)
    document.removeEventListener('mousemove', onMouseMove)
    document.removeEventListener('click', onMouseClick)
    document.removeEventListener('visibilitychange', onVisibilityChange)
    document.removeEventListener('paste', onPaste)
    if (unlistenFocus) { unlistenFocus(); unlistenFocus = null }
    if (unlistenDisplay) { unlistenDisplay(); unlistenDisplay = null }
    if (hotkeysRunning) {
      hotkeysRunning = false
      void tauriInvoke('sentinel_hotkeys_stop').catch(() => undefined)
    }
    if (displayResampleTimer) { clearTimeout(displayResampleTimer); displayResampleTimer = null }
    if (liveTimer) { clearInterval(liveTimer); liveTimer = null }
  }

  const stopSession = async () => {
    if (!sessionId.value) return

    if (snapshotTimer) { clearTimeout(snapshotTimer); snapshotTimer = null }
    await snapshotWork
    if (snapshotTimer) { clearTimeout(snapshotTimer); snapshotTimer = null }
    // A suspended session refuses snapshots, so a final one would only fail
    // and leave the attempt unable to finish.
    if (isActive.value && !suspended.value) await captureSnapshot(true)
    isActive.value = false
    sentinelDebug.active = false
    detachMonitoringListeners()
    void setAssessmentShield(false)

    const deviceFp = await computeDeviceFingerprint()
    await updateProfile(deviceFp)

    try {
      const ended = await invoke<{ id: string; status: string }>('integrity_end_session', {
        sessionId: sessionId.value,
      })
      // A session in review or flagged is the only case where evidence was
      // staged, and the only case where the learner has anything to decide
      // about. Surfacing it here rather than silently is the point: until
      // this shipped, Sentinel could flag a session and the person it flagged
      // was never told. Interview-purpose sessions never stage evidence, so
      // there is nothing to keep and nothing to ask.
      if ((ended?.status === 'review' || ended?.status === 'flagged') && sessionPurpose === 'assessment') {
        const snaps = await invoke<Array<{ anomaly_flags: string[] }>>(
          'integrity_list_snapshots',
          { sessionId: ended.id },
        ).catch(() => [])
        const reasons = [...new Set(snaps.flatMap((s) => s.anomaly_flags ?? []))]
        pendingEvidenceConsent.value = { sessionId: ended.id, reasons }
      }
    } catch (error) {
      console.warn('Sentinel: failed to end session')
      throw error
    }

    const currentSessionId = sessionId.value
    sessionId.value = null
    trustProfile.value = null
    cameraOptedIn.value = false
    sentinelDebug.cameraOptedIn = false
    keystrokeBuffer = []
    mouseBuffer = []
    tabSwitchCount = 0
    totalUnfocusedMs = 0
    pasteEventCount = 0
    pastedCharCount = 0
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
    gazeTotalChecks = 0
    gazeOffscreenChecks = 0
    gazeOccludedChecks = 0
    gazeDownGlances = 0
    livenessChecks = 0
    livenessSpoofChecks = 0
    livenessRealProbSum = 0

    return currentSessionId
  }

  /**
   * Ask the backend to choose and lock the session's trust profile. A camera
   * stream the learner turned on is both the capability and the consent;
   * so is a phone the learner paired.
   * Null when the choice failed: the session then has no profile, and a
   * role floor fails closed at attempt start.
   */
  const selectProfile = async (
    id: string,
    camera: boolean,
    options: SentinelStartOptions,
    companionPhone = false,
  ): Promise<SelectedProfile | null> => {
    const capabilities = { camera, microphone: false, companion_phone: companionPhone }
    const request: ProfileRequest = {
      capabilities,
      consent: capabilities,
      role_assessment_id: options.roleAssessmentId ?? null,
    }
    try {
      return (await invoke<SelectedProfile | null>('integrity_select_profile', { sessionId: id, request })) ?? null
    } catch (err) {
      console.warn('[sentinel] trust profile selection failed', err)
      return null
    }
  }

  const start = (
    enrollmentId: string | null,
    optInCamera = false,
    purpose: SentinelSessionPurpose = 'assessment',
    options: SentinelStartOptions = {},
  ) => {
    const generation = lifecycleGeneration
    return serializeTransition(() => startSession(enrollmentId, optInCamera, purpose, generation, options))
  }

  const stop = () => {
    lifecycleGeneration++
    // A start still waiting for a paired phone must not hold the stop.
    void useCompanionPhone().skip().catch((err: unknown) => console.warn('[sentinel] companion cancel failed', err))
    return serializeTransition(stopSession)
  }

  const stopForProfileLock = () => {
    // Invalidate optional work immediately, but retain finalization evidence
    // until the serialized backend close succeeds (including cleanup retries).
    profileStateGeneration++
    lifecycleGeneration++
    cameraGeneration++
    return serializeTransition(async () => {
      await stopSession()
      detachMonitoringListeners()
      isActive.value = false
      sessionId.value = null
      profile = null
      faceEmbedder = null
      gazeCanvas = null
      currentElementId = ''
      currentElementType = ''
      keystrokeBuffer = []
      mouseBuffer = []
      lastKeystrokeTime = 0
      lastBlurTime = 0
      snapshotWindowStartMs = 0
      tabSwitchCount = 0
      totalUnfocusedMs = 0
      pasteEventCount = 0
      pastedCharCount = 0
      environmentChanged = false
      appFocusLostCount = 0
      appFocusLostMs = 0
      focusLostAt = 0
      lastFocusApp = ''
      displayBaseline = null
      displayLatest = null
      displayChangeCount = 0
      displayNativeTransitions = -1
      displayNativeToggle = false
      androidEnvLatest = null
      obscuredTouchesWindow = 0
      overlayLatest = null
      processLatest = null
      hotkeyStatus = null
      webviewCombos = []
      blurIntervals = []
      phantomWindow = []
      nativeHotkeyCount = 0
      blurStartedAt = 0
      cameraDeviceLabel = ''
      cameraDeviceVirtual = false
      facePresent = undefined
      faceCount = undefined
      faceConsistency = undefined
      faceSimilarity = undefined
      faceMatch = undefined
      consecutiveNoFaceChecks = 0
      totalFaceChecks = 0
      faceAbsentChecks = 0
      gazeTotalChecks = 0
      gazeOffscreenChecks = 0
      gazeOccludedChecks = 0
      gazeDownGlances = 0
      livenessChecks = 0
      livenessSpoofChecks = 0
      livenessRealProbSum = 0
      keystrokeAeStatus.value = null
      mouseCnnStatus.value = null
      cameraOptedIn.value = false
      integrityScore.value = 1
      consistencyScore.value = 1
      aiScoringEnabled.value = false
      pasteClassifierEnabled.value = true
      try {
        localStorage.removeItem(AI_SCORING_STORAGE_KEY)
        localStorage.removeItem(PASTE_CLASSIFIER_STORAGE_KEY)
      } catch { /* localStorage disabled */ }
      pendingEvidenceConsent.value = null
      suspended.value = false
      Object.assign(sentinelDebug, emptySentinelDebug())
    })
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
    features: collectFeatures(),
  })

  // =========================================================================
  // Training API (calibration wizard)
  // =========================================================================

  const startTrainingKeystrokes = () => {
    trainingKeystrokesCleanup?.()
    const generation = profileStateGeneration
    const token = getProfileSessionToken()
    const canRecord = () => token !== null && generation === profileStateGeneration
      && token === getProfileSessionToken() && !isActive.value
    const keyDown = (event: KeyboardEvent) => { if (canRecord()) recordKeyDown(event) }
    const keyUp = () => { if (canRecord()) recordKeyUp() }
    keystrokeBuffer = []
    lastKeystrokeTime = 0
    document.addEventListener('keydown', keyDown, { passive: true })
    document.addEventListener('keyup', keyUp, { passive: true })
    const cleanup = () => {
      if (trainingKeystrokesCleanup !== cleanup) return
      document.removeEventListener('keydown', keyDown)
      document.removeEventListener('keyup', keyUp)
      trainingKeystrokesCleanup = null
    }
    trainingKeystrokesCleanup = cleanup
    return cleanup
  }

  const startTrainingMouse = () => {
    trainingMouseCleanup?.()
    const generation = profileStateGeneration
    const token = getProfileSessionToken()
    const canRecord = () => token !== null && generation === profileStateGeneration
      && token === getProfileSessionToken() && !isActive.value
    const mouseMove = (event: MouseEvent) => { if (canRecord()) recordMouseMove(event) }
    const mouseClick = (event: MouseEvent) => { if (canRecord()) recordMouseClick(event) }
    mouseBuffer = []
    document.addEventListener('mousemove', mouseMove, { passive: true })
    document.addEventListener('click', mouseClick, { passive: true })
    const cleanup = () => {
      if (trainingMouseCleanup !== cleanup) return
      document.removeEventListener('mousemove', mouseMove)
      document.removeEventListener('click', mouseClick)
      trainingMouseCleanup = null
    }
    trainingMouseCleanup = cleanup
    return cleanup
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

  const saveTrainingProfile = async (signal?: AbortSignal) => {
    const requireCurrent = profileStateGuard(signal)
    const userId = stakeAddress.value
    if (!userId) return
    const deviceFp = await computeDeviceFingerprint()
    requireCurrent()
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
        requireCurrent()
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

    requireCurrent()
    if (moves.length >= 51) {
      try {
        const r = await tauriInvoke<TrainMouseCnnResponse>('sentinel_train_mouse_cnn', {
          req: {
            user_address: userId,
            device_fp_prefix: fpPrefix,
            points: moves.map(m => ({ x: m.x, y: m.y, t: m.t })),
          },
        })
        requireCurrent()
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

    requireCurrent()
    profile.lastUpdated = Date.now()
    await saveProfile(userId, deviceFp, profile)
  }

  /**
   * Score a labeled-samples blob against the current local classifier.
   *
   * Used by the holdout evaluator: adversarial-labeled samples should
   * score as strongly anomalous, and human-labeled samples should not.
   *
   * Returns null if the local classifier isn't trained yet (no signal
   * to compare against). Returns `meanScore` on [0,1]:
   *   - keystroke: average reconstruction-error anomaly score (higher
   *     = more anomalous; > 0.65 is the adversarial signal)
   *   - mouse: 1 - average human probability (higher = more bot-like;
   *     > 0.50 is the adversarial signal since the CNN is symmetric)
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
    const requireCurrent = profileStateGuard()
    const deviceFp = (await computeDeviceFingerprint()).substring(0, 16)
    requireCurrent()

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
          requireCurrent()
          if (s >= 0) scores.push(s)
        } catch { /* ignore single-window failure */ }
        requireCurrent()
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
        requireCurrent()
        if (humanProb >= 0) botScores.push(1 - humanProb)
      } catch { /* ignore single-trajectory failure */ }
      requireCurrent()
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

  const trainAIModels = async (signal?: AbortSignal): Promise<{
    keystrokeAE: { trained: boolean; loss: number; samples: number }
    mouseCNN: { trained: boolean; loss: number; samples: number }
    faceEmbedder: { enrolled: boolean; progress: number }
  }> => {
    const requireCurrent = profileStateGuard(signal)
    const userId = stakeAddress.value
    if (!userId) {
      return {
        keystrokeAE: { trained: false, loss: -1, samples: 0 },
        mouseCNN: { trained: false, loss: -1, samples: 0 },
        faceEmbedder: {
          enrolled: faceEmbedder?.isEnrolled ?? false,
          progress: faceEmbedder?.enrollmentProgress ?? 0,
        },
      }
    }
    const deviceFp = (await computeDeviceFingerprint()).substring(0, 16)
    requireCurrent()

    let aeLoss = -1
    let aeSamples = 0
    let aeTrained = false
    if (keystrokeBuffer.length >= 20) {
      try {
        const r = await tauriInvoke<TrainKeystrokeAeResponse>('sentinel_train_keystroke_ae', {
          req: {
            user_address: userId,
            device_fp_prefix: deviceFp,
            events: keystrokeBuffer.map(k => ({ key: k.key, dwellMs: k.dwellMs, flightMs: k.flightMs })),
          },
        })
        requireCurrent()
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

    requireCurrent()
    let cnnLoss = -1
    let cnnSamples = 0
    let cnnTrained = false
    const moves = mouseBuffer.filter(m => m.type === 'move')
    if (moves.length >= 51) {
      try {
        const r = await tauriInvoke<TrainMouseCnnResponse>('sentinel_train_mouse_cnn', {
          req: {
            user_address: userId,
            device_fp_prefix: deviceFp,
            points: moves.map(m => ({ x: m.x, y: m.y, t: m.t })),
          },
        })
        requireCurrent()
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

    requireCurrent()
    return {
      keystrokeAE: { trained: aeTrained, loss: aeLoss, samples: aeSamples },
      mouseCNN: { trained: cnnTrained, loss: cnnLoss, samples: cnnSamples },
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

  const hydrateBehavioralProfile = async (signal?: AbortSignal): Promise<void> => {
    const requireCurrent = profileStateGuard(signal)
    const userId = stakeAddress.value
    if (!userId) return
    const deviceFp = await computeDeviceFingerprint()
    requireCurrent()
    const loaded = await loadProfile(userId, deviceFp)
    requireCurrent()
    faceEmbedder = null
    profile = loaded
    if (loaded) loadAIModels(loaded)
  }

  const resetProfile = async (signal?: AbortSignal) => {
    const requireCurrent = profileStateGuard(signal)
    const userId = stakeAddress.value
    if (!userId) return
    const deviceFp = await computeDeviceFingerprint()
    requireCurrent()
    await tauriInvoke('sentinel_reset_user_models', {
      userAddress: userId,
      deviceFpPrefix: deviceFp.substring(0, 16),
    })
    requireCurrent()
    try { localStorage.removeItem(legacyProfileKey(userId, deviceFp)) } catch { /* ignore */ }
    profile = null
    faceEmbedder = null
    keystrokeAeStatus.value = null
    mouseCnnStatus.value = null
  }

  const setAIScoringEnabled = (enabled: boolean) => {
    const requireCurrent = profileStateGuard()
    aiScoringEnabled.value = enabled
    // Persist to per-profile settings (scope=sync) so the toggle
    // propagates to the user's other devices.
    void (async () => {
      const { useSettings } = await import('./useSettings')
      requireCurrent()
      useSettings()
        .setSetting('sentinel.ai_scoring_enabled', enabled ? 'true' : 'false')
        .catch(() => { /* no profile yet */ })
    })().catch(() => { /* profile changed while loading settings */ })
  }

  const setPasteClassifierEnabled = (enabled: boolean) => {
    const requireCurrent = profileStateGuard()
    pasteClassifierEnabled.value = enabled
    void (async () => {
      const { useSettings } = await import('./useSettings')
      requireCurrent()
      useSettings()
        .setSetting('sentinel.paste_classifier_enabled', enabled ? 'true' : 'false')
        .catch(() => { /* no profile yet */ })
    })().catch(() => { /* profile changed while loading settings */ })
  }

  /** Toggle camera opt-in mid-session. Caller is responsible for acquiring
   * the MediaStream, attaching an HTMLVideoElement, and driving the 3s
   * face-verification loop (see docs/sentinel.md §Camera). This only flips
   * the flag that gates the window's camera features. */
  /**
   * Record which camera device feeds the opted-in stream (the video
   * track's label). A known virtual-camera product raises `virtual_camera`
   * each window the camera stays opted in. `null` clears it.
   */
  const reportCameraDevice = (label: string | null) => {
    cameraDeviceLabel = label ?? ''
    cameraDeviceVirtual = isVirtualCameraLabel(label)
    sentinelDebug.cameraDeviceLabel = cameraDeviceLabel
    sentinelDebug.cameraDeviceVirtual = cameraDeviceVirtual
  }

  const setCameraOptedIn = (opted: boolean) => {
    cameraGeneration++
    cameraOptedIn.value = opted
    if (!opted) {
      reportCameraDevice(null)
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
    suspended: readonly(suspended),
    integrityScore: readonly(integrityScore),
    consistencyScore: readonly(consistencyScore),
    cameraOptedIn: readonly(cameraOptedIn),
    trustProfile: readonly(trustProfile),
    aiScoringEnabled: readonly(aiScoringEnabled),
    pasteClassifierEnabled: readonly(pasteClassifierEnabled),
    keystrokeAeStatus: readonly(keystrokeAeStatus),
    mouseCnnStatus: readonly(mouseCnnStatus),
    loadedClassifierInfo: readonly(loadedClassifierInfo),

    // Session controls
    start,
    stop,
    /** Response-process events: item shown, response event, cross-modal cue. */
    responses: {
      present: responseTracker.present,
      input: responseTracker.input,
      cue: responseTracker.cue,
    },
    stopForProfileLock,
    setElement,
    isAssessmentElement,
    reportFaceDetection,
    verifyFace,
    scoreGaze,
    extractGazeFeatures,
    computeDeviceFingerprint,
    trainGazeCalibration,
    debug: readonly(sentinelDebug),
    getDebugState,
    getFinalScore,
    getSessionId,
    pendingEvidenceConsent,
    clearPendingEvidenceConsent: () => {
      pendingEvidenceConsent.value = null
    },

    // Training API
    startTrainingKeystrokes,
    startTrainingMouse,
    getTrainingMetrics,
    clearTrainingBuffers,
    getProfile,
    hydrateBehavioralProfile,
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
    reportCameraDevice,
    testBlobAgainstClassifier,
  }
}
