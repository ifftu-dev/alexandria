import { webcrypto } from 'node:crypto'
import { flushPromises } from '@vue/test-utils'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const mocks = vi.hoisted(() => ({
  invoke: vi.fn<(command: string, args?: Record<string, unknown>, options?: { headers: Record<string, string> }) => Promise<unknown>>(),
  unlisten: vi.fn(),
  active: '',
  failEnd: false,
  settingsInitialize: vi.fn<() => Promise<void>>(),
  settingsEntries: [] as Array<{ key: string; current_value: string; is_default: boolean }>,
  behavioralProfiles: new Map<string, unknown>(),
}))

vi.mock('@tauri-apps/api/core', () => ({ invoke: mocks.invoke }))
vi.mock('@tauri-apps/api/event', () => ({ listen: async () => mocks.unlisten }))
vi.mock('../useSettings', () => ({
  useSettings: () => ({ entries: { value: mocks.settingsEntries }, initialize: mocks.settingsInitialize }),
}))
vi.mock('@/utils/sentinel/face-embedder', () => ({
  FaceEmbedder: class {
    isEnrolled = true
    enrollmentProgress = 1
    enroll() { return true }
    exportEnrollment() {
      return { vector: Array.from({ length: 944 }, () => 0.01), frameCount: 5, updatedAt: 1 }
    }
  },
}))

function deferred<T>() {
  let resolve!: (value: T) => void
  const promise = new Promise<T>(res => { resolve = res })
  return { promise, resolve }
}

async function services() {
  const { useProfiles, onProfileLocked } = await import('../useProfiles')
  const { useSentinel } = await import('../useSentinel')
  const sentinel = useSentinel()
  onProfileLocked(() => sentinel.stopForProfileLock())
  return { profiles: useProfiles(), sentinel }
}

function status(epochs: number) {
  return [{ model_kind: 'keystroke_ae', trained_epochs: epochs, training_samples: 20, train_loss: 0.1, updated_at: '' }]
}

beforeEach(() => {
  vi.resetModules()
  vi.useFakeTimers()
  vi.stubGlobal('crypto', webcrypto)
  vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockReturnValue(null)
  vi.spyOn(console, 'warn').mockImplementation(() => undefined)
  localStorage.clear()
  mocks.active = ''
  mocks.failEnd = false
  mocks.unlisten.mockReset()
  mocks.settingsInitialize.mockReset().mockResolvedValue()
  mocks.settingsEntries = []
  mocks.behavioralProfiles.clear()
  mocks.invoke.mockReset().mockImplementation(async (command, args) => {
    switch (command) {
      case 'unlock_profile':
        mocks.active = String(args?.id)
        return { wallet: {}, profile: { stake_address: `wallet-${mocks.active}` } }
      case 'get_profile_session_token': return `token-${mocks.active}`
      case 'list_profiles': return [{ id: 'A' }, { id: 'B' }]
      case 'get_profile_cleanup_required': return false
      case 'integrity_start_session': return { session_id: `session-${mocks.active}` }
      case 'integrity_end_session':
        if (mocks.failEnd) throw new Error('end unavailable')
        return { id: `session-${mocks.active}`, status: 'flagged' }
      case 'integrity_list_snapshots': return [{ anomaly_flags: ['paste_detected'] }]
      case 'sentinel_user_models_status': return status(mocks.active === 'A' ? 3 : 7)
      case 'sentinel_load_behavioral_profile':
        return mocks.behavioralProfiles.get(`${String(args?.userAddress)}:${String(args?.deviceFpPrefix)}`) ?? null
      case 'sentinel_save_behavioral_profile': {
        const profile = args?.profile as { userId: string; deviceFingerprint: string }
        mocks.behavioralProfiles.set(
          `${profile.userId}:${profile.deviceFingerprint.substring(0, 16)}`,
          structuredClone(profile),
        )
        return null
      }
      case 'sentinel_paste_classifier_info': return { source: 'bundled', version: 'v1' }
      case 'sentinel_score_paste': return { score: -1, classifier: null }
      case 'lock_profile': mocks.active = ''; return null
      default: return null
    }
  })
})

