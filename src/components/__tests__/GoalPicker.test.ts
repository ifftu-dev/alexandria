import { beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import { createI18n } from 'vue-i18n'
import GoalPicker from '../goals/GoalPicker.vue'
import LearningDecisionReview from '../LearningDecisionReview.vue'
import en from '@/locales/en'
import type { GoalResolution } from '@/types'

const { resolveGoal, addGoal, locked } = vi.hoisted(() => ({ resolveGoal: vi.fn(), addGoal: vi.fn(), locked: { callback: () => {} } }))
vi.mock('@/composables/useGoals', () => ({ useGoals: () => ({ listGoalTemplates: async () => [], resolveGoal, addGoal }) }))
vi.mock('@/composables/useProfiles', () => ({ onProfileLocked: (callback: () => void) => { locked.callback = callback; return () => {} } }))
const render = () => mount(GoalPicker, { global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en } })], stubs: { LearningDecisionReview: true } } })
const result: GoalResolution = { label: 'Learn Rust.', goal_skill_ids: [], suggestions: [{ skill_id: 'skill_rust', name: 'Rust', matched: 'Rust', score: 0.8 }], resolution_provenance: 'goal_parsed' }
const button = (wrapper: ReturnType<typeof render>, text: string) => wrapper.findAll('button').find(b => b.text() === text)!
beforeEach(() => { resolveGoal.mockReset(); addGoal.mockReset() })

describe('learning goal entry', () => {
  it('uses the goal task and requires selection after adding a Jev suggestion', async () => {
    const wrapper = render(); await flushPromises()
    await button(wrapper, 'Learning goal').trigger('click')
    await wrapper.get('textarea').setValue('Learn Rust.')
    const review = wrapper.getComponent(LearningDecisionReview)
    expect(review.props('task')).toBe('learning_goal')
    review.vm.$emit('suggestion', { skill_id: 'skill_rust', name: 'Rust', matched: 'Rust', score: 0 })
    await flushPromises()
    expect(addGoal).not.toHaveBeenCalled()
    const checkbox = wrapper.get('input[type="checkbox"]')
    expect((checkbox.element as HTMLInputElement).checked).toBe(false)
    await checkbox.setValue(true)
    await button(wrapper, 'Add 1 skill as goal').trigger('click'); await flushPromises()
    expect(addGoal).toHaveBeenCalledWith(expect.objectContaining({ label: 'Learn Rust.', kind: 'learning_goal', resolutionProvenance: 'goal_parsed', goalSkillIds: ['skill_rust'] }))
    wrapper.unmount()
  })
  it('keeps local matching and discards stale local results', async () => {
    const wrapper = render(); await flushPromises()
    await button(wrapper, 'Learning goal').trigger('click')
    await wrapper.get('textarea').setValue('Learn Rust.')
    let finish!: (r: GoalResolution) => void
    resolveGoal.mockReturnValue(new Promise<GoalResolution>(resolve => { finish = resolve }))
    await button(wrapper, 'Find skills').trigger('click')
    expect(resolveGoal).toHaveBeenCalledWith({ kind: 'learning_goal', text: 'Learn Rust.' })
    await wrapper.get('textarea').setValue('Learn SQL.')
    finish(result); await flushPromises()
    expect(wrapper.findAll('input[type="checkbox"]')).toHaveLength(0)
    resolveGoal.mockResolvedValue(result)
    await button(wrapper, 'Find skills').trigger('click'); await flushPromises()
    expect(wrapper.findAll('input[type="checkbox"]')).toHaveLength(1)
    locked.callback(); await flushPromises()
    expect(wrapper.findAll('input[type="checkbox"]')).toHaveLength(0)
    expect((wrapper.get('textarea').element as HTMLTextAreaElement).value).toBe('')
    wrapper.unmount()
  })
})
