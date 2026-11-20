import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { createI18n } from 'vue-i18n'
import { createMemoryHistory, createRouter } from 'vue-router'
import en from '@/locales/en'
import { atlasBackend, type AtlasScenario } from '../../../tests/desktop-ui/atlasBackend'

const ipc = vi.hoisted(() => vi.fn())
vi.mock('@tauri-apps/api/core', () => ({ invoke: ipc, transformCallback: () => 1 }))
let wrapper: VueWrapper | undefined
let release: (() => void) | undefined
beforeEach(() => { vi.resetModules(); ipc.mockReset() })
afterEach(() => { wrapper?.unmount(); wrapper = undefined; release?.(); release = undefined; vi.useRealTimers() })

const BIG_O = 'Big-O Analysis, earned, Bloom level 2 Understand'

/** Mounts the sidebar graph over the strict atlas fixture, as the app sidebar shows it. */
async function setup(scenario: AtlasScenario | null = null, prepare?: (backend: ReturnType<typeof atlasBackend>) => void) {
  const backend = atlasBackend(scenario)
  prepare?.(backend)
  ipc.mockImplementation(backend.invoke)
  const { useProfiles } = await import('@/composables/useProfiles')
  const { installSkillAtlasRefresh, useSkillAtlas } = await import('@/composables/useSkillAtlas')
  const { announceCredentialsChanged } = await import('@/composables/credentialEvents')
  const { default: Sidebar } = await import('./SidebarSkillGraph.vue')
  const router = createRouter({ history: createMemoryHistory(), routes: [{ path: '/:pathMatch(.*)*', component: {} }] })
  await router.push('/skills')
  await useProfiles().initialize()
  release = installSkillAtlasRefresh(router)
  wrapper = mount(Sidebar, { attachTo: document.body, global: { plugins: [router, createI18n({ legacy: false, locale: 'en', messages: { en } })] } })
  await flushPromises()
  return { backend, wrapper, router, atlas: useSkillAtlas(), profiles: useProfiles(), announceCredentialsChanged }
}
const graph = (view: VueWrapper) => view.get('[role="group"]')
const dots = (view: VueWrapper) => view.findAll('[data-skill]')
function dot(view: VueWrapper, name: string) {
  const found = dots(view).find(item => item.attributes('aria-label') === name)
  if (!found) throw new Error(`Missing dot: ${name}`)
  return found
}

it('draws the graph behind one Tab stop with the presenter\'s names; refreshes on evidence, not on navigation', async () => {
  const { backend, wrapper, router, profiles, announceCredentialsChanged } = await setup()
  expect(graph(wrapper).attributes('aria-label')).toBe('Your skill web')
  expect(dots(wrapper)).toHaveLength(103)
  const stops = wrapper.findAll('[data-skill]').filter(item => item.attributes('tabindex') === '0')
  expect(stops).toHaveLength(1)
  expect(dot(wrapper, BIG_O).attributes('aria-pressed')).toBe('false')
  expect(wrapper.text()).toContain('Computer Science')
  expect(wrapper.text()).toContain('1 earned · 102 Ready · 103 loaded skills')
  expect(wrapper.text()).not.toContain('Locked')
  const before = backend.state.reads
  backend.state.level = 4
  announceCredentialsChanged()
  await flushPromises()
  dot(wrapper, 'Big-O Analysis, earned, Bloom level 5 Evaluate')
  expect(backend.state.reads).toBe(before + 1)
  vi.useFakeTimers()
  await vi.advanceTimersByTimeAsync(1600)
  backend.state.earned = false
  // Route changes elsewhere never refetch; the next credential event does.
  await router.push('/other')
  await router.push('/again')
  await flushPromises()
  expect(backend.state.reads).toBe(before + 1)
  announceCredentialsChanged()
  await flushPromises()
  expect(wrapper.text()).toContain('0 earned · 103 Ready · 103 loaded skills')
  expect(backend.state.reads).toBe(before + 2)
  await profiles.lockProfile()
  await flushPromises()
  expect(dots(wrapper)).toHaveLength(0)
  expect(backend.state.unexpected).toEqual([])
})

it('selects a clicked dot in place and captions it with Open skill and Expand, which keeps the selection', async () => {
  const { wrapper, router, atlas } = await setup()
  await dot(wrapper, BIG_O).trigger('click')
  expect(dot(wrapper, BIG_O).attributes('aria-pressed')).toBe('true')
  expect(atlas.modalView.value.selectedSkillId).toBe('skill_big_o')
  expect(atlas.modalOpen.value).toBe(false)
  const caption = () => wrapper.get('.sidebar-caption')
  expect(caption().text()).toContain('Big-O Analysis')
  expect(caption().text()).toContain('Earned · 2 Understand')
  expect(caption().text()).not.toContain('loaded skills')
  await caption().get('button[aria-label="Expand Big-O Analysis in the skill map"]').trigger('click')
  expect(atlas.modalOpen.value).toBe(true)
  expect(atlas.modalView.value.selectedSkillId).toBe('skill_big_o')
  atlas.setModalOpen(false)
  await caption().get('button[aria-label="Open skill Big-O Analysis"]').trigger('click')
  await flushPromises()
  expect(router.currentRoute.value.fullPath).toBe('/skills/skill_big_o')
  // Back from the skill page leaves the expanded graph closed: it was never open.
  router.back()
  await flushPromises()
  expect(router.currentRoute.value.fullPath).toBe('/skills')
  expect(atlas.modalOpen.value).toBe(false)
  // Escape on the graph clears the selection, and the counts line returns.
  const element = dot(wrapper, BIG_O).element as HTMLElement
  element.focus()
  element.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true }))
  await flushPromises()
  expect(atlas.modalView.value.selectedSkillId).toBeNull()
  expect(caption().text()).toContain('1 earned · 102 Ready · 103 loaded skills')
})
