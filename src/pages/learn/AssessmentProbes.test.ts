import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import AssessmentProbes from './AssessmentProbes.vue'
import type { ServedProbe } from '@/types'

const mocks = vi.hoisted(() => ({
  startProbes: vi.fn<(attempt: string) => Promise<ServedProbe[]>>(),
  renderProbe: vi.fn<(probe: string) => Promise<ServedProbe>>(),
  answerProbe: vi.fn<(probe: string, response: Record<string, unknown>, firstInputMs: number | null) => Promise<unknown>>(),
  finishProbes: vi.fn<(attempt: string) => Promise<unknown>>(),
}))

vi.mock('@/composables/useAssessment', () => ({
  useAssessment: () => ({
    startProbes: mocks.startProbes,
    renderProbe: mocks.renderProbe,
    answerProbe: mocks.answerProbe,
    finishProbes: mocks.finishProbes,
  }),
}))
vi.mock('@/components/ui', async () => ({
  AppButton: (await import('@/components/ui/AppButton.vue')).default,
}))

function probe(id: string, extra: Partial<ServedProbe> = {}): ServedProbe {
  return {
    id,
    item_kind: 'mcq',
    content: { kind: 'multi', prompt: `Prompt ${id}`, options: ['Red', 'Green', 'Blue'], probe: 'complement' },
    time_box_ms: 30_000,
    remaining_ms: null,
    ...extra,
  }
}

function render() {
  return mount(AssessmentProbes, {
    props: { attemptId: 'attempt-1' },
    global: { mocks: { $t: (key: string, args?: Record<string, unknown>) => (args ? `${key}:${JSON.stringify(args)}` : key) } },
  })
}

function button(wrapper: ReturnType<typeof render>, label: string) {
  return wrapper.findAll('button').find(b => b.text() === label)!
}

beforeEach(() => {
  vi.useFakeTimers()
  mocks.startProbes.mockReset()
  mocks.renderProbe.mockReset().mockImplementation(async id => probe(id, { remaining_ms: 30_000 }))
  mocks.answerProbe.mockReset().mockResolvedValue({ in_time: true })
  mocks.finishProbes.mockReset().mockResolvedValue({})
})

afterEach(() => {
  vi.useRealTimers()
})

describe('assessment probes', () => {
  it('finishes at once when the attempt has no probes', async () => {
    mocks.startProbes.mockResolvedValue([])
    const wrapper = render()
    await flushPromises()
    expect(wrapper.emitted('done')).toHaveLength(1)
    expect(mocks.renderProbe).not.toHaveBeenCalled()
    expect(mocks.finishProbes).not.toHaveBeenCalled()
  })

  it('renders each probe, submits served positions with first-input latency, then finishes', async () => {
    mocks.startProbes.mockResolvedValue([probe('p1'), probe('p2')])
    const wrapper = render()
    await flushPromises()
    expect(mocks.renderProbe).toHaveBeenCalledExactlyOnceWith('p1')
    expect(wrapper.text()).toContain('Prompt p1')
    expect(wrapper.text()).toContain('learn.assessment.probeComplement')

    vi.advanceTimersByTime(1_200)
    const inputs = wrapper.findAll('input')
    await inputs[0]!.setValue(true)
    await inputs[2]!.setValue(true)
    await button(wrapper, 'learn.assessment.probeAnswer').trigger('click')
    await flushPromises()
    expect(mocks.answerProbe).toHaveBeenCalledExactlyOnceWith('p1', { selected_positions: [0, 2] }, 1_200)
    expect(mocks.renderProbe).toHaveBeenLastCalledWith('p2')

    await button(wrapper, 'learn.assessment.probeSkip').trigger('click')
    await flushPromises()
    expect(mocks.answerProbe).toHaveBeenCalledOnce()
    expect(mocks.finishProbes).toHaveBeenCalledExactlyOnceWith('attempt-1')
    expect(wrapper.emitted('done')).toHaveLength(1)
  })

  it('answers what is selected when the time box runs out', async () => {
    mocks.startProbes.mockResolvedValue([probe('p1')])
    const wrapper = render()
    await flushPromises()
    await wrapper.findAll('input')[1]!.setValue(true)
    vi.advanceTimersByTime(31_000)
    await flushPromises()
    expect(mocks.answerProbe).toHaveBeenCalledOnce()
    expect(mocks.answerProbe.mock.calls[0]![1]).toEqual({ selected_positions: [1] })
    expect(wrapper.emitted('done')).toHaveLength(1)
  })

  it('skips a probe it cannot render and still finishes when the backend errs', async () => {
    mocks.startProbes.mockResolvedValue([probe('p1', { item_kind: 'plugin' })])
    mocks.finishProbes.mockRejectedValue(new Error('session ended'))
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined)
    const wrapper = render()
    await flushPromises()
    expect(mocks.renderProbe).not.toHaveBeenCalled()
    expect(wrapper.emitted('done')).toHaveLength(1)
    warn.mockRestore()
  })
})
