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
const shown = (view: VueWrapper) => new Set(dots(view).map(item => item.attributes('data-skill')!))
/** Dots whose name shows under them. */
const named = (view: VueWrapper) => view.findAll('.web-name').length
const places = (view: VueWrapper) => dots(view).map(item => `${item.attributes('data-skill')} ${item.attributes('style')}`)
const faded = (view: VueWrapper) => dots(view).filter(item => item.classes('is-faded')).map(item => item.attributes('data-skill'))
/** Line segments drawn by the paths matching a selector: each starts with a move. */
const segments = (view: VueWrapper, selector: string) => view.findAll(selector).reduce((sum, path) => sum + (path.attributes('d')?.match(/M/g)?.length ?? 0), 0)

it('draws the graph behind one Tab stop with the presenter\'s names; refreshes on evidence, not on navigation', async () => {
  const { backend, wrapper, router, profiles, announceCredentialsChanged } = await setup()
  expect(graph(wrapper).attributes('aria-label')).toBe('Your skill web')
  expect(dots(wrapper)).toHaveLength(113)
  const stops = wrapper.findAll('[data-skill]').filter(item => item.attributes('tabindex') === '0')
  expect(stops).toHaveLength(1)
  expect(dot(wrapper, BIG_O).attributes('aria-pressed')).toBe('false')
  expect(wrapper.text()).toContain('Computer Science')
  expect(wrapper.text()).toContain('1 earned · 112 Ready · 113 loaded skills')
  expect(wrapper.text()).not.toContain('Locked')
  expect(wrapper.text()).not.toContain('Showing')
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
  expect(wrapper.text()).toContain('0 earned · 113 Ready · 113 loaded skills')
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
  expect(caption().text()).toContain('1 earned · 112 Ready · 113 loaded skills')
})

it('follows the Names, Arrows and Related toggles set in the expanded graph, and keeps its own camera', async () => {
  const { wrapper, atlas } = await setup()
  await flushPromises()
  expect(named(wrapper)).toBe(0)
  expect(segments(wrapper, '.web-head')).toBe(0)
  expect(segments(wrapper, '.web-relation')).toBeGreaterThan(0)
  const before = places(wrapper)
  atlas.updateView('modal', { webNames: true, webArrows: true, webRelated: false, webCamera: { x: 0, y: 0, zoom: 4 } })
  await flushPromises()
  expect(named(wrapper)).toBe(113)
  expect(segments(wrapper, '.web-head')).toBeGreaterThan(0)
  expect(segments(wrapper, '.web-relation')).toBe(0)
  // The expanded graph's camera is its own: nothing moved here.
  expect(places(wrapper)).toEqual(before)
})

it('focuses a field in place from its button, fading the other fields and fitting it, and a second press clears the focus', async () => {
  const { wrapper, atlas, backend } = await setup()
  const field = () => wrapper.findAll('button').find(item => item.text().startsWith('Computer Science'))!
  const computing = backend.skills.filter(skill => skill.subject_field_name === 'Computer Science').map(skill => skill.id)
  const others = backend.skills.filter(skill => skill.subject_field_name !== 'Computer Science').map(skill => skill.id)
  expect(field().text()).toContain('1/' + computing.length + ' earned')
  expect(field().attributes('aria-pressed')).toBe('false')
  const whole = places(wrapper)
  await field().trigger('click')
  expect(field().attributes('aria-pressed')).toBe('true')
  expect(atlas.modalOpen.value).toBe(false)
  expect(atlas.modalView.value.focusedFieldId).toBe(backend.skills.find(skill => skill.id === 'skill_big_o')!.subject_field_id)
  expect(faded(wrapper).sort()).toEqual(others.sort())
  // The camera fits the field, so the dots are drawn larger and elsewhere.
  expect(places(wrapper)).not.toEqual(whole)
  await field().trigger('click')
  expect(field().attributes('aria-pressed')).toBe('false')
  expect(atlas.modalView.value.focusedFieldId).toBeNull()
  expect(faded(wrapper)).toEqual([])
  expect(places(wrapper)).toEqual(whole)
})

