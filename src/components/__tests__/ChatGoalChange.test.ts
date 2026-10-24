import { beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import { computed, ref } from 'vue'
import { createI18n } from 'vue-i18n'
import ChatGoalChange from '../chat/ChatGoalChange.vue'
import en from '@/locales/en'
import type { ChatArtifact, Goal } from '@/types'

const { updateGoal, goalChanged, deleteArtifact } = vi.hoisted(() => ({ updateGoal: vi.fn(), goalChanged: vi.fn(), deleteArtifact: vi.fn() }))
const goalList = ref<Goal[]>([])
vi.mock('@/composables/useGoals', () => ({ useGoals: () => ({ goals: computed(() => goalList.value), updateGoal }) }))
vi.mock('@/composables/useChat', () => ({ useChat: () => ({ goalChanged, deleteArtifact }) }))
vi.mock('vue-router', () => ({ useRouter: () => ({ push: vi.fn() }) }))

const change: ChatArtifact = {
  id: 'c1',
  status: 'proposed',
  title: 'Data analyst',
  body: { kind: 'goal_change', goal_id: 'g1', add: [{ id: 'sk-sql', name: 'SQL' }], remove: [{ id: 'sk-bayes', name: 'Bayes rule' }], text: 'Focus on querying.' },
  sources: [],
  thread_id: 't',
  message_id: 'm',
  created_at: '',
  updated_at: '',
}
const render = () => mount(ChatGoalChange, { props: { item: change }, global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en } })] } })
const button = (wrapper: ReturnType<typeof render>, text: string) => wrapper.findAll('button').find((b) => b.text() === text)

beforeEach(() => {
  for (const f of [updateGoal, goalChanged, deleteArtifact]) f.mockReset()
  goalList.value = [{ id: 'g1', label: 'Data analyst', goal_skill_ids: ['sk-bayes', 'sk-count'], created_at: '' }]
})

describe('a goal change proposed in chat', () => {
  it('shows the difference against the goal now and applies it through the Goals service', async () => {
    const wrapper = render()
    expect(wrapper.text()).toContain('+ SQL')
    expect(wrapper.text()).toContain('− Bayes rule')
    expect(updateGoal).not.toHaveBeenCalled()
    await button(wrapper, 'Apply change')!.trigger('click')
    await flushPromises()
    expect(updateGoal).toHaveBeenCalledWith('g1', { label: undefined, goalSkillIds: ['sk-count', 'sk-sql'] })
    expect(goalChanged).toHaveBeenCalledWith('c1')
  })

  it('cannot be applied once the goal is gone or would be left empty', async () => {
    goalList.value = [{ id: 'g1', label: 'Data analyst', goal_skill_ids: ['sk-bayes'], created_at: '' }]
    const change2 = { ...change, body: { ...change.body, add: [] } } as ChatArtifact
    const wrapper = mount(ChatGoalChange, { props: { item: change2 }, global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en } })] } })
    expect(wrapper.text()).toContain('without skills')
    expect(button(wrapper, 'Apply change')).toBeUndefined()
    goalList.value = []
    await flushPromises()
    expect(wrapper.text()).toContain('no longer in your goals')
  })
})
