import { webcrypto } from 'node:crypto'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { ref } from 'vue'

const mocks = vi.hoisted(() => ({
  invoke: vi.fn<(command: string, args?: Record<string, unknown>) => Promise<unknown>>(),
  listen: vi.fn(),
  unlisten: vi.fn(),
  stakeAddress: null as string | null,
}))

vi.mock('@tauri-apps/api/core', () => ({ invoke: mocks.invoke }))
vi.mock('@tauri-apps/api/event', () => ({ listen: mocks.listen }))
vi.mock('../useAuth', () => ({ useAuth: () => ({ stakeAddress: ref(mocks.stakeAddress) }) }))

async function freshService() {
  vi.resetModules()
  return (await import('../useSentinel')).useSentinel
}

beforeEach(() => {
  vi.useFakeTimers()
  vi.stubGlobal('crypto', webcrypto)
  vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockReturnValue(null)
  mocks.invoke.mockReset()
  mocks.stakeAddress = null
  mocks.unlisten.mockReset()
  mocks.listen.mockReset().mockResolvedValue(mocks.unlisten)
  mocks.invoke.mockImplementation(async (command) => {
    if (command === 'integrity_start_session') return { session_id: 'session-1' }
    if (command === 'integrity_end_session') return { id: 'session-1', status: 'completed' }
    if (command === 'sentinel_paste_classifier_info') return { source: 'bundled', version: 'v1' }
    if (command === 'sentinel_score_paste') return { score: -1, classifier: null }
    if (command === 'sentinel_user_models_status') return []
    return null
  })
})

afterEach(() => {
  vi.clearAllTimers()
  vi.useRealTimers()
  vi.restoreAllMocks()
  vi.unstubAllGlobals()
})

