import { flushPromises, mount } from '@vue/test-utils'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { ref } from 'vue'
import CandidateRoom from './CandidateRoom.vue'

const mocks = vi.hoisted(() => ({
  invoke: vi.fn(),
  joinRoom: vi.fn(),
  listDevices: vi.fn(),
  push: vi.fn(),
  begin: vi.fn(),
  abandon: vi.fn(),
  send: vi.fn(),
  sent: vi.fn(),
}))
const unsent = ref<{ interviewId: string; cloud: string; sessionId: string } | null>(null)
const sendError = ref<string | null>(null)
vi.mock('@/composables/useLocalApi', () => ({ useLocalApi: () => ({ invoke: mocks.invoke }) }))
vi.mock('@/composables/useTutoringRoom', () => ({
  useTutoringRoom: () => ({ joinRoom: mocks.joinRoom, listDevices: mocks.listDevices }),
}))
vi.mock('@/composables/useInterviewCheck', () => ({
  useInterviewCheck: () => ({
    unsent, sendError, begin: mocks.begin, abandon: mocks.abandon, send: mocks.send, sent: mocks.sent,
  }),
}))
vi.mock('vue-router', () => ({
  useRoute: () => ({ params: { id: 'int-1' }, query: { cloud: 'http://127.0.0.1:8787' } }),
  useRouter: () => ({ push: mocks.push }),
}))

const mountRoom = () => mount(CandidateRoom, {
  global: { mocks: { $t: (key: string, values?: Record<string, unknown>) => (values ? `${key} ${JSON.stringify(values)}` : key), $te: () => false } },
})

beforeEach(() => {
  vi.useFakeTimers()
  mocks.invoke.mockReset()
  mocks.joinRoom.mockReset().mockResolvedValue({ id: 'room-7' })
  mocks.listDevices.mockReset().mockResolvedValue({
    audio_inputs: [{ id: 'mic-1', name: 'Built-in Microphone', is_default: true }],
    audio_outputs: [],
    cameras: [{ index: 'Index(0)', name: 'MacBook Pro Camera' }, { index: 'Index(1)', name: 'iPhone Camera' }],
    selected_audio_input: null,
    selected_audio_output: null,
  })
  localStorage.clear()
  mocks.push.mockReset()
  mocks.begin.mockReset().mockResolvedValue(undefined)
  mocks.abandon.mockReset().mockResolvedValue(undefined)
  mocks.send.mockReset().mockResolvedValue(null)
  mocks.sent.mockReset().mockResolvedValue([])
  unsent.value = null
  sendError.value = null
})
afterEach(() => { vi.useRealTimers() })

