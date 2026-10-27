import { mount, type VueWrapper } from '@vue/test-utils'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { nextTick } from 'vue'
import { createI18n } from 'vue-i18n'
import en from '@/locales/en'
import { buildSkillAtlas } from '@/utils/skillAtlas'
import type { SkillInfo, VerifiableCredential } from '@/types'
import type { AtlasViewState } from '@/composables/useSkillAtlas'
import SkillAtlasExplorer from './SkillAtlasExplorer.vue'

const skill = (id: string, name: string, field: string | null, overrides: Partial<SkillInfo> = {}): SkillInfo => ({
  id, name, description: `${name} description`, subject_id: 'subject', subject_name: 'Subject',
  subject_field_id: field, subject_field_name: field, bloom_level: 'create',
  prerequisite_count: 0, dependent_count: 0, created_at: null, ...overrides,
})
const credential = (skillId: string, level: unknown): VerifiableCredential => ({
  '@context': [], type: [], issuer: 'did:issuer', validFrom: '',
  credentialSubject: { id: 'did:me', skillId, level },
  proof: { type: '', cryptosuite: '', created: '', verificationMethod: '', proofPurpose: '', proofValue: '' },
})
const model = buildSkillAtlas({
  skills: [skill('a', 'Algebra', 'Mathematics'), skill('b', 'Bridges', 'Engineering', { prerequisite_count: 2 }), skill('c', 'Craft', null)],
  edges: [
    { skill_id: 'b', skill_name: 'Bridges', skill_bloom: 'create', prerequisite_id: 'a', prerequisite_name: 'Algebra', prerequisite_bloom: 'create' },
    { skill_id: 'b', skill_name: 'Bridges', skill_bloom: 'create', prerequisite_id: 'missing', prerequisite_name: 'Unloaded foundation', prerequisite_bloom: 'apply' },
  ],
  activeCredentials: [credential('a', 3), credential('c', 'invalid')], subjectDid: 'did:me',
})
const initialView = (): AtlasViewState => ({ selectedSkillId: null, focusedFieldId: null, unassignedFocused: false, query: '', bloomEmphasis: null, neighborhood: 'none', neighborhoodDepth: 1, camera: null, inspectorCollapsed: false })
const mounted: VueWrapper[] = []
function setup(view = initialView(), reducedMotion = false, feedback: { revision: number; earnedIds: string[]; improvedIds: string[] } | null = null) {
  const wrapper = mount(SkillAtlasExplorer, {
    props: { model, view, reducedMotion, feedback, 'onUpdate:view': (next: AtlasViewState) => { void wrapper.setProps({ view: next }) } },
    global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en } })] },
  })
  mounted.push(wrapper)
  return wrapper
}
function button(wrapper: VueWrapper, name: string) {
  const found = wrapper.findAll('button').find(item => (item.attributes('aria-label') ?? item.text()) === name)
  if (!found) throw new Error(`Missing button: ${name}`)
  return found
}
afterEach(() => { mounted.splice(0).forEach(w => w.unmount()); vi.useRealTimers() })

