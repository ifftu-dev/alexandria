import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import { createI18n } from 'vue-i18n'
import LearnerTutor from '../LearnerTutor.vue'
import en from '@/locales/en'
import type { StudioConnection, StudioDocument, TutorReply } from '@/types'

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }))
vi.mock('@/composables/useLocalApi', () => ({ useLocalApi: () => ({ invoke }) }))

const connection: StudioDocument<StudioConnection> = {
  id: 'cloud',
  revision: 1,
  value: {
    id: 'cloud',
    name: 'My provider',
    endpoint: 'https://provider.example/v1',
    model: 'tutor-model',
    location: 'cloud',
    capability: 'text',
    enabled: true,
    has_key: true,
  },
}
const emptyReply: TutorReply = {
  provider_location: 'cloud',
  thread: {
    id: 'thread',
    course_id: 'course',
    element_id: 'lesson',
    connection_id: 'cloud',
    messages: [],
  },
}
const mounted: ReturnType<typeof mount>[] = []

function render() {
  const wrapper = mount(LearnerTutor, {
    props: { courseId: 'course', elementId: 'lesson' },
    global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en } })] },
  })
  mounted.push(wrapper)
  return wrapper
}

beforeEach(() => invoke.mockReset())
afterEach(() => {
  for (const wrapper of mounted.splice(0)) wrapper.unmount()
  document.body.innerHTML = ''
})

describe('learner BYOM tutor', () => {
  it('does not render when the instructor disabled the tutor', async () => {
    invoke.mockImplementation((command: string) => {
      if (!command) return Promise.resolve(undefined)
      if (command === 'studio_get_tutor_policy') return Promise.resolve({ enabled: false, guidance: 'socratic', initial_prompt: '' })
      if (command === 'studio_list_connections') return Promise.resolve([])
      return Promise.reject(new Error(`Unexpected command ${command}`))
    })
    const wrapper = render()
    await flushPromises()
    expect(wrapper.text()).toBe('')
  })

  it('discloses cloud context and sends only after the learner submits', async () => {
    invoke.mockImplementation((command: string) => {
      if (!command) return Promise.resolve(undefined)
      if (command === 'studio_get_tutor_policy') return Promise.resolve({ enabled: true, guidance: 'balanced', initial_prompt: 'Use short examples.' })
      if (command === 'studio_list_connections') return Promise.resolve([connection])
      if (command === 'studio_get_tutor_thread') return Promise.resolve(emptyReply)
      if (command === 'studio_ask_tutor') return Promise.resolve({
        ...emptyReply,
        thread: {
          ...emptyReply.thread,
          messages: [
            { role: 'learner', text: 'Give me a hint', created_at: '2026-09-15T00:00:00Z' },
            { role: 'tutor', text: 'Start with the first definition.', created_at: '2026-09-15T00:00:01Z' },
          ],
        },
      })
      return Promise.reject(new Error(`Unexpected command ${command}`))
    })
    const wrapper = render()
    await flushPromises()
    expect(invoke.mock.calls.map(call => call[0])).toEqual([
      'studio_get_tutor_policy',
      'studio_list_connections',
      'studio_get_tutor_thread',
    ])
    await wrapper.get('button').trigger('click')
    await flushPromises()
    expect(document.body.textContent).toContain('Lesson text, conversation, and your question are sent to My provider')
    const textarea = document.body.querySelector('textarea') as HTMLTextAreaElement
    textarea.value = 'Give me a hint'
    textarea.dispatchEvent(new Event('input', { bubbles: true }))
    const form = textarea.closest('form')!
    form.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }))
    await flushPromises()
    expect(invoke).toHaveBeenCalledWith('studio_ask_tutor', {
      courseId: 'course',
      elementId: 'lesson',
      connectionId: 'cloud',
      question: 'Give me a hint',
    })
    expect(document.body.textContent).toContain('Start with the first definition.')
  })
})
