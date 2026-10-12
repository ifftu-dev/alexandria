import { beforeEach, describe, expect, it, vi } from 'vitest'
import { ref } from 'vue'

const mocks = vi.hoisted(() => ({ invoke: vi.fn(), start: vi.fn(), stop: vi.fn() }))
const active = ref(false)
vi.mock('@/composables/useLocalApi', () => ({ useLocalApi: () => ({ invoke: mocks.invoke }) }))
vi.mock('@/composables/useSentinel', () => ({
  useSentinel: () => ({ start: mocks.start, stop: mocks.stop, isActive: active }),
}))

import { useInterviewCheck } from './useInterviewCheck'

beforeEach(async () => {
  mocks.invoke.mockReset()
  mocks.start.mockReset().mockImplementation(async () => { active.value = true })
  mocks.stop.mockReset().mockImplementation(async () => { active.value = false; return 'session-9' })
  // Module state carries over between tests; settle it.
  await useInterviewCheck().abandon()
})

describe('useInterviewCheck', () => {
  it('runs Sentinel for the call as an interview, then sends the stored session', async () => {
    const check = useInterviewCheck()
    await check.begin({ interviewId: 'int-1', cloud: 'http://cloud.test', camera: true })
    expect(mocks.start).toHaveBeenCalledWith(null, true, 'interview')
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
})