describe('personal atlas explorer public interactions', () => {
  it('selects a Ready skill despite unmet prerequisites and only opens or learns on explicit action', async () => {
    const wrapper = setup()
    await button(wrapper, 'Bridges · Ready').trigger('click')
    expect(wrapper.get('[aria-label="Skill inspector"]').text()).toContain('Bridges')
    expect(wrapper.text()).toContain('Unloaded foundation')
    expect(wrapper.emitted('open-skill')).toBeUndefined()
    await button(wrapper, 'Open skill').trigger('click')
    await button(wrapper, 'Find learning opportunities').trigger('click')
    expect(wrapper.emitted('open-skill')).toEqual([['b']])
    expect(wrapper.emitted('learn-skill')).toEqual([['b']])
  })
  it('shows credential Bloom separately from intrinsic Bloom, preserves malformed earned status, and offers further learning', async () => {
    const wrapper = setup()
    await button(wrapper, 'Algebra · 4 Analyze').trigger('click')
    const inspector = wrapper.get('[aria-label="Skill inspector"]')
    expect(inspector.text()).toContain('Current earned Bloom4 Analyze')
    expect(inspector.text()).toContain('Learning-outcome BloomCreate')
    expect(inspector.text()).toContain('Earned')
    await button(wrapper, 'Find learning opportunities').trigger('click')
    expect(wrapper.emitted('learn-skill')).toEqual([['a']])
    await button(wrapper, 'Craft · Earned level unavailable').trigger('click')
    expect(inspector.text()).toContain('Earned level unavailable')
    expect(inspector.text()).not.toContain('Remember')
    expect(inspector.text()).toContain('Unassigned')
  })

  it('searches across fields, retains cross-field context, and reverses prerequisite focus without losing selection', async () => {
    const wrapper = setup()
    await wrapper.get('input[type="search"]').setValue('bridg')
    await button(wrapper, 'Bridges · Ready').trigger('click')
    await button(wrapper, 'Focus prerequisites').trigger('click')
    expect(wrapper.props('view')).toMatchObject({ query: 'bridg', selectedSkillId: 'b', neighborhood: 'prerequisites' })
    expect(wrapper.get('[aria-label="Skill inspector"]').text()).toContain('Mathematics')
    await button(wrapper, 'Show all connections').trigger('click')
    expect(wrapper.props('view')).toMatchObject({ selectedSkillId: 'b', neighborhood: 'none' })
    await button(wrapper, 'Inspect Algebra').trigger('click')
    expect(wrapper.props('view')).toMatchObject({ selectedSkillId: 'a', focusedFieldId: 'Mathematics' })
    expect(wrapper.emitted('open-skill')).toBeUndefined()
  })

  it('restores view props across remount and resets all exploration controls through the public event', async () => {
    const wrapper = setup()
    await button(wrapper, 'Explore Engineering').trigger('click')
    await button(wrapper, 'Bridges · Ready').trigger('click')
    await button(wrapper, 'Zoom in').trigger('click')
    await button(wrapper, 'Move map right').trigger('click')
    await wrapper.get('select').setValue('3')
    await button(wrapper, 'Focus dependents').trigger('click')
    await button(wrapper, 'Collapse inspector').trigger('click')
    const saved = wrapper.props('view')
    expect(saved).toMatchObject({ selectedSkillId: 'b', focusedFieldId: 'Engineering', bloomEmphasis: 3, neighborhood: 'dependents', inspectorCollapsed: true, camera: { zoom: 1.2 } })
    wrapper.unmount()
    const restored = setup(saved)
    expect(restored.props('view')).toEqual(saved)
    expect(button(restored, 'Expand inspector').attributes('aria-expanded')).toBe('false')
    await button(restored, 'Expand inspector').trigger('click')
    expect(restored.get('[aria-label="Skill inspector"]').text()).toContain('Bridges')
    await button(restored, 'Reset exploration').trigger('click')
    expect(restored.props('view')).toEqual(initialView())
  })

  it('consumes stale feedback silently and bounds new evidence feedback with reduced motion requested', async () => {
    vi.useFakeTimers()
    const wrapper = setup(initialView(), true, { revision: 1, earnedIds: ['a'], improvedIds: [] })
    expect(wrapper.get('[role="status"]').text()).toBe('')
    expect(wrapper.emitted('feedback-consumed')).toEqual([[1]])
    await wrapper.setProps({ feedback: { revision: 2, earnedIds: ['b'], improvedIds: [] } })
    expect(wrapper.get('[role="status"]').text()).toContain('New evidence highlighted')
    expect(wrapper.get('[role="status"]').text()).toContain('Bridges')
    await vi.advanceTimersByTimeAsync(1600)
    expect(wrapper.emitted('feedback-consumed')).toEqual([[1], [2]])
    expect(wrapper.find('[role="status"]').text()).toBe('')
    expect(vi.getTimerCount()).toBe(0)
  })

  it('moves the map with arrow keys and exits relationship focus with Escape without routing', async () => {
    const wrapper = setup()
    await button(wrapper, 'Bridges · Ready').trigger('click')
    await button(wrapper, 'Focus prerequisites').trigger('click')
    const before = wrapper.props('view').camera
    await wrapper.get('[role="group"][aria-label="Skill map"]').trigger('keydown', { key: 'ArrowRight' })
    expect(wrapper.props('view').camera?.x).toBeGreaterThan(before?.x ?? 0)
    await button(wrapper, 'Focus prerequisites').trigger('keydown', { key: 'Escape' })
    expect(wrapper.props('view').neighborhood).toBe('none')
    expect(wrapper.props('view').selectedSkillId).toBe('b')
    expect(wrapper.emitted('open-skill')).toBeUndefined()
  })

  it('does not replay a consumed revision and discards pending feedback when hidden or unmounted', async () => {
    vi.useFakeTimers()
    const wrapper = setup()
    await wrapper.setProps({ feedback: { revision: 3, earnedIds: ['b'], improvedIds: [] } })
    await vi.advanceTimersByTimeAsync(1600)
    await wrapper.setProps({ feedback: { revision: 3, earnedIds: ['b'], improvedIds: [] } })
    expect(wrapper.get('[role="status"]').text()).toBe('')
    expect(vi.getTimerCount()).toBe(0)
    await wrapper.setProps({ feedback: { revision: 4, earnedIds: [], improvedIds: ['a'] } })
    const visibility = vi.spyOn(document, 'visibilityState', 'get').mockReturnValue('hidden')
    document.dispatchEvent(new Event('visibilitychange'))
    await nextTick()
    expect(wrapper.get('[role="status"]').text()).toBe('')
    expect(wrapper.emitted('feedback-consumed')).toEqual([[3], [4]])
    visibility.mockRestore()
    await wrapper.setProps({ feedback: { revision: 5, earnedIds: [], improvedIds: ['a'] } })
    wrapper.unmount()
    expect(vi.getTimerCount()).toBe(0)
    expect(wrapper.emitted('feedback-consumed')).toContainEqual([5])
  })

})

