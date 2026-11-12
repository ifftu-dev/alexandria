// Seam 5 for response-process signals: the collector turns a sequence of
// item presentations and response events into the raw per-visit timing it
// submits. Nothing here is about scores or flags; the backend derives those.
import { webcrypto } from 'node:crypto'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { ref } from 'vue'
import type { BehavioralProfile, ItemResponse, ResponseFeatures, WindowFeatures } from '@/types'

const mocks = vi.hoisted(() => ({
  invoke: vi.fn<(command: string, args?: Record<string, unknown>) => Promise<unknown>>(),
  listen: vi.fn(),
  stakeAddress: null as string | null,
  storedProfile: null as unknown,
}))

vi.mock('@tauri-apps/api/core', () => ({ invoke: mocks.invoke }))
vi.mock('@tauri-apps/api/event', () => ({ listen: mocks.listen }))
vi.mock('../useAuth', () => ({ useAuth: () => ({ stakeAddress: ref(mocks.stakeAddress) }) }))

const submitted = (): ResponseFeatures[] =>
  mocks.invoke.mock.calls
    .filter(([name]) => name === 'integrity_submit_snapshot')
    .map(([, args]) => (args?.req as { features: WindowFeatures }).features.response)
    .filter((r): r is ResponseFeatures => r !== null)

const submittedItems = (): ItemResponse[] => submitted().flatMap(r => r.items)

const savedProfiles = (): BehavioralProfile[] =>
  mocks.invoke.mock.calls
    .filter(([name]) => name === 'sentinel_save_behavioral_profile')
    .map(([, args]) => args?.profile as BehavioralProfile)

async function startedService() {
  vi.resetModules()
  const service = (await import('../useSentinel')).useSentinel()
  await service.start(null)
  return service
}

const visit = (overrides: Partial<ItemResponse>): ItemResponse => ({
  item_id: 'q1',
  input: 'choice',
  chars: 200,
  difficulty: 3,
  revisit: false,
  first_input_ms: 0,
  commit_ms: 0,
  revisions: 0,
  idle_bursts: 0,
  changes_after_idle: 0,
  ...overrides,
})

beforeEach(() => {
  vi.useFakeTimers()
  vi.stubGlobal('crypto', webcrypto)
  vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockReturnValue(null)
  // The periodic snapshot then fires at exactly 15 s, which the tests avoid
  // or expect.
  vi.spyOn(Math, 'random').mockReturnValue(0)
  mocks.stakeAddress = null
  mocks.storedProfile = null
  mocks.listen.mockReset().mockResolvedValue(() => undefined)
  mocks.invoke.mockReset().mockImplementation(async (command) => {
    if (command === 'integrity_start_session') return { session_id: 'session-1' }
    if (command === 'integrity_end_session') return { id: 'session-1', status: 'completed' }
    if (command === 'sentinel_paste_classifier_info') return { source: 'bundled', version: 'v1' }
    if (command === 'sentinel_score_paste') return { score: -1, classifier: null }
    if (command === 'sentinel_user_models_status') return []
    if (command === 'sentinel_load_behavioral_profile') return mocks.storedProfile
    return null
  })
})

afterEach(() => {
  vi.clearAllTimers()
  vi.useRealTimers()
  vi.restoreAllMocks()
  vi.unstubAllGlobals()
})

