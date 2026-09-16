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
    expect(mocks.unlisten).toHaveBeenCalledOnce()
    expect(vi.getTimerCount()).toBe(0)
    expect(starter.isActive.value).toBe(false)
  })

  it('concurrent starts create one backend session and one set of listeners', async () => {
    const useSentinel = await freshService()
    const service = useSentinel()
    await Promise.all([service.start('enrollment'), useSentinel().start('enrollment')])
    expect(mocks.invoke.mock.calls.filter(([name]) => name === 'integrity_start_session')).toHaveLength(1)
    expect(mocks.listen).toHaveBeenCalledOnce()
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