afterEach(async () => {
  await flushPromises()
  vi.clearAllTimers()
  vi.useRealTimers()
  vi.restoreAllMocks()
  vi.unstubAllGlobals()
})

describe('Sentinel profile-lock boundary', () => {
  it('captures calibration without creating an assessment session and stops on cleanup', async () => {
    const { profiles, sentinel } = await services()
    await profiles.unlockProfile('A', 'password')
    const keyboardCleanup = sentinel.startTrainingKeystrokes()
    const mouseCleanup = sentinel.startTrainingMouse()
    const record = () => {
      document.dispatchEvent(new KeyboardEvent('keydown', { key: 'a' }))
      vi.advanceTimersByTime(60)
      document.dispatchEvent(new KeyboardEvent('keyup', { key: 'a' }))
      document.dispatchEvent(new MouseEvent('mousemove', { clientX: 1, clientY: 1 }))
      document.dispatchEvent(new MouseEvent('click'))
    }
    record()
    expect(sentinel.getTrainingMetrics()).toMatchObject({
      keystrokeCount: 1, mouseMoveCount: 1, mouseClickCount: 1,
    })
    expect(sentinel.isActive.value).toBe(false)
    expect(sentinel.sessionId.value).toBeNull()
    expect(mocks.invoke.mock.calls.map(([command]) => command)).not.toContain('integrity_start_session')
    keyboardCleanup(); mouseCleanup()
    record()
    expect(sentinel.getTrainingMetrics()).toMatchObject({
      keystrokeCount: 1, mouseMoveCount: 1, mouseClickCount: 1,
    })
    await profiles.lockProfile()
  })

  it('keeps replacement calibration listeners owned and avoids duplicate assessment events', async () => {
    const { profiles, sentinel } = await services()
    await profiles.unlockProfile('A', 'password')
    const oldCleanup = sentinel.startTrainingKeystrokes()
    const newCleanup = sentinel.startTrainingKeystrokes()
    oldCleanup()
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'a' }))
    expect(sentinel.getTrainingMetrics().keystrokeCount).toBe(1)
    await sentinel.start('enrollment')
    const before = sentinel.getTrainingMetrics().keystrokeCount
    expect(before).toBe(0)
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'b' }))
    expect(sentinel.getTrainingMetrics().keystrokeCount).toBe(before + 1)
    newCleanup()
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'c' }))
    expect(sentinel.getTrainingMetrics().keystrokeCount).toBe(before + 2)
    await profiles.lockProfile()
  })

  it.each(['train', 'save', 'gaze'] as const)('cancels %s follow-up work without requiring a profile lock', async action => {
    const { profiles, sentinel } = await services()
    await profiles.unlockProfile('A', 'password')
    const keyboardCleanup = sentinel.startTrainingKeystrokes()
    const mouseCleanup = sentinel.startTrainingMouse()
    for (let i = 0; i < 20; i++) document.dispatchEvent(new KeyboardEvent('keydown', { key: 'a' }))
    for (let i = 0; i < 51; i++) {
      vi.advanceTimersByTime(60)
      document.dispatchEvent(new MouseEvent('mousemove', { clientX: i, clientY: i }))
    }
    const response = deferred<unknown>()
    const requested = deferred<void>()
    const commandToPause = {
      train: 'sentinel_train_keystroke_ae', save: 'sentinel_train_keystroke_ae', gaze: 'sentinel_train_gaze_calib',
    }[action]
    const fallback = mocks.invoke.getMockImplementation()!
    mocks.invoke.mockImplementation(async (command, args, options) => {
      if (command === commandToPause) { requested.resolve(); return response.promise }
      return fallback(command, args, options)
    })
    const controller = new AbortController()
    const training = action === 'train' ? sentinel.trainAIModels(controller.signal)
      : action === 'save' ? sentinel.saveTrainingProfile(controller.signal)
        : sentinel.trainGazeCalibration([
            { yaw: 0, pitch: 0, roll: 0, irisDx: 0, irisDy: 0, targetX: 0.5, targetY: 0.5 },
          ], controller.signal)
    const settled = action === 'gaze' ? expect(training).resolves.toBeNull()
      : expect(training).rejects.toThrow('Sentinel operation was cancelled')
    await requested.promise
    controller.abort()
    response.resolve({ trained_epochs: 99, training_samples: 20, train_loss: 0.1 })
    await settled
    const commands = mocks.invoke.mock.calls.map(([command]) => command)
    expect(commands).not.toContain('sentinel_train_mouse_cnn')
    expect(commands).not.toContain('sentinel_user_models_status')
    expect(sentinel.getAIModelStatus().keystrokeAE).toBeNull()
    expect(profiles.isUnlocked.value).toBe(true)
    keyboardCleanup(); mouseCleanup()
    await profiles.lockProfile()
  })

  it('finalizes with the original token, clears private memory, and preserves encrypted calibration', async () => {
    localStorage.setItem('sentinel_ai_scoring_enabled', '1')
    localStorage.setItem('sentinel_paste_classifier_enabled', '0')
    const { profiles, sentinel } = await services()
    await profiles.unlockProfile('A', 'password')
    await sentinel.start('private-enrollment', true)
    sentinel.enrollFace(document.createElement('video'))
    await sentinel.saveTrainingProfile()
    await sentinel.refreshUserModelsStatus()
    sentinel.setElement('private-element', 'quiz')
    sentinel.reportFaceDetection(false, 0, 0.2)
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'a' }))
    expect(sentinel.getProfile()?.userId).toBe('wallet-A')
    expect(sentinel.getAIModelStatus().faceEmbedder?.enrolled).toBe(true)
    expect(sentinel.getDebugState().keystrokeBufferSize).toBe(1)
    const savedCall = mocks.invoke.mock.calls.find(([command]) => command === 'sentinel_save_behavioral_profile')
    expect(savedCall?.[1]?.profile).toMatchObject({
      userId: 'wallet-A',
      aiModels: { faceEnrollment: { frameCount: 5 } },
    })
    expect([...mocks.behavioralProfiles.values()]).toHaveLength(1)
    expect(Object.keys(localStorage).some(key => key.startsWith('sentinel_profile_'))).toBe(false)

    await profiles.lockProfile()
    expect(profiles.isUnlocked.value).toBe(false)
    expect(profiles.lockState.value).toBe('idle')
    const commands = mocks.invoke.mock.calls.map(([command]) => command)
    expect(commands.indexOf('integrity_end_session')).toBeLessThan(commands.indexOf('lock_profile'))
    expect(mocks.invoke).toHaveBeenCalledWith('integrity_end_session', expect.objectContaining({ sessionId: 'session-A' }), {
      headers: { 'x-alexandria-profile-session': 'token-A' },
    })
    expect(sentinel.getProfile()).toBeNull()
    expect(sentinel.getAIModelStatus()).toEqual({ keystrokeAE: null, mouseCNN: null, faceEmbedder: null })
    expect(sentinel.getDebugState()).toMatchObject({
      currentElementId: '', currentElementType: '', keystrokeBufferSize: 0, mouseBufferSize: 0,
      profile: null, facePresent: undefined, hasSnapshotTimer: false,
    })
    expect(sentinel.debug).toMatchObject({ active: false, signals: null, flags: [], lastApp: '', lastSnapshotAt: 0 })
    expect(sentinel.pendingEvidenceConsent.value).toBeNull()
    expect(sentinel.aiScoringEnabled.value).toBe(false)
    expect(sentinel.pasteClassifierEnabled.value).toBe(true)
    expect(localStorage.getItem('sentinel_ai_scoring_enabled')).toBeNull()
    expect(localStorage.getItem('sentinel_paste_classifier_enabled')).toBeNull()
    expect(sentinel.integrityScore.value).toBe(1)
    expect([...mocks.behavioralProfiles.values()]).toHaveLength(1)
    expect(commands).not.toContain('sentinel_reset_user_models')
    await vi.advanceTimersByTimeAsync(0)
    expect(vi.getTimerCount()).toBe(0)
    await profiles.unlockProfile('B', 'password')
    expect(sentinel.getProfile()).toBeNull()
    expect(sentinel.getAIModelStatus().faceEmbedder).toBeNull()
    await profiles.lockProfile()
  })

  it('erases a pre-SQLCipher profile key without adopting it', async () => {
    const { profiles, sentinel } = await services()
    await profiles.unlockProfile('A', 'password')
    await sentinel.start('enrollment')
    await sentinel.stop()
    const baseline = structuredClone([...mocks.behavioralProfiles.values()][0]) as {
      userId: string
      deviceFingerprint: string
      aiModels?: Record<string, unknown>
    }
    await profiles.lockProfile()

    const key = `sentinel_profile_${baseline.userId}_${baseline.deviceFingerprint.substring(0, 16)}`
    baseline.aiModels = {
      faceEnrollment: { vector: Array.from({ length: 944 }, () => 0.01), frameCount: 5, updatedAt: 1 },
    }
    localStorage.setItem(key, JSON.stringify(baseline))
    mocks.behavioralProfiles.clear()
    mocks.invoke.mockClear()

    await profiles.unlockProfile('A', 'password')
    await sentinel.start('enrollment')
    const imported = mocks.invoke.mock.calls
      .filter(([command]) => command === 'sentinel_save_behavioral_profile')
      .some(([, args]) => (args?.profile as { aiModels?: Record<string, unknown> })?.aiModels
        ?.faceEnrollment !== undefined)
    expect(imported).toBe(false)
    expect(localStorage.getItem(key)).toBeNull()
    await profiles.lockProfile()
  })

  it('does not resurrect a pre-SQLCipher profile when encrypted storage fails', async () => {
    const { profiles, sentinel } = await services()
    await profiles.unlockProfile('A', 'password')
    await sentinel.start('enrollment')
    await sentinel.stop()
    const baseline = structuredClone([...mocks.behavioralProfiles.values()][0]) as {
      userId: string
      deviceFingerprint: string
    }
    await profiles.lockProfile()

    const key = `sentinel_profile_${baseline.userId}_${baseline.deviceFingerprint.substring(0, 16)}`
    localStorage.setItem(key, JSON.stringify(baseline))
    mocks.behavioralProfiles.clear()
    const fallback = mocks.invoke.getMockImplementation()!
    mocks.invoke.mockImplementation(async (command, args, options) => {
      if (command === 'sentinel_load_behavioral_profile') throw new Error('encrypted storage unavailable')
      return fallback(command, args, options)
    })

    await profiles.unlockProfile('A', 'password')
    await expect(sentinel.start('enrollment')).rejects.toThrow('encrypted storage unavailable')
    expect(mocks.behavioralProfiles).toHaveLength(0)
    await profiles.lockProfile()
  })

  it('rejects a late encrypted profile load after locking and switching profiles', async () => {
    const { profiles, sentinel } = await services()
    await profiles.unlockProfile('A', 'password')
    const response = deferred<unknown>()
    const requested = deferred<void>()
    const fallback = mocks.invoke.getMockImplementation()!
    mocks.invoke.mockImplementation(async (command, args, options) => {
      if (command === 'sentinel_load_behavioral_profile' && mocks.active === 'A') {
        requested.resolve()
        return response.promise
      }
      return fallback(command, args, options)
    })

    const hydration = sentinel.hydrateBehavioralProfile()
    await requested.promise
    await profiles.lockProfile()
    await profiles.unlockProfile('B', 'password')
    response.resolve({ userId: 'wallet-A', deviceFingerprint: 'a'.repeat(64) })
    await expect(hydration).rejects.toThrow(/cancelled by locking|Profile session changed/)
    expect(sentinel.getProfile()).toBeNull()
    await profiles.lockProfile()
  })

  it('retains finalization evidence and blocks unlock on failure, then clears it on retry', async () => {
    const { profiles, sentinel } = await services()
    await profiles.unlockProfile('A', 'password')
    await sentinel.start('enrollment', true)
    sentinel.setElement('private-element', 'quiz')
    sentinel.reportFaceDetection(false, 0, 0.2)
    mocks.failEnd = true
    await expect(profiles.lockProfile()).rejects.toThrow('Profile cleanup did not finish')
    expect(profiles.isUnlocked.value).toBe(false)
    expect(profiles.lockState.value).toBe('failed')
    expect(sentinel.sessionId.value).toBe('session-A')
    expect(sentinel.getDebugState().facePresent).toBe(false)
    expect(sentinel.cameraOptedIn.value).toBe(true)
    expect(mocks.invoke.mock.calls.some(([command]) => command === 'lock_profile')).toBe(false)
    await expect(profiles.unlockProfile('B', 'password')).rejects.toThrow('Finish locking')
    mocks.failEnd = false
    await profiles.lockProfile()
    expect(sentinel.sessionId.value).toBeNull()
    expect(sentinel.getDebugState().facePresent).toBeUndefined()
    expect(sentinel.cameraOptedIn.value).toBe(false)
  })

  it('rejects an old model-status response after another profile has hydrated', async () => {
    const { profiles, sentinel } = await services()
    await profiles.unlockProfile('A', 'password')
    const response = deferred<unknown>()
    const requested = deferred<void>()
    const fallback = mocks.invoke.getMockImplementation()!
    mocks.invoke.mockImplementation(async (command, args, options) => {
      if (command === 'sentinel_user_models_status' && mocks.active === 'A') {
        requested.resolve(); return response.promise
      }
      return fallback(command, args, options)
    })
    const oldRefresh = sentinel.refreshUserModelsStatus()
    await requested.promise
    await profiles.lockProfile()
    await profiles.unlockProfile('B', 'password')
    await sentinel.refreshUserModelsStatus()
    response.resolve(status(99))
    await oldRefresh
    expect(sentinel.getAIModelStatus().keystrokeAE?.epochs).toBe(7)
    await profiles.lockProfile()
  })

  it('does not issue an old request with a new token after fingerprinting finishes late', async () => {
    const { profiles, sentinel } = await services()
    await profiles.unlockProfile('A', 'password')
    const fingerprint = deferred<ArrayBuffer>()
    vi.spyOn(crypto.subtle, 'digest').mockReturnValueOnce(fingerprint.promise)
    const oldRefresh = sentinel.refreshUserModelsStatus()
    await profiles.lockProfile()
    await profiles.unlockProfile('B', 'password')
    fingerprint.resolve(new ArrayBuffer(32))
    await oldRefresh
    expect(mocks.invoke.mock.calls.some(([command]) => command === 'sentinel_user_models_status')).toBe(false)
    await profiles.lockProfile()
  })

  it('does not return old calibration results or refresh models for the next profile', async () => {
    const { profiles, sentinel } = await services()
    await profiles.unlockProfile('A', 'password')
    const response = deferred<unknown>()
    const requested = deferred<void>()
    const fallback = mocks.invoke.getMockImplementation()!
    mocks.invoke.mockImplementation(async (command, args, options) => {
      if (command === 'sentinel_train_gaze_calib') { requested.resolve(); return response.promise }
      return fallback(command, args, options)
    })
    const training = sentinel.trainGazeCalibration([
      { yaw: 0, pitch: 0, roll: 0, irisDx: 0, irisDy: 0, targetX: 0.5, targetY: 0.5 },
    ])
    await requested.promise
    await profiles.lockProfile()
    await profiles.unlockProfile('B', 'password')
    response.resolve({ training_samples: 1 })
    expect(await training).toBeNull()
    expect(mocks.invoke.mock.calls.some(([command]) => command === 'sentinel_user_models_status')).toBe(false)
    await profiles.lockProfile()
  })

  it('cancels late training projection and follow-up mouse training after locking', async () => {
    const { profiles, sentinel } = await services()
    await profiles.unlockProfile('A', 'password')
    await sentinel.start('enrollment')
    for (let i = 0; i < 20; i++) document.dispatchEvent(new KeyboardEvent('keydown', { key: 'a' }))
    for (let i = 0; i < 51; i++) {
      vi.advanceTimersByTime(60)
      document.dispatchEvent(new MouseEvent('mousemove', { clientX: i, clientY: i }))
    }
    const response = deferred<unknown>()
    const requested = deferred<void>()
    const fallback = mocks.invoke.getMockImplementation()!
    mocks.invoke.mockImplementation(async (command, args, options) => {
      if (command === 'sentinel_train_keystroke_ae') { requested.resolve(); return response.promise }
      return fallback(command, args, options)
    })
    const training = sentinel.saveTrainingProfile()
    const rejected = expect(training).rejects.toThrow('cancelled by locking')
    await requested.promise
    await profiles.lockProfile()
    await profiles.unlockProfile('B', 'password')
    await sentinel.saveTrainingProfile()
    response.resolve({ trained_epochs: 99, training_samples: 20, train_loss: 0.1 })
    await rejected
    expect(sentinel.getProfile()?.userId).toBe('wallet-B')
    expect(sentinel.getAIModelStatus().keystrokeAE).toBeNull()
    expect(mocks.invoke.mock.calls.some(([command]) => command === 'sentinel_train_mouse_cnn')).toBe(false)
    await profiles.lockProfile()
  })

  it('removes training listeners without a session and makes old cleanup handles harmless', async () => {
    const { profiles, sentinel } = await services()
    await profiles.unlockProfile('A', 'password')
    const removed = vi.spyOn(document, 'removeEventListener')
    const oldKeyboard = sentinel.startTrainingKeystrokes()
    const oldMouse = sentinel.startTrainingMouse()
    await profiles.lockProfile()
    expect(removed.mock.calls.map(([event]) => event)).toEqual(expect.arrayContaining(['keydown', 'keyup', 'mousemove', 'click']))
    await profiles.unlockProfile('B', 'password')
    const newKeyboard = sentinel.startTrainingKeystrokes()
    const newMouse = sentinel.startTrainingMouse()
    removed.mockClear()
    oldKeyboard(); oldMouse()
    expect(removed).not.toHaveBeenCalled()
    newKeyboard(); newMouse()
    expect(removed).toHaveBeenCalledTimes(4)
    await profiles.lockProfile()
  })

  it('does not hydrate old profile preferences after a different profile initializes', async () => {
    const { profiles, sentinel } = await services()
    const { initSentinelFlagsFromSettings } = await import('../useSentinel')
    await profiles.unlockProfile('A', 'password')
    mocks.settingsEntries = [{ key: 'sentinel.ai_scoring_enabled', current_value: 'true', is_default: false }]
    const pending = deferred<void>()
    const requested = deferred<void>()
    mocks.settingsInitialize.mockImplementationOnce(() => { requested.resolve(); return pending.promise })
    const oldHydration = initSentinelFlagsFromSettings()
    const rejected = expect(oldHydration).rejects.toThrow('cancelled by locking')
    await requested.promise
    await profiles.lockProfile()
    await profiles.unlockProfile('B', 'password')
    mocks.settingsEntries = []
    await initSentinelFlagsFromSettings()
    pending.resolve()
    await rejected
    expect(sentinel.aiScoringEnabled.value).toBe(false)
    expect(sentinel.pasteClassifierEnabled.value).toBe(true)
    await profiles.lockProfile()
  })

})