describe('Sentinel lifecycle ownership', () => {
  it('preserves camera evidence through failed teardown and clears opt-in only after success', async () => {
    const service = (await freshService())()
    await service.start('enrollment', true)
    service.reportFaceDetection(false, 0, 0.2)
    const fallback = mocks.invoke.getMockImplementation()!
    mocks.invoke.mockImplementation(async (command, args) => {
      if (command === 'integrity_end_session') throw new Error('end unavailable')
      return fallback(command, args)
    })
    vi.spyOn(console, 'warn').mockImplementation(() => undefined)
    await expect(service.stop()).rejects.toThrow('end unavailable')
    expect(service.cameraOptedIn.value).toBe(true)
    expect(service.getDebugState().facePresent).toBe(false)
    const failedRequest = mocks.invoke.mock.calls.find(([name]) => name === 'integrity_end_session')?.[1]
    mocks.invoke.mockImplementation(fallback)
    await service.stop()
    expect(mocks.invoke.mock.calls.filter(([name]) => name === 'integrity_end_session')[1]?.[1]).toEqual(failedRequest)
    expect(service.cameraOptedIn.value).toBe(false)
    expect(service.getDebugState().facePresent).toBeUndefined()
  })

  it('propagates startup failure without installing listeners and permits a later retry', async () => {
    const service = (await freshService())()
    const fallback = mocks.invoke.getMockImplementation()!
    mocks.invoke.mockImplementation(async (command, args) => {
      if (command === 'integrity_start_session') throw new Error('start unavailable')
      return fallback(command, args)
    })
    vi.spyOn(console, 'warn').mockImplementation(() => undefined)
    await expect(service.start(null)).rejects.toThrow('start unavailable')
    expect(service.isActive.value).toBe(false)
    expect(service.sessionId.value).toBeNull()
    expect(mocks.listen).not.toHaveBeenCalled()
    expect(vi.getTimerCount()).toBe(0)
    mocks.invoke.mockImplementation(fallback)
    await service.start(null)
    expect(service.isActive.value).toBe(true)
    await service.stop()
  })

  it('discards late gaze results after stopping, replacing a session, or toggling the camera', async () => {
    mocks.stakeAddress = 'gaze-test-user'
    vi.spyOn(document, 'hidden', 'get').mockReturnValue(false)
    const context = {
      drawImage: vi.fn(),
      getImageData: () => ({ data: new Uint8ClampedArray(4) }),
    }
    vi.mocked(HTMLCanvasElement.prototype.getContext).mockReturnValue(context as unknown as CanvasRenderingContext2D)
    const video = document.createElement('video')
    Object.defineProperties(video, {
      readyState: { value: 2 }, videoWidth: { value: 1 }, videoHeight: { value: 1 },
    })
    await vi.advanceTimersByTimeAsync(0)
    const service = (await freshService())()
    expect(await service.scoreGaze(video)).toBeNull()
    expect(context.drawImage).not.toHaveBeenCalled()
    const estimate = { yaw: 0, pitch: 0, screenX: 0.5, screenY: 0.5, onScreen: true, occluded: false, confidence: 1 }
    const fallback = mocks.invoke.getMockImplementation()!
    for (const transition of ['stop', 'replace', 'camera']) {
      let resolveGaze!: (value: unknown) => void
      let notifyGaze!: () => void
      const pendingGaze = new Promise<unknown>(resolve => { resolveGaze = resolve })
      const gazeStarted = new Promise<void>(resolve => { notifyGaze = resolve })
      mocks.invoke.mockImplementation(async (command, args) => {
        if (command === 'sentinel_score_gaze') { notifyGaze(); return pendingGaze }
        return fallback(command, args)
      })
      await service.start('original', true)
      const gaze = service.scoreGaze(video)
      await gazeStarted
      if (transition === 'camera') {
        service.setCameraOptedIn(false)
        service.setCameraOptedIn(true)
      } else {
        await service.stop()
        if (transition === 'replace') await service.start('replacement', true)
      }
      resolveGaze({ estimate, faceCount: 1 })
      expect(await gaze).toBeNull()
      expect(service.debug.sessionGazeChecks).toBe(0)
      if (transition !== 'stop') {
        expect(await service.scoreGaze(video)).toEqual(estimate)
        expect(service.debug.sessionGazeChecks).toBe(1)
        await service.stop()
      }
    }
    // jsdom queues a storage event when the behavioral profile is persisted.
    await vi.advanceTimersByTimeAsync(0)
    expect(vi.getTimerCount()).toBe(0)
  })

  it('window dimensions cannot affect snapshot or final scores or create misconduct flags', async () => {
    vi.spyOn(Math, 'random').mockReturnValue(0)
    const service = (await freshService())()
    await service.start('enrollment')
    await vi.advanceTimersByTimeAsync(15001)
    const baseline = service.integrityScore.value

    vi.stubGlobal('outerWidth', window.innerWidth + 800)
    vi.stubGlobal('outerHeight', window.innerHeight + 800)
    window.dispatchEvent(new Event('resize'))
    await vi.advanceTimersByTimeAsync(15001)
    expect(service.integrityScore.value).toBe(baseline)
    const snapshots = mocks.invoke.mock.calls.filter(([name]) => name === 'integrity_submit_snapshot')
    expect(snapshots).toHaveLength(2)
    for (const [, args] of snapshots) {
      expect(args?.req).toMatchObject({
        integrity_score: baseline,
        devtools_score: null,
        anomaly_flags: [],
      })
    }
    await service.stop()
    expect(mocks.invoke).toHaveBeenCalledWith('integrity_end_session', expect.objectContaining({
      req: expect.objectContaining({ overall_integrity_score: baseline }),
    }), { headers: {} })
    expect(vi.getTimerCount()).toBe(0)
  })

  it('retiring the resize heuristic preserves unrelated paste anomaly reporting', async () => {
    vi.spyOn(Math, 'random').mockReturnValue(0)
    const service = (await freshService())()
    await service.start('enrollment')
    const paste = new Event('paste')
    Object.defineProperty(paste, 'clipboardData', { value: { getData: () => 'x'.repeat(600) } })
    document.dispatchEvent(paste)
    window.dispatchEvent(new Event('resize'))
    await vi.advanceTimersByTimeAsync(15001)
    const snapshot = mocks.invoke.mock.calls.find(([name]) => name === 'integrity_submit_snapshot')
    expect(snapshot?.[1]?.req).toMatchObject({
      devtools_score: null,
      paste_score: 0.4,
      anomaly_flags: ['paste_detected'],
    })
    await service.stop()
  })

  it('another consumer removes the exact listeners installed by the starter', async () => {
    const useSentinel = await freshService()
    const added = vi.spyOn(document, 'addEventListener')
    const removed = vi.spyOn(document, 'removeEventListener')
    const starter = useSentinel()
    const stopper = useSentinel()
    await starter.start('enrollment')
    const listeners = added.mock.calls.filter(([type]) =>
      ['keydown', 'keyup', 'mousemove', 'click', 'visibilitychange', 'paste'].includes(type),
    )
    expect(listeners).toHaveLength(6)
    await stopper.stop()
    for (const [type, listener] of listeners) {
      expect(removed).toHaveBeenCalledWith(type, listener)
    }
    // One unlisten per native listener: sentinel://focus and sentinel://display.
    expect(mocks.unlisten).toHaveBeenCalledTimes(2)
    expect(vi.getTimerCount()).toBe(0)
    expect(starter.isActive.value).toBe(false)
  })

  it('concurrent starts create one backend session and one set of listeners', async () => {
    const useSentinel = await freshService()
    const service = useSentinel()
    await Promise.all([service.start('enrollment'), useSentinel().start('enrollment')])
    expect(mocks.invoke.mock.calls.filter(([name]) => name === 'integrity_start_session')).toHaveLength(1)
    // Exactly one set of native listeners (focus + display), not one per caller.
    expect(mocks.listen).toHaveBeenCalledTimes(2)
    expect(mocks.listen.mock.calls.map(([name]) => name).sort()).toEqual(['sentinel://display', 'sentinel://focus'])
    await service.stop()
  })

  it('stop during backend startup reclaims the session without activating listeners', async () => {
    let completeStart: ((value: unknown) => void) | undefined
    let notifyStarted: (() => void) | undefined
    const started = new Promise<void>(resolve => { notifyStarted = resolve })
    const response = new Promise<unknown>(resolve => { completeStart = resolve })
    mocks.invoke.mockImplementation(async command => {
      if (command === 'integrity_start_session') {
        notifyStarted?.()
        return response
      }
      return { id: 'late-session', status: 'completed' }
    })
    const useSentinel = await freshService()
    const service = useSentinel()
    const start = service.start('enrollment')
    await started
    const stop = useSentinel().stop()
    completeStart?.({ session_id: 'late-session' })
    await Promise.all([start, stop])
    expect(mocks.listen).not.toHaveBeenCalled()
    expect(mocks.invoke).toHaveBeenCalledWith('integrity_end_session', expect.objectContaining({
      sessionId: 'late-session',
    }), { headers: {} })
    expect(service.isActive.value).toBe(false)
    expect(service.sessionId.value).toBeNull()
    expect(vi.getTimerCount()).toBe(0)
  })

  it('a stop requested before startup executes cancels it entirely', async () => {
    const service = (await freshService())()
    await Promise.all([service.start('enrollment'), service.stop()])
    expect(mocks.invoke).not.toHaveBeenCalled()
    expect(vi.getTimerCount()).toBe(0)
  })

  it('failed backend teardown remains retryable and blocks replacement sessions', async () => {
    const service = (await freshService())()
    await service.start('enrollment')
    const fallback = mocks.invoke.getMockImplementation()!
    mocks.invoke.mockImplementation(async (command, args) => {
      if (command === 'integrity_end_session') throw new Error('backend unavailable')
      return fallback(command, args)
    })
    vi.spyOn(console, 'warn').mockImplementation(() => undefined)
    await expect(service.stop()).rejects.toThrow('backend unavailable')
    expect(service.sessionId.value).toBe('session-1')
    expect(service.isActive.value).toBe(false)
    expect(vi.getTimerCount()).toBe(0)
    await expect(service.start('other-enrollment')).rejects.toThrow('previous Sentinel session')
    mocks.invoke.mockImplementation(fallback)
    await service.stop()
    expect(service.sessionId.value).toBeNull()
    await service.start('other-enrollment')
    expect(service.isActive.value).toBe(true)
    await service.stop()
  })
})

