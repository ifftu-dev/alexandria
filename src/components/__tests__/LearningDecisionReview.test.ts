import { beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import { createI18n } from 'vue-i18n'
import LearningDecisionReview from '../LearningDecisionReview.vue'
import en from '@/locales/en'
import type { LearningDecisionReview as Review } from '@/types'

const { invoke, locked } = vi.hoisted(() => ({ invoke: vi.fn(), locked: { callback: () => {} } }))
vi.mock('@/composables/useLocalApi', () => ({ useLocalApi: () => ({ invoke }) }))
vi.mock('@/composables/useProfiles', () => ({ onProfileLocked: (callback: () => void) => { locked.callback = callback; return () => {} } }))

const review: Review = {
  status: 'completed', names: { skill_rust: 'Rust' }, evidence: { skill_rust: 'Use Rust.' },
  record: { schema_version: 1, task: 'job_description', taxonomy_digest: 'digest', taxonomy_revision: 'v1', source_hash: 'source', model: 'jev-1.13.0', rubric_version: 'learning-v1', decisions: [{ skill_id: 'skill_rust', relation: { value: 'required', confidence: 0.9, probabilities: { required: 1 } }, bloom: { value: 'apply', confidence: 0.9, probabilities: { apply: 1 } }, evidence: { start: 0, end: 9 } }] },
}
const render = () => mount(LearningDecisionReview, { props: { source: 'Use Rust.', task: 'job_description' }, global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en } })] } })
beforeEach(() => invoke.mockReset())

describe('optional learning checks', () => {
  it('requires an explicit request and a separate selection', async () => {
    invoke.mockResolvedValue(review)
    const wrapper = render()
    expect(invoke).not.toHaveBeenCalled()
    await wrapper.get('button').trigger('click'); await flushPromises()
    expect(wrapper.text()).toContain('Use Rust.')
    expect(wrapper.emitted('suggestion')).toBeUndefined()
    await wrapper.findAll('button')[1]!.trigger('click')
    expect(wrapper.emitted('suggestion')?.[0]?.[0]).toMatchObject({ skill_id: 'skill_rust', score: 0 })
    wrapper.unmount()
  })
  it('discards a response after its source or profile changes', async () => {
    for (const changeProfile of [false, true]) {
      let resolve!: (value: Review) => void
      invoke.mockReturnValue(new Promise<Review>(done => { resolve = done }))
      const wrapper = render()
      await wrapper.get('button').trigger('click')
      if (changeProfile) locked.callback()
      else await wrapper.setProps({ source: 'Use SQL.' })
      resolve(review); await flushPromises()
      expect(wrapper.text()).not.toContain('Use Rust.')
      expect(wrapper.emitted('suggestion')).toBeUndefined()
      wrapper.unmount()
    }
  })
  it('shadow completion offers no model-derived selection', async () => {
    invoke.mockResolvedValue({ status: 'shadow_complete', record: null, evidence: {}, names: {} })
    const wrapper = render()
    await wrapper.get('button').trigger('click'); await flushPromises()
    expect(wrapper.findAll('button')).toHaveLength(1)
    expect(wrapper.text()).toContain('local suggestions are unchanged')
    wrapper.unmount()
  })
})
