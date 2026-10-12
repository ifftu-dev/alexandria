import { flushPromises, mount } from '@vue/test-utils'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import CandidateRoom from './CandidateRoom.vue'

const mocks = vi.hoisted(() => ({ invoke: vi.fn(), joinRoom: vi.fn(), push: vi.fn() }))
vi.mock('@/composables/useLocalApi', () => ({ useLocalApi: () => ({ invoke: mocks.invoke }) }))
vi.mock('@/composables/useTutoringRoom', () => ({ useTutoringRoom: () => ({ joinRoom: mocks.joinRoom }) }))
vi.mock('vue-router', () => ({
  useRoute: () => ({ params: { id: 'int-1' }, query: { cloud: 'http://127.0.0.1:8787' } }),
  useRouter: () => ({ push: mocks.push }),
}))

const mountRoom = () => mount(CandidateRoom, { global: { mocks: { $t: (key: string) => key } } })

beforeEach(() => {
  vi.useFakeTimers()
  mocks.invoke.mockReset()
  mocks.joinRoom.mockReset().mockResolvedValue({ id: 'room-7' })
  mocks.push.mockReset()
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

    mocks.invoke.mockResolvedValue({ ticket: 'ticket-abc', openedAt: '2026-10-12T10:00:00Z' })
    await vi.advanceTimersByTimeAsync(5000)
    expect(wrapper.text()).toContain('tutoring.interviewRoom.ready')

    await wrapper.get('[data-testid=join-interview]').trigger('click'); await flushPromises()
    expect(mocks.joinRoom).toHaveBeenCalledWith('ticket-abc')
    expect(mocks.push).toHaveBeenCalledWith('/tutoring/room-7')

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
})