describe('Display topology signal', () => {
  const topology = (overrides: Partial<{
    display_count: number; external_display: boolean; mirrored: boolean; split_screen: boolean; source: string
  }> = {}) => ({
    display_count: 1, external_display: false, mirrored: false, split_screen: false, source: 'test', ...overrides,
  })

  const withTopology = (responses: Array<ReturnType<typeof topology> | null>) => {
    const fallback = mocks.invoke.getMockImplementation()!
    let i = 0
    mocks.invoke.mockImplementation(async (command, args) => {
      if (command === 'sentinel_display_topology') return responses[Math.min(i++, responses.length - 1)]
      return fallback(command, args)
    })
  }

  const snapshotFlags = () =>
    mocks.invoke.mock.calls
      .filter(([name]) => name === 'integrity_submit_snapshot')
      .map(([, args]) => (args?.req as { anomaly_flags: string[] }).anomaly_flags)

  it('samples a baseline at start and once per snapshot window', async () => {
    vi.spyOn(Math, 'random').mockReturnValue(0)
    withTopology([topology()])
    const service = (await freshService())()
    await service.start('enrollment')
    expect(mocks.invoke.mock.calls.filter(([n]) => n === 'sentinel_display_topology')).toHaveLength(1)
    await vi.advanceTimersByTimeAsync(15001)
    expect(mocks.invoke.mock.calls.filter(([n]) => n === 'sentinel_display_topology')).toHaveLength(2)
    expect(snapshotFlags()).toEqual([[]])
    await service.stop()
  })

  it('a null probe (unsupported platform) adds no flags and no signal fields', async () => {
    vi.spyOn(Math, 'random').mockReturnValue(0)
    withTopology([null])
    const service = (await freshService())()
    await service.start('enrollment')
    await vi.advanceTimersByTimeAsync(15001)
    expect(snapshotFlags()).toEqual([[]])
    expect(service.getDebugState().signals?.display_count).toBeUndefined()
    await service.stop()
  })

  it('a second monitor present from the start is info-only external_display, not a change', async () => {
    vi.spyOn(Math, 'random').mockReturnValue(0)
    withTopology([topology({ display_count: 2, external_display: true })])
    const service = (await freshService())()
    await service.start('enrollment')
    await vi.advanceTimersByTimeAsync(15001)
    await vi.advanceTimersByTimeAsync(15001)
    expect(snapshotFlags()).toEqual([['external_display'], ['external_display']])
    await service.stop()
  })

  it('a monitor plugged in mid-session raises display_change for that window only', async () => {
    vi.spyOn(Math, 'random').mockReturnValue(0)
    withTopology([
      topology(),                                              // baseline at start
      topology({ display_count: 2, external_display: true }),  // window 1: changed
      topology({ display_count: 2, external_display: true }),  // window 2: steady
    ])
    const service = (await freshService())()
    await service.start('enrollment')
    await vi.advanceTimersByTimeAsync(15001)
    await vi.advanceTimersByTimeAsync(15001)
    expect(snapshotFlags()).toEqual([
      ['external_display', 'display_change'],
      ['external_display'],
    ])
    await service.stop()
  })

  it('split screen and screen capture are flagged every window they persist', async () => {
    vi.spyOn(Math, 'random').mockReturnValue(0)
    withTopology([topology({ split_screen: true, mirrored: true })])
    const service = (await freshService())()
    await service.start('enrollment')
    await vi.advanceTimersByTimeAsync(15001)
    expect(snapshotFlags()).toEqual([['split_screen', 'screen_captured']])
    // getDebugState() only computes signals once there is typing to score.
    for (let i = 0; i < 5; i++) document.dispatchEvent(new KeyboardEvent('keydown', { key: 'a' }))
    const debug = service.getDebugState()
    expect(debug.signals?.split_screen).toBe(true)
    expect(debug.signals?.screen_captured).toBe(true)
    expect(debug.signals?.display_count).toBe(1)
    await service.stop()
  })

  it('a failing probe is tolerated and never flags', async () => {
    vi.spyOn(Math, 'random').mockReturnValue(0)
    vi.spyOn(console, 'warn').mockImplementation(() => undefined)
    const fallback = mocks.invoke.getMockImplementation()!
    mocks.invoke.mockImplementation(async (command, args) => {
      if (command === 'sentinel_display_topology') throw new Error('probe unavailable')
      return fallback(command, args)
    })
    const service = (await freshService())()
    await service.start('enrollment')
    await vi.advanceTimersByTimeAsync(15001)
    expect(snapshotFlags()).toEqual([[]])
    await service.stop()
  })

  it('window resize still produces no display flags', async () => {
    vi.spyOn(Math, 'random').mockReturnValue(0)
    withTopology([topology()])
    const service = (await freshService())()
    await service.start('enrollment')
    vi.stubGlobal('outerWidth', window.innerWidth + 800)
    window.dispatchEvent(new Event('resize'))
    await vi.advanceTimersByTimeAsync(15001)
    expect(snapshotFlags()).toEqual([[]])
    await service.stop()
  })
})

