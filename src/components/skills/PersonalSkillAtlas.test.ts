import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { defineComponent, h } from 'vue'
import { createI18n } from 'vue-i18n'
import { createMemoryHistory, createRouter, RouterView } from 'vue-router'
import en from '@/locales/en'
import { atlasBackend } from '../../../tests/desktop-ui/atlasBackend'

const ipc = vi.hoisted(() => vi.fn())
vi.mock('@tauri-apps/api/core', () => ({ invoke: ipc, transformCallback: () => 1 }))
let wrapper: VueWrapper | undefined
beforeEach(() => { vi.resetModules(); ipc.mockReset(); Object.defineProperty(window, '__TAURI_INTERNALS__', { configurable: true, value: { invoke: ipc, transformCallback: () => 1, unregisterCallback: () => {} } }) })
afterEach(() => { wrapper?.unmount(); document.body.innerHTML = ''; sessionStorage.clear() })
function button(name: string, root: ParentNode = document) {
  const target = [...root.querySelectorAll('button')].find(el => (el.getAttribute('aria-label') ?? el.textContent?.trim()) === name)
  if (!target) throw new Error(`Missing button ${name}`)
  return target
}
async function click(name: string, root: ParentNode = document) { button(name, root).click(); await flushPromises() }
async function setup() {
  const backend = atlasBackend(true)
  ipc.mockImplementation(backend.invoke)
  const { useProfiles } = await import('@/composables/useProfiles')
  const { useSkillAtlas } = await import('@/composables/useSkillAtlas')
  const { default: Page } = await import('@/pages/skills/Index.vue')
  const { default: Detail } = await import('@/pages/skills/Detail.vue')
  const { default: Sidebar } = await import('@/components/layout/SidebarSkillGraph.vue')
  const { default: Host } = await import('@/components/skills/SkillAtlasHost.vue')
  const router = createRouter({ history: createMemoryHistory(), routes: [
    { path: '/courses', component: (await import('@/pages/courses/Index.vue')).default },
    { path: '/courses/:id', component: (await import('@/pages/courses/Detail.vue')).default },
    { path: '/skills', component: Page }, { path: '/skills/:id', component: Detail }, { path: '/goals', component: (await import('@/pages/goals/Index.vue')).default }, { path: '/other', component: { template: '<p>Other</p>' } },
  ] })
  await useProfiles().initialize()
  await router.push('/skills')
  const atlas = useSkillAtlas()
  const Root = defineComponent({ setup() { return () => h('div', [h(Host), h(Sidebar), h(RouterView)]) } })
  wrapper = mount(Root, { attachTo: document.body, global: { plugins: [router, createI18n({ legacy: false, locale: 'en', messages: { en } })] } })
  await flushPromises()
  return { backend, router, atlas, profiles: useProfiles() }
}
it('keeps browse filters separate and restores graph selection, camera and controls after opening a real skill detail', async () => {
  const { atlas, router, backend } = await setup()
  await click('Browse Skills')
  await wrapper!.get('input').setValue('impossible browse query')
  await click('My Graph')
  expect(wrapper!.text()).toContain('Visibility & teaching')
  expect(wrapper!.findAll('[aria-label="Full-label skill list"] button')).toHaveLength(104)
  await wrapper!.get('input[type="search"]').setValue('Big-O')
  await click('Big-O Analysis · 2 Understand')
  await click('Zoom in')
  await click('Focus dependents')
  const saved = structuredClone(JSON.parse(JSON.stringify(atlas.pageView.value)))
  expect(router.currentRoute.value.path).toBe('/skills')
  await click('Open skill')
  expect(router.currentRoute.value.path).toBe('/skills/skill_big_o')
  expect(wrapper!.text()).toContain('Big-O Analysis')
  await click('Back to all skills')
  expect(atlas.pageView.value).toEqual(saved)
  expect(wrapper!.get<HTMLInputElement>('input[type="search"]').element.value).toBe('Big-O')
  expect(wrapper!.get('[aria-label="Skill inspector"]').text()).toContain('2 Understand')
  expect(backend.state.unexpected).toEqual([])
})

it('returns to the expanded context once, releases scroll locks and does not reopen on a later visit', async () => {
  const { atlas, router, backend } = await setup()
  await router.push('/other')
  await flushPromises()
  atlas.setModalOpen(true)
  await flushPromises()
  const dialog = document.querySelector('[role="dialog"]')!
  expect(dialog).not.toBeNull()
  const input = dialog.querySelector('input[type="search"]') as HTMLInputElement
  input.value = 'Big-O'
  input.dispatchEvent(new Event('input', { bubbles: true }))
  await flushPromises()
  await click('Big-O Analysis · 2 Understand', dialog)
  await click('Zoom in', dialog)
  const saved = JSON.parse(JSON.stringify(atlas.modalView.value))
  expect(document.body.style.overflow).toBe('hidden')
  await click('Open skill', dialog)
  expect(router.currentRoute.value.path).toBe('/skills/skill_big_o')
  expect(document.querySelector('[role="dialog"]')).toBeNull()
  expect(document.body.style.overflow).toBe('')
  await click('Back to all skills')
  expect(document.querySelector('[role="dialog"]')).not.toBeNull()
  expect(atlas.modalView.value).toEqual(saved)
  await click('Close', document.querySelector('[role="dialog"]')!)
  expect(document.body.style.overflow).toBe('')
  await router.push('/skills/skill_big_o')
  router.back()
  await flushPromises()
  expect(document.querySelector('[role="dialog"]')).toBeNull()
  expect(backend.state.unexpected).toEqual([])
})

