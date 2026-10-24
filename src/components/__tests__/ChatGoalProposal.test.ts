import { beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import { computed, ref } from 'vue'
import { createI18n } from 'vue-i18n'
import ChatGoalProposal from '../chat/ChatGoalProposal.vue'
import en from '@/locales/en'
import type { ChatArtifact, Goal } from '@/types'

const { resolveGoal, addGoal, goalAdded, deleteArtifact, push } = vi.hoisted(() => ({
  resolveGoal: vi.fn(),
  addGoal: vi.fn(),
  goalAdded: vi.fn(),
  deleteArtifact: vi.fn(),
  push: vi.fn(),
}))
const goalList = ref<Goal[]>([])
vi.mock('@/composables/useGoals', () => ({ useGoals: () => ({ goals: computed(() => goalList.value), resolveGoal, addGoal }) }))
vi.mock('@/composables/useChat', () => ({ useChat: () => ({ goalAdded, deleteArtifact }) }))
vi.mock('vue-router', () => ({ useRouter: () => ({ push }) }))

const proposal = (over: Partial<ChatArtifact> = {}): ChatArtifact => ({
  id: 'a1',
  status: 'proposed',
  title: 'Data analyst',
  body: { kind: 'goal', text: 'Clean data and build dashboards' },
  sources: [],
  thread_id: 't',
  message_id: 'm',
  created_at: '2026-10-24T00:00:00Z',
  updated_at: '2026-10-24T00:00:00Z',
  ...over,
})
const render = (item: ChatArtifact) =>
  mount(ChatGoalProposal, { props: { item }, global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en } })] } })
const button = (wrapper: ReturnType<typeof render>, text: string) => wrapper.findAll('button').find((b) => b.text() === text)

beforeEach(() => {
  for (const f of [resolveGoal, addGoal, goalAdded, deleteArtifact, push]) f.mockReset()
  goalList.value = []
})

describe('a goal proposed in chat', () => {
  it('adds nothing until the learner picks skills, then adds through the Goals service', async () => {
    resolveGoal.mockResolvedValue({
      label: 'x',
      goal_skill_ids: [],
      suggestions: [
        { skill_id: 'sk-sql', name: 'SQL', matched: 'data', score: 0.8 },
        { skill_id: 'sk-viz', name: 'Charts', matched: 'dashboards', score: 0.3 },
      ],
      resolution_provenance: 'goal_parsed',
    })
    addGoal.mockResolvedValue({ id: 'g9' })
    const wrapper = render(proposal())
    expect(addGoal).not.toHaveBeenCalled()
    await button(wrapper, 'Find skills')!.trigger('click')
    await flushPromises()
    expect(resolveGoal).toHaveBeenCalledWith({ kind: 'learning_goal', text: 'Clean data and build dashboards' })
    const boxes = wrapper.findAll('input[type="checkbox"]')
    expect(boxes.map((b) => (b.element as HTMLInputElement).checked)).toEqual([true, false])
    await boxes[1]!.setValue(true)
    await button(wrapper, 'Add goal with 2 skills')!.trigger('click')
    await flushPromises()
    expect(addGoal).toHaveBeenCalledWith({
      label: 'Data analyst',
      goalSkillIds: ['sk-sql', 'sk-viz'],
      kind: 'learning_goal',
      resolutionProvenance: 'goal_parsed',
    })
    expect(goalAdded).toHaveBeenCalledWith('a1', 'g9')
  })

  it('points to the Goals page when no skill matches', async () => {
    resolveGoal.mockResolvedValue({ label: 'x', goal_skill_ids: [], suggestions: [], resolution_provenance: 'goal_parsed' })
    const wrapper = render(proposal())
    await button(wrapper, 'Find skills')!.trigger('click')
    await flushPromises()
    expect(wrapper.text()).toContain('No matching skills were found')
    await button(wrapper, 'Open Goals')!.trigger('click')
    expect(push).toHaveBeenCalledWith('/goals')
    expect(addGoal).not.toHaveBeenCalled()
  })

  it('shows whether an added goal is still among the learner goals', async () => {
    const added = proposal({ status: 'added', body: { kind: 'goal', text: 't', goal_id: 'g9' } })
    goalList.value = [{ id: 'g9', label: 'Data analyst', goal_skill_ids: ['sk-sql'], created_at: '' }]
    const wrapper = render(added)
    expect(wrapper.text()).toContain('Added to your goals · Data analyst')
    goalList.value = []
    await flushPromises()
    expect(wrapper.text()).toContain('Added to your goals, then removed')
    expect(button(wrapper, 'Open')).toBeUndefined()
  })
})