describe('Android environment signal and assessment shield', () => {
  const env = (overrides: Partial<{
    foreign_accessibility: string[]; adb_enabled: boolean; shield_active: boolean; obscured_touches: number
  }> = {}) => ({
    accessibility_services: [], foreign_accessibility: [], adb_enabled: false, development_settings_enabled: false,
    shield_active: true, overlay_hiding_supported: true, obscured_touches: 0, sdk_int: 36, source: 'android', ...overrides,
  })

  const withEnv = (report: ReturnType<typeof env> | null, obscured: number[] = [0]) => {
    const fallback = mocks.invoke.getMockImplementation()!
    let i = 0
    mocks.invoke.mockImplementation(async (command, args) => {
      if (command === 'sentinel_android_environment') return report
      if (command === 'sentinel_take_obscured_touches') return obscured[Math.min(i++, obscured.length - 1)]
      return fallback(command, args)
    })
  }

  const snapshotFlags = () =>
    mocks.invoke.mock.calls
      .filter(([name]) => name === 'integrity_submit_snapshot')
      .map(([, args]) => (args?.req as { anomaly_flags: string[] }).anomaly_flags)

  const shieldCalls = () =>
    mocks.invoke.mock.calls.filter(([name]) => name === 'sentinel_set_assessment_shield').map(([, args]) => (args as { on: boolean }).on)

  it('off Android (null report, zero touches) adds no flags', async () => {
    vi.spyOn(Math, 'random').mockReturnValue(0)
    withEnv(null)
    const service = (await freshService())()
    await service.start('enrollment')
    await vi.advanceTimersByTimeAsync(15001)
    expect(snapshotFlags()).toEqual([[]])
    await service.stop()
  })

  it('foreign accessibility service and adb are warnings; obscured touches are critical', async () => {
    vi.spyOn(Math, 'random').mockReturnValue(0)
    withEnv(env({ foreign_accessibility: ['com.evil/.Reader'], adb_enabled: true }), [3, 0])
    const service = (await freshService())()
    await service.start('enrollment')
    await vi.advanceTimersByTimeAsync(15001)
    await vi.advanceTimersByTimeAsync(15001)
    expect(snapshotFlags()).toEqual([
      ['foreign_accessibility_service', 'debug_bridge_enabled', 'obscured_touch'],
      ['foreign_accessibility_service', 'debug_bridge_enabled'],
    ])
    await service.stop()
  })

  it('a standalone assessment engages the shield at start and releases it at stop', async () => {
    withEnv(null)
    const service = (await freshService())()
    await service.start(null)
    expect(shieldCalls()).toEqual([true])
    await service.stop()
    expect(shieldCalls()).toEqual([true, false])
  })

  it('in a course the shield follows assessment elements only', async () => {
    withEnv(null)
    const service = (await freshService())()
    await service.start('enrollment')
    expect(shieldCalls()).toEqual([])
    service.setElement('e1', 'video')
    expect(shieldCalls()).toEqual([])
    service.setElement('e2', 'quiz')
    expect(shieldCalls()).toEqual([true])
    service.setElement('e3', 'quiz')      // idempotent
    expect(shieldCalls()).toEqual([true])
    service.setElement('e4', 'reading')
    expect(shieldCalls()).toEqual([true, false])
    await service.stop()
    expect(shieldCalls()).toEqual([true, false])
  })

  it('an interview session never engages the shield', async () => {
    withEnv(null)
    const service = (await freshService())()
    await service.start(null, false, 'interview')
    service.setElement('e', 'quiz')
    await service.stop()
    expect(shieldCalls()).toEqual([])
  })

  it('a failing shield IPC is tolerated', async () => {
    vi.spyOn(console, 'warn').mockImplementation(() => undefined)
    const fallback = mocks.invoke.getMockImplementation()!
    mocks.invoke.mockImplementation(async (command, args) => {
      if (command === 'sentinel_set_assessment_shield') throw new Error('no activity')
      return fallback(command, args)
    })
    const service = (await freshService())()
    await expect(service.start(null)).resolves.toBeUndefined()
    await service.stop()
  })
})

