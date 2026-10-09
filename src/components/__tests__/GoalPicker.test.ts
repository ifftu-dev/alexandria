import { beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import { createI18n } from 'vue-i18n'
import GoalPicker from '../goals/GoalPicker.vue'
import LearningDecisionReview from '../LearningDecisionReview.vue'
import en from '@/locales/en'
import type { GoalResolution, GoalTemplate } from '@/types'

const { listGoalTemplates, resolveGoal, addGoal, locked } = vi.hoisted(() => ({ listGoalTemplates: vi.fn(), resolveGoal: vi.fn(), addGoal: vi.fn(), locked: { callback: () => {} } }))
vi.mock('@/composables/useGoals', () => ({ useGoals: () => ({ listGoalTemplates, resolveGoal, addGoal }) }))
vi.mock('@/composables/useProfiles', () => ({ onProfileLocked: (callback: () => void) => { locked.callback = callback; return () => {} } }))
const render = () => mount(GoalPicker, { global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en } })], stubs: { LearningDecisionReview: true } } })
const result: GoalResolution = { label: 'Learn Rust.', goal_skill_ids: [], suggestions: [{ skill_id: 'skill_rust', name: 'Rust', matched: 'Rust', score: 0.8 }], resolution_provenance: 'goal_parsed' }
const button = (wrapper: ReturnType<typeof render>, text: string) => wrapper.findAll('button').find(b => b.text() === text)!
beforeEach(() => { listGoalTemplates.mockReset().mockResolvedValue([]); resolveGoal.mockReset(); addGoal.mockReset() })

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

const role: GoalTemplate = { id: 'gt_role_fe', kind: 'job_role', key: 'frontend_engineer', label: 'Frontend Engineer', skill_ids: ['skill_javascript'], taxonomy_version: 'bundled', ratified: true }
describe('offline goal choices', () => {
  it('loads the default role choices and adds only the selected goal', async () => {
    listGoalTemplates.mockResolvedValue([role])
    resolveGoal.mockResolvedValue({ label: role.label, goal_skill_ids: role.skill_ids, suggestions: [], taxonomy_version: 'bundled', resolution_provenance: 'template' })
    const wrapper = render(); await flushPromises()
    expect(listGoalTemplates).toHaveBeenCalledWith('job_role')
    expect(wrapper.findAll('option').map(option => option.text())).toContain('Frontend Engineer')
    expect(addGoal).not.toHaveBeenCalled()
    await wrapper.get('select').setValue(role.key)
    await button(wrapper, 'Set as goal').trigger('click'); await flushPromises()
    expect(resolveGoal).toHaveBeenCalledWith({ kind: 'job_role', key: role.key })
    expect(addGoal).toHaveBeenCalledWith(expect.objectContaining({ kind: 'job_role', sourceKey: role.key, goalSkillIds: role.skill_ids }))
    wrapper.unmount()
  })
  it('shows a failed load and retries instead of leaving an empty dropdown', async () => {
    listGoalTemplates.mockRejectedValueOnce(new Error('Database temporarily unavailable'))
    const wrapper = render(); await flushPromises()
    expect(wrapper.get('[role=alert]').text()).toContain('Database temporarily unavailable')
    expect(wrapper.find('select').exists()).toBe(false)
    listGoalTemplates.mockResolvedValue([role])
    await button(wrapper, 'Retry').trigger('click'); await flushPromises()
    expect(wrapper.find('select').exists()).toBe(true)
    expect(wrapper.find('[role=alert]').exists()).toBe(false)
    wrapper.unmount()
  })
  it('discards a late category response and handles an empty category', async () => {
    let finish!: (templates: GoalTemplate[]) => void
    listGoalTemplates.mockReturnValueOnce(new Promise<GoalTemplate[]>(resolve => { finish = resolve }))
    const wrapper = render(); await flushPromises()
    expect(wrapper.get('[role=status]').text()).toBe('Loading…')
    await button(wrapper, 'Exam').trigger('click'); await flushPromises()
    expect(wrapper.text()).toContain('No goal options are available')
    finish([role]); await flushPromises()
    expect(wrapper.find('select').exists()).toBe(false)
    expect(wrapper.text()).not.toContain(role.label)
    wrapper.unmount()
  })
})
