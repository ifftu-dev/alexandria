import { mount } from '@vue/test-utils'
import { afterEach, expect, test, vi } from 'vitest'
import { computed, nextTick, ref } from 'vue'
import { i18n } from '@/i18n'
import type { OmniSearchResult } from '@/composables/useOmniSearch'

// Grouped results and empty-query recents share one row template: the same
// press feedback, selection, pointer handling and accessible text.

const state = vi.hoisted(() => ({ selectAt: vi.fn((_index: number) => null as string | null) }))
const query = ref('')
const items = ref<OmniSearchResult[]>([])
const selectedIndex = ref(0)

vi.mock('vue-router', () => ({ useRouter: () => ({ push: vi.fn() }) }))
vi.mock('@/composables/useKeyboardShortcuts', () => ({ useKeyboardShortcuts: () => ({ registerAction: () => {} }) }))
vi.mock('@/composables/useOmniSearch', () => ({
  useOmniSearch: () => ({
    isOpen: ref(true),
    query,
    loading: ref(false),
    selectedIndex,
    visibleItems: computed(() => items.value),
    groupedItems: computed(() => (['skill', 'course'] as const)
      .map(type => ({ type, label: type === 'skill' ? 'Skills' : 'Courses', items: items.value.filter(item => item.type === type) }))
      .filter(group => group.items.length)),
    open: () => {}, close: () => {}, setQuery: () => {}, navigate: () => {}, select: () => null,
    selectAt: state.selectAt,
  }),
}))

const sorting: OmniSearchResult = { id: 'c1', type: 'course', title: 'Sorting algorithms', subtitle: 'Algorithms foundations', route: '/courses/c1' }
const recursion: OmniSearchResult = { id: 's1', type: 'skill', title: 'Recursion', route: '/skills/s1' }

async function mountSearch() {
  const { default: OmniSearch } = await import('./OmniSearch.vue')
  const wrapper = mount(OmniSearch, { global: { plugins: [i18n], stubs: { teleport: true, transition: false, 'transition-group': false } } })
  await nextTick()
  return wrapper
}

afterEach(() => { state.selectAt.mockClear(); selectedIndex.value = 0 })

function rows(wrapper: Awaited<ReturnType<typeof mountSearch>>) {
  return wrapper.findAll('[role="dialog"] button').filter(button => button.attributes('data-omni-selected') !== undefined)
}

test('recents render as selectable rows under the Recent heading', async () => {
  query.value = ''
  items.value = [sorting, recursion]
  const wrapper = await mountSearch()
  expect(wrapper.text()).toContain('Recent')
  const [first, second] = rows(wrapper)
  expect(first!.text()).toContain('Sorting algorithms')
  expect(first!.text()).toContain('Algorithms foundations')
  for (const row of [first!, second!]) expect(row.classes()).toEqual(expect.arrayContaining(['pressable', '*:pointer-events-none']))
  expect(first!.attributes('data-omni-selected')).toBe('true')
  await second!.trigger('mousemove')
  expect(second!.attributes('data-omni-selected')).toBe('true')
  await second!.trigger('click')
  expect(state.selectAt).toHaveBeenCalledWith(1)
})

test('results render grouped by type and select by their place in the whole list', async () => {
  query.value = 'sort'
  items.value = [recursion, sorting]
  const wrapper = await mountSearch()
  expect(wrapper.text()).not.toContain('Recent')
  expect(wrapper.text()).toContain('Skills')
  expect(wrapper.text()).toContain('Courses')
  const [skill, course] = rows(wrapper)
  expect(skill!.text()).toContain('Recursion')
  expect(course!.text()).toContain('Sorting algorithms')
  for (const row of [skill!, course!]) expect(row.classes()).toEqual(expect.arrayContaining(['pressable', '*:pointer-events-none']))
  await course!.trigger('mousemove')
  expect(course!.attributes('data-omni-selected')).toBe('true')
  expect(skill!.attributes('data-omni-selected')).toBe('false')
  await course!.trigger('click')
  expect(state.selectAt).toHaveBeenCalledWith(1)
})