it('offers a course and enrollment for an already earned skill from the modal', async () => {
  const { atlas, backend } = await setup()
  // A previous kind preference must not silently hide matching learning content.
  sessionStorage.setItem('courses-kind-filter', 'tutorial')
  atlas.setModalOpen(true)
  await flushPromises()
  await click('Big-O Analysis · 2 Understand', document.querySelector('[role="dialog"]')!)
  await click('Find learning opportunities', document.querySelector('[role="dialog"]')!)
  expect(wrapper!.text()).toContain('Learning for Big-O Analysis')
  const courses = wrapper!.findAll('a.cc-grid')
  expect(courses.map(course => course.text())).toEqual([expect.stringContaining('Further algorithm analysis')])
  await courses[0]!.trigger('click')
  await flushPromises()
  expect(wrapper!.get('h1').text()).toBe('Further algorithm analysis')
  expect(wrapper!.text()).toContain('Linear and quadratic growth')
  expect(button('Enroll').disabled).toBe(false)
  expect(document.querySelector('[role="dialog"]')).toBeNull()
  expect(document.body.style.overflow).toBe('')
  expect(backend.state.unexpected).toEqual([])
  sessionStorage.clear()
})

it('keeps Ready learning accessible, updates the skill filter on route reuse and explains an empty result', async () => {
  const { router, backend } = await setup()
  await click('My Graph')
  await click('Sorting Algorithms · Ready')
  await click('Find learning opportunities')
  expect(wrapper!.text()).toContain('Learning for Sorting Algorithms')
  expect(wrapper!.findAll('a.cc-grid').map(course => course.text())).toEqual([expect.stringContaining('Practising sorting algorithms')])
  await router.push('/courses?skillId=skill_big_o')
  await flushPromises()
  expect(wrapper!.text()).toContain('Learning for Big-O Analysis')
  expect(wrapper!.findAll('a.cc-grid').map(course => course.text())).toEqual([expect.stringContaining('Further algorithm analysis')])
  await router.push('/courses?skillId=skill_logic')
  await flushPromises()
  expect(wrapper!.text()).toContain('No courses or tutorials for this skill in the selected category yet.')
  expect(wrapper!.findAll('a.cc-grid')).toHaveLength(0)
  await click('Browse Courses')
  expect(wrapper!.findAll('a.cc-grid')).toHaveLength(3)
  expect(backend.state.unexpected).toEqual([])
})

it('keeps the modal open with an actionable error when learning navigation fails and allows retry', async () => {
  const { atlas, router } = await setup()
  atlas.setModalOpen(true)
  await flushPromises()
  const dialog = document.querySelector('[role="dialog"]')!
  await click('Big-O Analysis · 2 Understand', dialog)
  router.onError(() => {})
  const removeGuard = router.beforeEach(to => {
    if (to.path === '/courses') throw new Error('Could not open courses')
  })
  await click('Find learning opportunities', dialog)
  expect(router.currentRoute.value.path).toBe('/skills')
  expect(dialog.querySelector('[role="alert"]')?.textContent).toContain('Could not open courses')
  removeGuard()
  await click('Find learning opportunities', dialog)
  expect(wrapper!.text()).toContain('Further algorithm analysis')
  expect(document.querySelector('[role="dialog"]')).toBeNull()
})

it('clears modal returns on lock and tears down the app refresh owner without leaking the scroll lock', async () => {
  const { atlas, profiles, router, backend } = await setup()
  document.body.style.overflow = 'clip'
  atlas.setModalOpen(true)
  await flushPromises()
  await click('Big-O Analysis · 2 Understand', document.querySelector('[role="dialog"]')!)
  await click('Open skill', document.querySelector('[role="dialog"]')!)
  await profiles.lockProfile()
  backend.state.earned = false
  await profiles.unlockProfile('profile-b', 'fixture-only')
  router.back()
  await flushPromises()
  expect(document.querySelector('[role="dialog"]')).toBeNull()
  expect(atlas.modalView.value.selectedSkillId).toBeNull()
  atlas.setModalOpen(true)
  await flushPromises()
  expect(document.body.style.overflow).toBe('hidden')
  wrapper!.unmount()
  wrapper = undefined
  expect(document.body.style.overflow).toBe('clip')
  document.body.style.overflow = ''
  const reads = backend.state.reads
  const { announceCredentialsChanged } = await import('@/composables/credentialEvents')
  announceCredentialsChanged()
  await router.push('/other')
  await flushPromises()
  expect(backend.state.reads).toBe(reads)
})

it('keeps graph visibility and teaching settings usable alongside independent browse filters', async () => {
  const { backend } = await setup()
  await click('My Graph')
  await click('👁 Public')
  let prefs = JSON.parse(backend.settings.find(item => item.key === 'instructor.graph_prefs')!.current_value)
  expect(prefs.skill_big_o.public).toBe(false)
  await click('🔒 Private')
  await click('☆ Teach')
  prefs = JSON.parse(backend.settings.find(item => item.key === 'instructor.graph_prefs')!.current_value)
  expect(prefs.skill_big_o).toEqual({ public: true, teaching: true })
})