describe('CandidateRoom', () => {
  it('waits for the panel, then joins the room it opened', async () => {
    mocks.invoke.mockResolvedValueOnce({ ticket: null, openedAt: null })
    const wrapper = mountRoom(); await flushPromises()
    expect(mocks.invoke).toHaveBeenCalledWith('hiring_interview_room', {
      directoryUrl: 'http://127.0.0.1:8787',
      interviewId: 'int-1',
    })
    expect(wrapper.text()).toContain('tutoring.interviewRoom.waiting')
    expect(wrapper.get('[data-testid=join-interview]').attributes('disabled')).toBeDefined()
    expect(wrapper.find('[data-testid=integrity-consent]').exists()).toBe(false)

    mocks.invoke.mockResolvedValue({ ticket: 'ticket-abc', openedAt: '2026-10-12T10:00:00Z' })
    await vi.advanceTimersByTimeAsync(5000)
    expect(wrapper.text()).toContain('tutoring.interviewRoom.ready')

    await wrapper.get('[data-testid=join-interview]').trigger('click'); await flushPromises()
    expect(mocks.begin).not.toHaveBeenCalled()
    expect(mocks.joinRoom).toHaveBeenCalledWith('ticket-abc', undefined, undefined, null, null, null, { kind: 'interview_candidate', contextId: 'int-1' })
    expect(mocks.push).toHaveBeenCalledWith(`/interview-call/room-7?cloud=${encodeURIComponent('http://127.0.0.1:8787')}`)

    // No more asking once the room is known.
    const asked = mocks.invoke.mock.calls.length
    await vi.advanceTimersByTimeAsync(20_000)
    expect(mocks.invoke.mock.calls.length).toBe(asked)
  })

  it('shows why it cannot ask, and keeps trying', async () => {
    mocks.invoke.mockRejectedValueOnce(new Error('offline'))
    const wrapper = mountRoom(); await flushPromises()
    expect(wrapper.text()).toContain('offline')
    mocks.invoke.mockResolvedValue({ ticket: 'ticket-abc', openedAt: null })
    await vi.advanceTimersByTimeAsync(5000)
    expect(wrapper.text()).not.toContain('offline')
  })

  it('asks the candidate to choose before joining when the organisation asked for the check', async () => {
    mocks.invoke.mockResolvedValue({ ticket: 'ticket-abc', openedAt: null, integrityCheck: true })
    const wrapper = mountRoom(); await flushPromises()
    const consent = wrapper.get('[data-testid=integrity-consent]')
    expect(consent.text()).toContain('tutoring.interviewRoom.check.shared')
    expect(consent.text()).toContain('tutoring.interviewRoom.check.optional')
    // Nothing is preselected, so joining waits for an answer.
    expect((wrapper.get('[data-testid=check-run]').element as HTMLInputElement).checked).toBe(false)
    expect((wrapper.get('[data-testid=check-skip]').element as HTMLInputElement).checked).toBe(false)
    expect(wrapper.get('[data-testid=join-interview]').attributes('disabled')).toBeDefined()

    await wrapper.get('[data-testid=check-run]').setValue(true)
    expect((wrapper.get('[data-testid=check-camera]').element as HTMLInputElement).checked).toBe(false)
    await wrapper.get('[data-testid=check-camera]').setValue(true)
    await wrapper.get('[data-testid=join-interview]').trigger('click'); await flushPromises()
    expect(mocks.begin).toHaveBeenCalledWith({ interviewId: 'int-1', cloud: 'http://127.0.0.1:8787', camera: true })
    expect(mocks.begin.mock.invocationCallOrder[0]!).toBeLessThan(mocks.joinRoom.mock.invocationCallOrder[0]!)
    expect(mocks.push).toHaveBeenCalledWith(`/interview-call/room-7?cloud=${encodeURIComponent('http://127.0.0.1:8787')}`)
  })

  it('joins without the check when the candidate says no', async () => {
    mocks.invoke.mockResolvedValue({ ticket: 'ticket-abc', openedAt: null, integrityCheck: true })
    const wrapper = mountRoom(); await flushPromises()
    await wrapper.get('[data-testid=check-skip]').setValue(true)
    await wrapper.get('[data-testid=join-interview]').trigger('click'); await flushPromises()
    expect(mocks.begin).not.toHaveBeenCalled()
    expect(mocks.joinRoom).toHaveBeenCalledWith('ticket-abc', undefined, undefined, null, null, null, { kind: 'interview_candidate', contextId: 'int-1' })
  })

  it('stops the check if the room cannot be joined', async () => {
    mocks.invoke.mockResolvedValue({ ticket: 'ticket-abc', openedAt: null, integrityCheck: true })
    mocks.joinRoom.mockRejectedValueOnce(new Error('relay unreachable'))
    const wrapper = mountRoom(); await flushPromises()
    await wrapper.get('[data-testid=check-run]').setValue(true)
    await wrapper.get('[data-testid=join-interview]').trigger('click'); await flushPromises()
    expect(mocks.abandon).toHaveBeenCalled()
    expect(wrapper.text()).toContain('relay unreachable')
    expect(mocks.push).not.toHaveBeenCalled()
  })

  it('shows exactly what was shared after the call', async () => {
    mocks.invoke.mockResolvedValue({ ticket: 'ticket-abc', openedAt: null, integrityCheck: true })
    mocks.sent.mockResolvedValue([{
      integrity_session_id: 's-1', interview_id: 'int-1', organization: 'IFFTU', sent_at: '2026-10-12T10:40:00Z',
      report: {
        outcome: 'flagged', score: 71, started_at: 1_000, ended_at: 1_000 + 31 * 60, snapshots: 60,
        camera: false, camera_coverage: 0,
        flags: [{ flag: 'app_switch', severity: 'warning', count: 2 }],
      },
    }])
    const wrapper = mountRoom(); await flushPromises()
    expect(mocks.sent).toHaveBeenCalledWith('int-1')
    const card = wrapper.get('[data-testid=integrity-shared]')
    expect(card.text()).toContain('{"org":"IFFTU"}')
    expect(card.text()).toContain('tutoring.interviewRoom.check.flagged')
    expect(card.text()).toContain('71 / 100')
    expect(card.text()).toContain('{"minutes":31,"snapshots":60}')
    expect(card.text()).toContain('app_switch × 2')
    expect(card.text()).toContain('tutoring.interviewRoom.check.cameraOff')
  })

  it('offers to send again when the result did not get through', async () => {
    mocks.invoke.mockResolvedValue({ ticket: null, openedAt: null, integrityCheck: true })
    unsent.value = { interviewId: 'int-1', cloud: 'http://127.0.0.1:8787', sessionId: 's-1' }
    sendError.value = 'Cloud unreachable'
    const wrapper = mountRoom(); await flushPromises()
    const box = wrapper.get('[data-testid=integrity-unsent]')
    expect(box.text()).toContain('Cloud unreachable')
    mocks.send.mockResolvedValueOnce({ integrity_session_id: 's-1' })
    await wrapper.get('[data-testid=integrity-retry]').trigger('click'); await flushPromises()
    expect(mocks.send).toHaveBeenCalled()
    expect(mocks.sent).toHaveBeenCalledTimes(2)
  })
  it('joins with the camera and microphone the candidate picked', async () => {
    mocks.invoke.mockResolvedValue({ ticket: 'ticket-abc', openedAt: '2026-10-12T10:00:00Z' })
    const wrapper = mountRoom(); await flushPromises()
    await wrapper.get('[data-testid=device-camera]').setValue('Index(1)')
    await wrapper.get('[data-testid=device-mic]').setValue('mic-1')
    await wrapper.get('[data-testid=join-interview]').trigger('click'); await flushPromises()
    expect(mocks.joinRoom).toHaveBeenCalledWith('ticket-abc', undefined, undefined, 'Index(1)', 'mic-1', null, { kind: 'interview_candidate', contextId: 'int-1' })
    expect(JSON.parse(localStorage.getItem('alexandria.call-devices') ?? '{}')).toEqual({ camera: 'Index(1)', mic: 'mic-1', speaker: null })
  })
})
