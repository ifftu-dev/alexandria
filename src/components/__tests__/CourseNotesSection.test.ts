import { beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import { ref } from 'vue'
import { createI18n } from 'vue-i18n'
import CourseNotesSection from '../course/CourseNotesSection.vue'
import en from '@/locales/en'
import type { CourseNote } from '@/types'

const { list, remove } = vi.hoisted(() => ({ list: vi.fn(), remove: vi.fn() }))
const revision = ref(0)
vi.mock('@/composables/useCourseNotes', () => ({ useCourseNotes: () => ({ revision, list, remove }) }))

const note = (id: string, element: string | null, title: string): CourseNote => ({
  id,
  course_id: 'course',
  element_id: element,
  element_title: element ? `Lesson ${element}` : null,
  title,
  body: `${title} body`,
  origin: 'chat',
  created_at: '',
  updated_at: '',
})
const render = (elementId: string | null) =>
  mount(CourseNotesSection, {
    props: { courseId: 'course', elementId },
    global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en } })], stubs: { ConfirmDialog: true } },
  })

beforeEach(() => {
  list.mockReset()
  remove.mockReset()
})

describe('course notes in the player', () => {
  it('is absent without notes', async () => {
    list.mockResolvedValue([])
    const wrapper = render('a')
    await flushPromises()
    expect(wrapper.find('section').exists()).toBe(false)
  })

  it('lists the current lesson first and refreshes when notes change elsewhere', async () => {
    list.mockResolvedValue([note('n2', 'b', 'Other'), note('n1', 'a', 'Here')])
    const wrapper = render('a')
    await flushPromises()
    const titles = wrapper.findAll('button[aria-expanded]').map((b) => b.text())
    expect(titles[0]).toContain('Here')
    expect(titles[1]).toContain('Other')
    expect(wrapper.text()).toContain('This lesson')
    await wrapper.findAll('button[aria-expanded]')[1]!.trigger('click')
    await wrapper.findAll('button').find((b) => b.text() === 'Open lesson')!.trigger('click')
    expect(wrapper.emitted('open')).toEqual([['b']])

    list.mockResolvedValue([])
    revision.value++
    await flushPromises()
    expect(wrapper.find('section').exists()).toBe(false)
  })
})