it('drags the public map camera without selecting the node under a drag, while preserving tap and keyboard selection', async () => {
  const wrapper = setup()
  await button(wrapper, 'Explore Engineering').trigger('click')
  const map = wrapper.get('.atlas-map')
  const before = wrapper.props('view').camera!
  const pointer = async (name: string, x: number, y: number) => {
    const event = new MouseEvent(name, { bubbles: true, button: 0, clientX: x, clientY: y })
    Object.defineProperties(event, { pointerId: { value: 1 }, pointerType: { value: 'mouse' } })
    map.element.dispatchEvent(event)
    await nextTick()
  }
  await pointer('pointerdown', 200, 200)
  await pointer('pointermove', 120, 160)
  await pointer('pointerup', 120, 160)
  expect(wrapper.props('view').camera).toEqual({ ...before, x: before.x + 80, y: before.y + 40 })
  const node = wrapper.findAll('.map-node').find(item => item.text().includes('Bridges'))!
  node.element.dispatchEvent(new MouseEvent('click', { bubbles: true, detail: 1 })); await nextTick()
  expect(wrapper.props('view').selectedSkillId).toBeNull()
  node.element.dispatchEvent(new MouseEvent('click', { bubbles: true, detail: 0 })); await nextTick()
  expect(wrapper.props('view').selectedSkillId).toBe('b')
  await pointer('pointerdown', 100, 100)
  await pointer('pointerup', 100, 100)
  node.element.dispatchEvent(new MouseEvent('click', { bubbles: true, detail: 1 })); await nextTick()
  expect(wrapper.props('view').selectedSkillId).toBe('b')
})