describe('Hidden overlay signal', () => {
  const snapshotFlags = () =>
    mocks.invoke.mock.calls
      .filter(([name]) => name === 'integrity_submit_snapshot')
      .map(([, args]) => (args?.req as { anomaly_flags: string[] }).anomaly_flags)

  const withScan = (scan: unknown) => {
    const fallback = mocks.invoke.getMockImplementation()!
    mocks.invoke.mockImplementation(async (command, args) => {
      if (command === 'sentinel_hidden_overlay') return scan
      return fallback(command, args)
    })
  }

  it('a clean scan adds no flag but records counts in signals', async () => {
    vi.spyOn(Math, 'random').mockReturnValue(0)
    withScan({ suspicious: [], allowlisted: 1, scanned: 17, source: 'cgwindow' })
    const service = (await freshService())()
    await service.start('enrollment')
    await vi.advanceTimersByTimeAsync(15001)
    expect(snapshotFlags()).toEqual([[]])
    for (let i = 0; i < 5; i++) document.dispatchEvent(new KeyboardEvent('keydown', { key: 'a' }))
    const signals = service.getDebugState().signals
    expect(signals?.hidden_overlays).toBe(0)
    expect(signals?.overlay_windows_scanned).toBe(17)
    expect(signals?.overlay_windows_allowlisted).toBe(1)
    await service.stop()
  })

  it('a capture-excluded window is a critical hidden_overlay every window it persists', async () => {
    vi.spyOn(Math, 'random').mockReturnValue(0)
    withScan({
      suspicious: [{ pid: 42, owner: 'Cluely', title: null, width: 800, height: 300, on_screen: true, reason: 'capture_excluded' }],
      allowlisted: 0, scanned: 12, source: 'cgwindow',
    })
    const service = (await freshService())()
    await service.start('enrollment')
    await vi.advanceTimersByTimeAsync(15001)
    await vi.advanceTimersByTimeAsync(15001)
    expect(snapshotFlags()).toEqual([['hidden_overlay'], ['hidden_overlay']])
    expect(service.debug.overlaySuspicious).toEqual(['Cluely:capture_excluded'])
    await service.stop()
  })

  it('a null scan (mobile / Wayland) is silent', async () => {
    vi.spyOn(Math, 'random').mockReturnValue(0)
    withScan(null)
    const service = (await freshService())()
    await service.start('enrollment')
    await vi.advanceTimersByTimeAsync(15001)
    expect(snapshotFlags()).toEqual([[]])
    await service.stop()
  })
})

