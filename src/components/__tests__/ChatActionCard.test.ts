import { beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import { computed, ref } from 'vue'
import { createI18n } from 'vue-i18n'
import ChatActionCard from '../chat/ChatActionCard.vue'
import en from '@/locales/en'
import type { ChatArtifact, ChatArtifactBody, Goal } from '@/types'

const { resolveGoal, allowAction, declineAction, push } = vi.hoisted(() => ({
  resolveGoal: vi.fn(),
  allowAction: vi.fn(),
  declineAction: vi.fn(),
  push: vi.fn(),
}))
const goalList = ref<Goal[]>([])
vi.mock('@/composables/useGoals', () => ({ useGoals: () => ({ goals: computed(() => goalList.value), resolveGoal }) }))
vi.mock('@/composables/useChat', async (original) => ({
  ...(await original<typeof import('@/composables/useChat')>()),
  useChat: () => ({ allowAction, declineAction }),
}))
vi.mock('vue-router', () => ({ useRouter: () => ({ push }) }))

const item = (body: ChatArtifactBody, over: Partial<ChatArtifact> = {}): ChatArtifact => ({
  id: 'a1',
  status: 'proposed',
  title: 'Bayes rule',
  body,
  sources: [],
  thread_id: 't',
  message_id: 'm',
  created_at: '2026-10-24T00:00:00Z',
  updated_at: '2026-10-24T00:00:00Z',
  ...over,
})
const render = (artifact: ChatArtifact) =>
  mount(ChatActionCard, { props: { item: artifact }, global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en } })] } })
const button = (wrapper: ReturnType<typeof render>, text: string) => wrapper.findAll('button').find((b) => b.text() === text)
const goal = (over: Partial<Goal> = {}): Goal => ({
  id: 'g1',
  label: 'Data analyst',
  source_did: null,
  goal_skill_ids: ['sk-bayes'],
  created_at: '2026-10-01',
  ...over,
})

beforeEach(() => {
  for (const f of [resolveGoal, allowAction, declineAction, push]) f.mockReset()
  goalList.value = []
})

describe('something the chat asks to save or change', () => {
  it('asks first, and writes only through Allow', async () => {
    const wrapper = render(item({ kind: 'course_note', course_id: 'c', element_id: 'l', lesson: 'Conditional probability', text: 'Update a prior.' }))
    expect(wrapper.text()).toContain('The chat is asking to')
    expect(wrapper.text()).toContain('Add a note to “Conditional probability”')
    expect(wrapper.text()).toContain('Nothing is saved unless you allow it.')
    expect(allowAction).not.toHaveBeenCalled()
    await button(wrapper, 'Allow')!.trigger('click')
    expect(allowAction).toHaveBeenCalledWith('a1', undefined)
  })

  it('records a decline without writing anything', async () => {
    const wrapper = render(item({ kind: 'note', text: 'Prior: belief before evidence' }))
    await button(wrapper, "Don't allow")!.trigger('click')
    expect(declineAction).toHaveBeenCalledWith('a1')
    expect(allowAction).not.toHaveBeenCalled()
    const declined = render(item({ kind: 'note', text: 'x' }, { status: 'declined' }))
    expect(declined.text()).toContain('Not allowed')
    expect(declined.find('button').exists()).toBe(false)
  })

  it('adds a goal only with the skills the learner picked', async () => {
    resolveGoal.mockResolvedValue({
      label: 'x',
      goal_skill_ids: [],
      suggestions: [
        { skill_id: 'sk-sql', name: 'SQL', matched: 'data', score: 0.8 },
        { skill_id: 'sk-viz', name: 'Charts', matched: 'dashboards', score: 0.3 },
      ],
      resolution_provenance: 'goal_parsed',
    })
    const wrapper = render(item({ kind: 'goal', text: 'Clean data and build dashboards' }, { title: 'Data analyst' }))
    await flushPromises()
    expect(resolveGoal).toHaveBeenCalledWith({ kind: 'learning_goal', text: 'Clean data and build dashboards' })
    const boxes = wrapper.findAll('input[type="checkbox"]')
    expect(boxes.map((b) => (b.element as HTMLInputElement).checked)).toEqual([true, false])
    await boxes[0]!.trigger('change')
    expect(button(wrapper, 'Allow')!.attributes('disabled')).toBeDefined()
    expect(wrapper.text()).toContain('Pick at least one skill to allow this.')
    await boxes[1]!.trigger('change')
    await button(wrapper, 'Allow')!.trigger('click')
    expect(allowAction).toHaveBeenCalledWith('a1', ['sk-viz'])
  })

  it('shows a goal change against the goal as it is now and never empties it', async () => {
    goalList.value = [goal()]
    const change = { kind: 'goal_change' as const, goal_id: 'g1', add: [{ id: 'sk-sql', name: 'SQL' }], remove: [], text: 'Querying first.' }
    const wrapper = render(item(change, { title: 'Data analyst' }))
    expect(wrapper.text()).toContain('Change your goal “Data analyst”')
    expect(wrapper.text()).toContain('+ SQL')

    const emptying = render(item({ ...change, add: [], remove: [{ id: 'sk-bayes', name: 'Bayes rule' }] }))
    expect(emptying.text()).toContain("can't be applied")
    expect(button(emptying, 'Allow')!.attributes('disabled')).toBeDefined()

    goalList.value = []
    await flushPromises()
    expect(wrapper.text()).toContain('no longer in your goals')
  })

  it('explains a refusal and points to the course', async () => {
    allowAction.mockRejectedValue(new Error('needs_plugins: open the course to install what it needs'))
    const wrapper = render(item({ kind: 'enroll', course_id: 'c-sql', goal_id: 'g1', text: 'Covers joins.' }, { title: 'Intro to SQL' }))
    expect(wrapper.text()).toContain('Enroll you in a course')
    await button(wrapper, 'Allow')!.trigger('click')
    await flushPromises()
    expect(wrapper.text()).toContain('Finish enrolling on the course page.')
    await button(wrapper, 'Open course')!.trigger('click')
    expect(push).toHaveBeenCalledWith('/courses/c-sql')
  })

  it('becomes a one-line record once allowed', async () => {
    goalList.value = [goal()]
    const wrapper = render(item({ kind: 'goal', text: 'x', goal_id: 'g1' }, { status: 'added', title: 'Data analyst' }))
    expect(wrapper.text()).toContain('Added to your goals')
    await button(wrapper, 'Open')!.trigger('click')
    expect(push).toHaveBeenCalledWith('/goals')
    goalList.value = []
    await flushPromises()
    expect(wrapper.text()).toContain('then removed')
  })
})