it('zooms with the wheel, shows dots dragged in the expanded graph, and Reset restores both without clearing the selection', async () => {
  const { wrapper, atlas } = await setup()
  const whole = places(wrapper)
  await dot(wrapper, BIG_O).trigger('click')
  const resting = atlas.webPositions.value.get('skill_big_o')!
  atlas.setWebPositions(new Map([['skill_big_o', { x: resting.x + 200, y: resting.y - 150 }]]))
  await flushPromises()
  expect(places(wrapper)).not.toEqual(whole)
  const wheel = new WheelEvent('wheel', { deltaY: -300, bubbles: true, cancelable: true })
  graph(wrapper).element.dispatchEvent(wheel)
  await flushPromises()
  // The wheel zooms the graph instead of scrolling the sidebar.
  expect(wheel.defaultPrevented).toBe(true)
  expect(atlas.sidebarWebCamera.value).not.toBeNull()
  expect(atlas.modalView.value.webCamera).toBeNull()
  await wrapper.get('button[aria-label="Reset zoom and moved dots"]').trigger('click')
  expect(atlas.sidebarWebCamera.value).toBeNull()
  expect(atlas.webPositions.value.get('skill_big_o')).toEqual(resting)
  expect(atlas.modalView.value.selectedSkillId).toBe('skill_big_o')
  atlas.updateView('modal', { selectedSkillId: null })
  await flushPromises()
  expect(places(wrapper)).toEqual(whole)
})

it('above the node limit shows only earned skills and their neighbours, and says so', async () => {
  const { scaleTaxonomy, SCALE_SKILLS } = await import('../../../tests/desktop-ui/atlasBackend')
  const { wrapper, atlas } = await setup('scale')
  const earned = new Set(atlas.model.value!.nodes.filter(node => node.status === 'earned').map(node => node.skill.id))
  const expected = new Set(earned)
  for (const edge of scaleTaxonomy().edges) {
    if (earned.has(edge.skill_id)) expected.add(edge.prerequisite_id)
    if (earned.has(edge.prerequisite_id)) expected.add(edge.skill_id)
  }
  expect(earned.size).toBeGreaterThan(1)
  expect(shown(wrapper)).toEqual(expected)
  expect(expected.size).toBeLessThan(SCALE_SKILLS)
  expect(wrapper.text()).toContain(`Showing ${expected.size} of ${SCALE_SKILLS} skills: your earned skills and the skills next to them.`)
})

it('above the node limit with nothing earned shows the skills that build on nothing', async () => {
  const { scaleTaxonomy, SCALE_SKILLS } = await import('../../../tests/desktop-ui/atlasBackend')
  const { wrapper } = await setup('scale', backend => { backend.state.earned = false })
  const { skills, edges } = scaleTaxonomy()
  const buildsOnSomething = new Set(edges.map(edge => edge.skill_id))
  const starts = new Set(skills.map(skill => skill.id).filter(id => !buildsOnSomething.has(id)))
  expect(starts.size).toBeGreaterThan(0)
  expect(shown(wrapper)).toEqual(starts)
  expect(wrapper.text()).toContain(`Showing ${starts.size} of ${SCALE_SKILLS} skills: the skills that build on nothing, where you can start.`)
})

it('keeps the loading, empty and error states, and retry draws the graph', async () => {
  let answer: (() => void) | undefined
  let fail = true
  const { wrapper } = await setup(null, backend => {
    const invoke = backend.invoke
    backend.invoke = async (command, args) => {
      if (command === 'list_skills') {
        await new Promise<void>(resolve => { answer = resolve })
        if (fail) throw new Error('Skills are unavailable')
      }
      return invoke(command, args)
    }
  })
  expect(wrapper.get('[role="status"]').text()).toBe('Loading your skills…')
  expect(dots(wrapper)).toHaveLength(0)
  answer!()
  await flushPromises()
  expect(wrapper.text()).toContain('No skills loaded yet')
  expect(wrapper.get('[role="alert"]').text()).toContain('Skills are unavailable')
  fail = false
  await wrapper.get('[role="alert"] button').trigger('click')
  await flushPromises()
  answer!()
  await flushPromises()
  expect(wrapper.find('[role="alert"]').exists()).toBe(false)
  expect(dots(wrapper)).toHaveLength(113)
})
