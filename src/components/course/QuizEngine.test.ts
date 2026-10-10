import { describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import QuizEngine from './QuizEngine.vue'

vi.mock('vue-i18n', () => ({ useI18n: () => ({ t: (key: string) => key }) }))
vi.mock('@/composables/useLocalApi', () => ({ useLocalApi: () => ({ invoke: vi.fn().mockResolvedValue(null) }) }))

function render(points?: number) {
  return mount(QuizEngine, {
    props: { contentCid: null, elementId: 'quiz', contentInline: JSON.stringify({ questions: [
      { id: 'q1', type: 'single_choice', prompt: 'Read a resource?', options: ['GET', 'DELETE'], correct_indices: [0], points },
    ] }) },
    global: { mocks: { $t: (key: string) => key } },
  })
}

describe('authored quizzes', () => {
  it('grades legacy editor questions without points as one point each', async () => {
    const wrapper = render()
    await flushPromises()
    await wrapper.findAll('button').find(button => button.text().includes('GET'))!.trigger('click')
    await wrapper.findAll('button').find(button => button.text().includes('courses.quiz.submit'))!.trigger('click')
    await flushPromises()
    expect(wrapper.emitted('complete')?.[0]?.[0]).toMatchObject({ score: 1, earned_points: 1, total_points: 1, passed: true })
    wrapper.unmount()
  })

  it('rejects invalid point weights without emitting a completion', async () => {
    const wrapper = render(-1)
    await flushPromises()
    expect(wrapper.text()).toContain('courses.quiz.parseError')
    expect(wrapper.emitted('complete')).toBeUndefined()
    wrapper.unmount()
  })

  it('allows a failed attempt to be retried only before course completion', async () => {
    const wrapper = render()
    await flushPromises()
    await wrapper.findAll('button').find(button => button.text().includes('DELETE'))!.trigger('click')
    await wrapper.findAll('button').find(button => button.text().includes('courses.quiz.submit'))!.trigger('click')
    await flushPromises()
    expect(wrapper.text()).toContain('common.actions.retry')
    await wrapper.setProps({ readOnly: true })
    expect(wrapper.text()).not.toContain('common.actions.retry')
    wrapper.unmount()
  })
})