describe('Virtual camera signal', () => {
  const snapshotFlags = () =>
    mocks.invoke.mock.calls
      .filter(([name]) => name === 'integrity_submit_snapshot')
      .map(([, args]) => (args?.req as { anomaly_flags: string[] }).anomaly_flags)

  it('a virtual camera label raises virtual_camera while the camera is opted in', async () => {
    vi.spyOn(Math, 'random').mockReturnValue(0)
    const service = (await freshService())()
    await service.start('enrollment', true)
    service.reportCameraDevice('OBS Virtual Camera')
    await vi.advanceTimersByTimeAsync(15001)
    expect(snapshotFlags()).toEqual([['virtual_camera']])
    expect(service.debug.cameraDeviceVirtual).toBe(true)
    await service.stop()
  })

  it('a real webcam adds nothing, and the label never reaches the snapshot', async () => {
    vi.spyOn(Math, 'random').mockReturnValue(0)
    const service = (await freshService())()
    await service.start('enrollment', true)
    service.reportCameraDevice('FaceTime HD Camera')
    await vi.advanceTimersByTimeAsync(15001)
    expect(snapshotFlags()).toEqual([[]])
    const snapshot = mocks.invoke.mock.calls.find(([name]) => name === 'integrity_submit_snapshot')?.[1]
    expect(JSON.stringify(snapshot)).not.toContain('FaceTime')
    await service.stop()
  })

  it('opting the camera out clears the device and the flag', async () => {
    vi.spyOn(Math, 'random').mockReturnValue(0)
    const service = (await freshService())()
    await service.start('enrollment', true)
    service.reportCameraDevice('ManyCam')
    service.setCameraOptedIn(false)
    await vi.advanceTimersByTimeAsync(15001)
    expect(snapshotFlags()).toEqual([[]])
    expect(service.debug.cameraDeviceLabel).toBe('')
    await service.stop()
  })
})

