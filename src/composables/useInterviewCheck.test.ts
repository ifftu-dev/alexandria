import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { ref } from 'vue'

const mocks = vi.hoisted(() => ({
  invoke: vi.fn(),
  start: vi.fn(),
  stop: vi.fn(),
  handlers: new Map<string, (event: { payload: unknown }) => void>(),
}))
const active = ref(false)
vi.mock('@/composables/useLocalApi', () => ({ useLocalApi: () => ({ invoke: mocks.invoke }) }))
vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(async (name: string, handler: (event: { payload: unknown }) => void) => {
    mocks.handlers.set(name, handler)
    return () => mocks.handlers.delete(name)
  }),
}))
vi.mock('@/composables/useSentinel', async () => {
  const { createResponseTracker } = await import('@/utils/sentinel/responseProcess')
  const responses = createResponseTracker()
  return {
    useSentinel: () => ({
      start: mocks.start,
      stop: mocks.stop,
      isActive: active,
      getSessionId: () => (active.value ? 'session-9' : null),
      responses,
    }),
  }
})

import { useInterviewCheck } from './useInterviewCheck'

const emit = (name: string, payload: unknown) => mocks.handlers.get(name)?.({ payload })
const commands = () => mocks.invoke.mock.calls.map(([name]) => name as string)

beforeEach(async () => {
  mocks.invoke.mockReset()
  mocks.handlers.clear()
  mocks.start.mockReset().mockImplementation(async () => { active.value = true })
  mocks.stop.mockReset().mockImplementation(async () => { active.value = false; return 'session-9' })
  // Module state carries over between tests; settle it.
  await useInterviewCheck().abandon()
})

afterEach(() => {
  vi.useRealTimers()
})

describe('useInterviewCheck', () => {
  it('runs Sentinel for the call as an interview, then sends the stored session', async () => {
    const check = useInterviewCheck()
    await check.begin({ interviewId: 'int-1', cloud: 'http://cloud.test', camera: true })
    expect(mocks.start).toHaveBeenCalledWith(null, true, 'interview', { microphone: false })
    expect(check.running.value?.interviewId).toBe('int-1')

    mocks.invoke.mockResolvedValueOnce({ integrity_session_id: 'session-9' })
    const next = await check.finish()
    expect(mocks.stop).toHaveBeenCalled()
    expect(mocks.invoke).toHaveBeenCalledWith('hiring_interview_integrity_send', {
      interviewId: 'int-1',
      integritySessionId: 'session-9',
    })
    expect(next).toBe('/interview-room/int-1?cloud=http%3A%2F%2Fcloud.test')
    expect(check.running.value).toBeNull()
    expect(check.unsent.value).toBeNull()
    // Finishing twice does nothing.
    expect(await check.finish()).toBeNull()
  })

  it('keeps an unsent result to try again', async () => {
    const check = useInterviewCheck()
    await check.begin({ interviewId: 'int-2', cloud: 'http://cloud.test', camera: false })
    mocks.invoke.mockRejectedValueOnce(new Error('Cloud unreachable'))
    await check.finish()
    expect(check.unsent.value).toEqual({ interviewId: 'int-2', cloud: 'http://cloud.test', sessionId: 'session-9' })
    expect(check.sendError.value).toBe('Cloud unreachable')

    mocks.invoke.mockResolvedValueOnce({ integrity_session_id: 'session-9' })
    expect(await check.send()).toEqual({ integrity_session_id: 'session-9' })
    expect(check.unsent.value).toBeNull()
    expect(check.sendError.value).toBeNull()
  })

  it('stops without sending when the call could not be joined', async () => {
    const check = useInterviewCheck()
    await check.begin({ interviewId: 'int-3', cloud: 'http://cloud.test', camera: false })
    await check.abandon()
    expect(mocks.stop).toHaveBeenCalled()
    expect(mocks.invoke).not.toHaveBeenCalled()
    expect(check.running.value).toBeNull()
  })

  it('never listens for voices without the candidate\'s microphone choice', async () => {
    const check = useInterviewCheck()
    await check.begin({ interviewId: 'int-4', cloud: 'http://cloud.test', camera: false })
    await check.listen()
    expect(commands()).not.toContain('sentinel_voice_listen')
    expect(check.voiceChecks.value).toBe(false)
    expect(mocks.handlers.size).toBe(0)
  })

  it('with voice checks allowed, times answers for the panel and stops listening before the check ends', async () => {
    vi.useFakeTimers()
    mocks.invoke.mockResolvedValue(undefined)
    const check = useInterviewCheck()
    await check.begin({ interviewId: 'int-5', cloud: 'http://cloud.test', camera: false, microphone: true })
    expect(mocks.start).toHaveBeenCalledWith(null, false, 'interview', { microphone: true })
    await check.listen()
    expect(mocks.invoke).toHaveBeenCalledWith('sentinel_voice_listen', { sessionId: 'session-9' })
    expect(check.voiceChecks.value).toBe(true)

    // The panel asks; the question plays for a second and ends; the
    // candidate starts answering 900 ms later.
    emit('tutoring:interview-signal', { sender: 'panel', sender_name: 'Panel', signal: { question: { question_id: 'q1' } }, timestamp: Date.now() })
    for (let t = 0; t < 1_000; t += 50) {
      emit('tutoring:audio-level', { mic_level: 0, output_level: 0.4 })
      vi.advanceTimersByTime(50)
    }
    const questionEnded = Date.now() - 50
    for (let t = 0; t < 600; t += 50) {
      emit('tutoring:audio-level', { mic_level: 0, output_level: 0 })
      vi.advanceTimersByTime(50)
    }
    emit('tutoring:voice-onset', { at_ms: questionEnded + 900, speaker: 'primary' })
    expect(mocks.invoke).toHaveBeenCalledWith('tutoring_send_interview_signal', {
      signal: { answer: { question_id: 'q1', latency_ms: 900, baseline_ms: null, gaze_down_before: false, second_voice_before: false } },
    })

    mocks.invoke.mockImplementation(async (name: string) => (name === 'hiring_interview_integrity_send' ? { integrity_session_id: 'session-9' } : undefined))
    const stopOrder: string[] = []
    mocks.stop.mockImplementation(async () => { stopOrder.push('sentinel.stop'); active.value = false; return 'session-9' })
    mocks.invoke.mockImplementationOnce(async (name: string) => { stopOrder.push(name) })
    await check.finish()
    expect(stopOrder).toEqual(['sentinel_voice_stop', 'sentinel.stop'])
    expect(check.voiceChecks.value).toBe(false)
    expect(mocks.handlers.size).toBe(0)
  })
})