describe('response-process collector', () => {
  it('submits a visit\'s time to first input, time to commit and revisions', async () => {
    const service = await startedService()
    service.responses.present('q1', { chars: 200, difficulty: 3, input: 'choice' })
    await vi.advanceTimersByTimeAsync(8_000)
    service.responses.input('q1', { changesAnswer: false })
    await vi.advanceTimersByTimeAsync(3_000)
    service.responses.input('q1', { changesAnswer: true })
    await service.stop()
    expect(submittedItems()).toEqual([
      visit({ first_input_ms: 8_000, commit_ms: 11_000, revisions: 1 }),
    ])
  })

  it('times a listed item from when the learner finished the one before, and a return as a revisit', async () => {
    const service = await startedService()
    // The whole attempt renders at once.
    service.responses.present('q1', { chars: 200, difficulty: 3, input: 'choice' })
    service.responses.present('q2', { chars: 80, difficulty: 1, input: 'text' })
    await vi.advanceTimersByTimeAsync(6_000)
    service.responses.input('q1', { changesAnswer: false })
    await vi.advanceTimersByTimeAsync(4_000)
    service.responses.input('q2', { changesAnswer: false })
    await vi.advanceTimersByTimeAsync(1_000)
    service.responses.input('q2', { changesAnswer: false })
    await vi.advanceTimersByTimeAsync(2_000)
    service.responses.input('q1', { changesAnswer: true })
    // Inputs on an item that was never shown are not response events.
    service.responses.input('unknown', { changesAnswer: false })
    await service.stop()
    expect(submittedItems()).toEqual([
      visit({ first_input_ms: 6_000, commit_ms: 6_000 }),
      visit({ item_id: 'q2', input: 'text', chars: 80, difficulty: 1, first_input_ms: 4_000, commit_ms: 5_000 }),
      visit({ revisit: true, revisions: 1 }),
    ])
  })

  it('counts idle-then-burst and answer changes after idle, but not the wait to read a new item', async () => {
    const service = await startedService()
    service.responses.present('q1', { chars: 200, difficulty: 3, input: 'choice' })
    service.responses.present('q2', { chars: 200, difficulty: 3, input: 'choice' })
    // 30 s of reading, then three quick picks: reading, not idle.
    await vi.advanceTimersByTimeAsync(14_000)
    await vi.advanceTimersByTimeAsync(14_000)
    for (let i = 0; i < 3; i++) {
      service.responses.input('q1', { changesAnswer: false })
      await vi.advanceTimersByTimeAsync(500)
    }
    // Idle with an answer given, then a change and two more picks at once.
    await vi.advanceTimersByTimeAsync(24_000)
    service.responses.input('q1', { changesAnswer: true })
    await vi.advanceTimersByTimeAsync(500)
    service.responses.input('q1', { changesAnswer: false })
    await vi.advanceTimersByTimeAsync(500)
    service.responses.input('q1', { changesAnswer: false })
    // The next item takes 25 s to read and is answered in three quick picks.
    await vi.advanceTimersByTimeAsync(25_000)
    for (let i = 0; i < 3; i++) {
      service.responses.input('q2', { changesAnswer: false })
      await vi.advanceTimersByTimeAsync(500)
    }
    await service.stop()
    const [q1, q2] = submittedItems()
    expect(q1).toMatchObject({ item_id: 'q1', revisions: 1, idle_bursts: 1, changes_after_idle: 1 })
    expect(q2).toMatchObject({ item_id: 'q2', first_input_ms: 25_000, idle_bursts: 0, changes_after_idle: 0 })
  })

  it('counts a response that follows a second voice within a few seconds, once per cue', async () => {
    const service = await startedService()
    service.responses.present('q1', { chars: 50, difficulty: 1, input: 'choice' })
    service.responses.cue('second_voice')
    await vi.advanceTimersByTimeAsync(2_000)
    service.responses.input('q1', { changesAnswer: false })
    await vi.advanceTimersByTimeAsync(500)
    service.responses.input('q1', { changesAnswer: true })
    // A cue long before the response is not linked to it.
    service.responses.cue('second_voice')
    await vi.advanceTimersByTimeAsync(5_000)
    service.responses.input('q1', { changesAnswer: true })
    await service.stop()
    expect(submitted().map(r => [r.voice_then_response, r.gaze_down_then_response])).toEqual([[1, 0]])
  })

  it('counts a response that follows a gaze-down tick', async () => {
    mocks.stakeAddress = 'gaze-user'
    vi.spyOn(document, 'hidden', 'get').mockReturnValue(false)
    vi.mocked(HTMLCanvasElement.prototype.getContext).mockReturnValue({
      drawImage: vi.fn(),
      getImageData: () => ({ data: new Uint8ClampedArray(4) }),
    } as unknown as CanvasRenderingContext2D)
    const video = document.createElement('video')
    Object.defineProperties(video, { readyState: { value: 2 }, videoWidth: { value: 1 }, videoHeight: { value: 1 } })
    const fallback = mocks.invoke.getMockImplementation()!
    mocks.invoke.mockImplementation(async (command, args) => {
      if (command === 'sentinel_score_gaze') {
        return {
          estimate: { yaw: 0, pitch: 0.3, screenX: 0.5, screenY: 1.4, onScreen: false, occluded: false, confidence: 1 },
          faceCount: 1,
        }
      }
      return fallback(command, args)
    })
    vi.resetModules()
    const service = (await import('../useSentinel')).useSentinel()
    await service.start(null, true)
    service.responses.present('q1', { chars: 50, difficulty: 1, input: 'choice' })
    await service.scoreGaze(video)
    await vi.advanceTimersByTimeAsync(1_000)
    service.responses.input('q1', { changesAnswer: false })
    await service.stop()
    expect(submitted().map(r => r.gaze_down_then_response)).toEqual([1])
  })

  it('takes the baseline from the first items of the attempt when there is no history, and keeps them', async () => {
    mocks.stakeAddress = 'learner'
    const service = await startedService()
    const items = ['q1', 'q2', 'q3', 'q4']
    for (const id of items) service.responses.present(id, { chars: 100, difficulty: 2, input: 'choice' })
    for (const id of items) {
      await vi.advanceTimersByTimeAsync(3_000)
      service.responses.input(id, { changesAnswer: false })
    }
    await service.stop()
    const [window] = submitted()
    const first3 = Array.from({ length: 3 }, () => ({ chars: 100, difficulty: 2, first_input_ms: 3_000 }))
    expect(window!.baseline).toEqual({ source: 'attempt', items: first3 })
    expect(savedProfiles().slice(-1)[0]!.responsePattern).toEqual({
      recent: [...first3, { chars: 100, difficulty: 2, first_input_ms: 3_000 }],
    })
  })

  it('prefers the learner\'s history from earlier sessions on this device', async () => {
    mocks.stakeAddress = 'learner'
    const history = Array.from({ length: 4 }, (_, i) => ({ chars: 60, difficulty: 1, first_input_ms: 2_000 + i }))
    vi.resetModules()
    const service = (await import('../useSentinel')).useSentinel()
    const deviceFingerprint = await service.computeDeviceFingerprint()
    mocks.invoke.mockImplementation(async (command) => {
      if (command === 'integrity_start_session') return { session_id: 'session-1' }
      if (command === 'integrity_end_session') return { id: 'session-1', status: 'completed' }
      if (command === 'sentinel_load_behavioral_profile') {
        return {
          userId: 'learner',
          deviceFingerprint,
          typingPattern: { avgDwellTime: 80, avgFlightTime: 120, speedWpm: 60, sampleCount: 0 },
          mousePattern: { avgVelocity: 2, avgAcceleration: 0.5, clickPrecision: 0.9, sampleCount: 0 },
          lastUpdated: 1,
          responsePattern: { recent: history },
        }
      }
      return null
    })
    await service.start(null)
    service.responses.present('q1', { chars: 100, difficulty: 2, input: 'choice' })
    await vi.advanceTimersByTimeAsync(9_000)
    service.responses.input('q1', { changesAnswer: false })
    await service.stop()
    expect(submitted()[0]!.baseline).toEqual({ source: 'history', items: history })
  })

  it('times a spoken interview answer from the end of the question to the first speech, without device history', async () => {
    mocks.stakeAddress = 'learner'
    const history = Array.from({ length: 4 }, () => ({ chars: 60, difficulty: 1, first_input_ms: 2_000 }))
    vi.resetModules()
    const service = (await import('../useSentinel')).useSentinel()
    mocks.storedProfile = {
      userId: 'learner',
      deviceFingerprint: await service.computeDeviceFingerprint(),
      typingPattern: { avgDwellTime: 80, avgFlightTime: 120, speedWpm: 60, sampleCount: 0 },
      mousePattern: { avgVelocity: 2, avgAcceleration: 0.5, clickPrecision: 0.9, sampleCount: 0 },
      lastUpdated: 1,
      responsePattern: { recent: history },
    }
    await service.start(null, false, 'interview')
    const spoken = { chars: 0, difficulty: null, input: 'voice' as const }
    // Each question ended 400 ms before it was handed over; the answer
    // began at a time the voice detector reports.
    for (const [id, latency] of [['q1', 700], ['q2', 900], ['q3', 800], ['q4', 5_000]] as const) {
      await vi.advanceTimersByTimeAsync(10_000)
      const endedAt = Date.now() - 400
      service.responses.present(id, spoken, endedAt)
      service.responses.input(id, { changesAnswer: false, at: endedAt + latency })
    }
    expect(service.responses.latest()).toEqual({ itemId: 'q4', latencyMs: 5_000, baselineMs: 800, cues: [] })
    await service.stop()
    expect(submittedItems().map(i => [i.item_id, i.input, i.first_input_ms])).toEqual([
      ['q1', 'voice', 700], ['q2', 'voice', 900], ['q3', 'voice', 800], ['q4', 'voice', 5_000],
    ])
    // Click timings from assessments say nothing about spoken turns.
    expect(submitted().slice(-1)[0]!.baseline?.source).toBe('attempt')
    expect(savedProfiles().length).toBeGreaterThan(0)
    expect(savedProfiles().map(p => p.responsePattern)).toEqual(savedProfiles().map(() => ({ recent: history })))
  })

  it('reports which cues came just before the latest answer', async () => {
    const service = await startedService()
    service.responses.present('q1', { chars: 0, difficulty: null, input: 'voice' })
    await vi.advanceTimersByTimeAsync(2_000)
    service.responses.cue('second_voice', Date.now() - 500)
    service.responses.input('q1', { changesAnswer: false })
    expect(service.responses.latest()).toEqual({ itemId: 'q1', latencyMs: 2_000, baselineMs: null, cues: ['second_voice'] })
    // A cue reported as happening after the answer began is not before it.
    service.responses.present('q2', { chars: 0, difficulty: null, input: 'voice' })
    service.responses.cue('gaze_down', Date.now() + 1_000)
    service.responses.input('q2', { changesAnswer: false })
    expect(service.responses.latest()?.cues).toEqual([])
    await service.stop()
  })

  it('submits nothing when no item was answered', async () => {
    const service = await startedService()
    service.responses.present('q1', { chars: 100, difficulty: 2, input: 'choice' })
    await service.stop()
    const features = mocks.invoke.mock.calls
      .filter(([name]) => name === 'integrity_submit_snapshot')
      .map(([, args]) => (args?.req as { features: WindowFeatures }).features)
    expect(features.map(f => f.response)).toEqual([null])
  })
})