describe('Display topology native nudges', () => {
  const topology = (overrides: Record<string, unknown> = {}) => ({
    display_count: 1, external_display: false, mirrored: false, split_screen: false, native_transitions: 0, source: 'test', ...overrides,
  })
  const snapshotFlags = () =>
    mocks.invoke.mock.calls
      .filter(([name]) => name === 'integrity_submit_snapshot')
      .map(([, args]) => (args?.req as { anomaly_flags: string[] }).anomaly_flags)
  const withTopology = (responses: Array<ReturnType<typeof topology>>) => {
    const fallback = mocks.invoke.getMockImplementation()!
    let i = 0
    mocks.invoke.mockImplementation(async (command, args) => {
      if (command === 'sentinel_display_topology') return responses[Math.min(i++, responses.length - 1)]
      return fallback(command, args)
    })
  }
  const fireDisplayEvent = () => {
    const handler = mocks.listen.mock.calls.find(([name]) => name === 'sentinel://display')?.[1] as ((e: { payload: unknown }) => void) | undefined
    expect(handler).toBeDefined()
    handler!({ payload: { reason: 'moved' } })
  }

  it('a native transition counter delta flags split_screen even when the sample is back to normal', async () => {
    vi.spyOn(Math, 'random').mockReturnValue(0)
    withTopology([topology({ native_transitions: 4 }), topology({ native_transitions: 6 }), topology({ native_transitions: 6 })])
    const service = (await freshService())()
    await service.start('enrollment')
    await vi.advanceTimersByTimeAsync(15001)
    await vi.advanceTimersByTimeAsync(15001)
    expect(snapshotFlags()).toEqual([['split_screen', 'display_change'], []])
    await service.stop()
  })

  it('a window-moved nudge resamples once after the debounce and catches a new monitor', async () => {
    vi.spyOn(Math, 'random').mockReturnValue(0)
    withTopology([topology(), topology({ display_count: 2, external_display: true })])
    const service = (await freshService())()
    await service.start('enrollment')
    const before = mocks.invoke.mock.calls.filter(([n]) => n === 'sentinel_display_topology').length
    fireDisplayEvent(); fireDisplayEvent(); fireDisplayEvent()
    await vi.advanceTimersByTimeAsync(350)
    expect(mocks.invoke.mock.calls.filter(([n]) => n === 'sentinel_display_topology').length).toBe(before + 1)
    await vi.advanceTimersByTimeAsync(15001)
    expect(snapshotFlags()[0]).toEqual(['external_display', 'display_change'])
    await service.stop()
    expect(mocks.unlisten).toHaveBeenCalled()
  })
})
