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
const BRIDGES = 'Bridges, not yet earned, Ready, learning-outcome Bloom level 6 Create'
const ALGEBRA = 'Algebra, earned, Bloom level 4 Analyze'
const CRAFT = 'Craft, earned, earned level unavailable'
const mounted: VueWrapper[] = []
function setup(view = initialView(), reducedMotion = false, feedback: { revision: number; earnedIds: string[]; improvedIds: string[] } | null = null, attachTo?: HTMLElement) {
  const wrapper = mount(SkillAtlasExplorer, {
    attachTo,
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
    await button(wrapper, BRIDGES).trigger('click')
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
    await button(wrapper, ALGEBRA).trigger('click')
    const inspector = wrapper.get('[aria-label="Skill inspector"]')
    expect(inspector.text()).toContain('Current earned Bloom4 Analyze')
    expect(inspector.text()).toContain('Learning-outcome BloomCreate')
    expect(inspector.text()).toContain('Earned')
    await button(wrapper, 'Find learning opportunities').trigger('click')
    expect(wrapper.emitted('learn-skill')).toEqual([['a']])
    await button(wrapper, CRAFT).trigger('click')
    expect(inspector.text()).toContain('Earned level unavailable')
    expect(inspector.text()).not.toContain('Remember')
    expect(inspector.text()).toContain('Unassigned')
  })

  it('names every skill by earned state and Bloom level in previews, the list and the map, with a decorative checkmark', async () => {
    const wrapper = setup()
    const names = (selector: string) => wrapper.findAll(selector).map(item => item.attributes('aria-label'))
    expect(names('.field-earned')).toEqual(expect.arrayContaining([ALGEBRA, CRAFT]))
    expect(names('.atlas-list li button')).toEqual([ALGEBRA, BRIDGES, CRAFT])
    await button(wrapper, 'Explore Engineering').trigger('click')
    expect(names('.map-node')).toContain(BRIDGES)
    await button(wrapper, 'Mathematics').trigger('click')
    expect(names('.map-node')).toContain(ALGEBRA)
    const checks = wrapper.findAll('span').filter(item => item.element.childNodes.length === 1 && item.text().startsWith('✓'))
    expect(checks.length).toBeGreaterThan(0)
    expect(checks.every(item => item.attributes('aria-hidden') === 'true' && item.attributes('aria-label') === undefined)).toBe(true)
  })

  it('keeps a relationship focus visible on the map with the inspector collapsed, and clears it from there', async () => {
    const wrapper = setup()
    const indicator = () => wrapper.findAll('[role="status"]').find(item => item.text().startsWith('Showing'))
    await button(wrapper, BRIDGES).trigger('click')
    expect(indicator()).toBeUndefined()
    await button(wrapper, 'Focus prerequisites').trigger('click')
    await button(wrapper, 'Collapse inspector').trigger('click')
    expect(indicator()?.isVisible()).toBe(true)
    expect(indicator()?.text()).toContain('Showing prerequisites of Bridges · 1 step')
    await wrapper.setProps({ view: { ...wrapper.props('view'), neighborhood: 'dependents', neighborhoodDepth: 2 } })
    expect(indicator()?.text()).toContain('Showing dependents of Bridges · 2 steps')
    await button(wrapper, 'Clear focus').trigger('click')
    expect(wrapper.props('view')).toMatchObject({ neighborhood: 'none', selectedSkillId: 'b', inspectorCollapsed: true })
    expect(indicator()).toBeUndefined()
    expect(wrapper.findAll('.map-node.is-dimmed')).toHaveLength(0)
  })

  it('says a skill with no prerequisites has none instead of claiming prerequisite evidence', async () => {
    const wrapper = setup()
    await button(wrapper, ALGEBRA).trigger('click')
    const inspector = wrapper.get('[aria-label="Skill inspector"]')
    expect(inspector.text()).toContain('No prerequisites')
    expect(inspector.text()).not.toContain('Prerequisite evidence available')
  })

  it('emphasizes earned Bloom in overview previews and the full-label list without removing Ready or prerequisite context', async () => {
    const wrapper = setup()
    await wrapper.get('select').setValue('3')
    expect(wrapper.find('.atlas-overview').exists()).toBe(true)
    expect(wrapper.text()).toContain('Emphasizing 4 Analyze · Matches in the list: 1. Other skills remain available.')
    expect(wrapper.get('.field-earned.is-bloom-match').text()).toContain('Algebra')
    expect(wrapper.get('.field-earned.is-bloom-match').text()).toContain('✓ 4 Analyze')
    expect(wrapper.get('.atlas-list .is-bloom-match').text()).toContain('Algebra')
    expect(wrapper.findAll('.atlas-list button')).toHaveLength(3)
    await wrapper.get('select').setValue('5')
    expect(wrapper.text()).toContain('Emphasizing 6 Create · Matches in the list: 0.')
    expect(wrapper.findAll('.is-bloom-match')).toHaveLength(0)
    expect(wrapper.text()).toContain('2 earned · 1 Ready · 3 loaded skills')
    await button(wrapper, BRIDGES).trigger('click')
    expect(wrapper.get('[aria-label="Skill inspector"]').text()).toContain('Unloaded foundation')
    expect(wrapper.get('[aria-label="Skill inspector"]').text()).toContain('Algebra')
    await button(wrapper, 'Find learning opportunities').trigger('click')
    expect(wrapper.emitted('learn-skill')).toEqual([['b']])
    await wrapper.get('select').setValue('')
    expect(wrapper.text()).not.toContain('Emphasizing')
  })

  it('brings a matching earned skill into a capped field preview while keeping loaded and search counts truthful', async () => {
    const wrapper = setup()
    await wrapper.setProps({ model: buildSkillAtlas({
      skills: [skill('a', 'Algebra', 'Mathematics'), skill('b', 'Basic geometry', 'Mathematics'), skill('c', 'Calculus', 'Mathematics')],
      edges: [], activeCredentials: [credential('a', 1), credential('b', 2), credential('c', 5)], subjectDid: 'did:me',
    }) })
    expect(wrapper.findAll('.field-earned').map(node => node.text())).not.toContain(expect.stringContaining('Calculus'))
    await wrapper.get('select').setValue('5')
    expect(wrapper.get('.field-earned.is-bloom-match').text()).toContain('Calculus')
    expect(wrapper.get('.field-earned.is-bloom-match').text()).toContain('✓ 6 Create')
    expect(wrapper.text()).toContain('3 earned · 0 Ready · 3 loaded skills')
    expect(wrapper.findAll('.field-earned')).toHaveLength(2)
    await wrapper.get('input[type="search"]').setValue('Algebra')
    expect(wrapper.get('.bloom-summary').text()).toContain('Matches in the list: 0.')
    expect(wrapper.findAll('.atlas-list button')).toHaveLength(1)
    expect(wrapper.text()).toContain('3 earned · 0 Ready · 3 loaded skills')
  })

  it('searches across fields, retains cross-field context, and reverses prerequisite focus without losing selection', async () => {
    const wrapper = setup()
    await wrapper.get('input[type="search"]').setValue('bridg')
    await button(wrapper, BRIDGES).trigger('click')
    await button(wrapper, 'Focus prerequisites').trigger('click')
    expect(wrapper.props('view')).toMatchObject({ query: 'bridg', selectedSkillId: 'b', neighborhood: 'prerequisites' })
    expect(wrapper.get('[aria-label="Skill inspector"]').text()).toContain('Mathematics')
    await button(wrapper, 'Show all connections').trigger('click')
    expect(wrapper.props('view')).toMatchObject({ selectedSkillId: 'b', neighborhood: 'none' })
    await button(wrapper, 'Inspect Algebra, earned').trigger('click')
    expect(wrapper.props('view')).toMatchObject({ selectedSkillId: 'a', focusedFieldId: 'Mathematics' })
    expect(wrapper.emitted('open-skill')).toBeUndefined()
  })

  it('names each inspector relation with its earned state, as its visible text shows', async () => {
    const wrapper = setup({ ...initialView(), selectedSkillId: 'b' })
    const inspector = wrapper.get('[aria-label="Skill inspector"]')
    expect(inspector.findAll('.inspector-relations button').map(item => item.attributes('aria-label'))).toEqual(['Inspect Algebra, earned'])
    await button(wrapper, 'Inspect Algebra, earned').trigger('click')
    expect(inspector.findAll('.inspector-relations button').map(item => item.attributes('aria-label'))).toEqual(['Inspect Bridges, not yet earned, Ready'])
  })

  it('restores view props across remount and resets all exploration controls through the public event', async () => {
    const wrapper = setup()
    await button(wrapper, 'Explore Engineering').trigger('click')
    await button(wrapper, BRIDGES).trigger('click')
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

  it('keeps new evidence highlighted for exactly as long as its reveal animation', async () => {
    vi.useFakeTimers()
    document.documentElement.style.setProperty('--duration-reveal', '900ms')
    const wrapper = setup()
    await wrapper.setProps({ feedback: { revision: 6, earnedIds: ['b'], improvedIds: [] } })
    await vi.advanceTimersByTimeAsync(850)
    expect(wrapper.get('[role="status"]').text()).toContain('Bridges')
    await vi.advanceTimersByTimeAsync(100)
    expect(wrapper.get('[role="status"]').text()).toBe('')
    document.documentElement.style.removeProperty('--duration-reveal')
  })

  it('moves the map with arrow keys and exits relationship focus with Escape without routing', async () => {
    const wrapper = setup()
    await button(wrapper, BRIDGES).trigger('click')
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

it('moves the canvas live while dragging and publishes the camera once, when the drag ends', async () => {
  const wrapper = setup()
  await button(wrapper, 'Explore Engineering').trigger('click')
  const map = wrapper.get('.atlas-map')
  const canvas = wrapper.get('.atlas-canvas').element as SVGSVGElement
  const emitted = () => wrapper.emitted('update:view')?.length ?? 0
  const offset = () => (canvas.style.transform.match(/-?[\d.]+(?=px)/g) ?? []).map(Number).slice(0, 2)
  const pointer = async (name: string, x: number, y: number) => {
    const event = new MouseEvent(name, { bubbles: true, button: 0, clientX: x, clientY: y })
    Object.defineProperties(event, { pointerId: { value: 1 }, pointerType: { value: 'mouse' } })
    map.element.dispatchEvent(event)
    await nextTick()
  }
  const before = wrapper.props('view').camera!
  const start = offset()
  expect(start).toHaveLength(2)
  const published = emitted()
  await pointer('pointerdown', 200, 200)
  await pointer('pointermove', 150, 180)
  await pointer('pointermove', 100, 160)
  // Only the canvas moves: the camera and the shared view wait for the drag to end.
  expect(offset()).toEqual([start[0]! - 100, start[1]! - 40])
  expect(map.attributes('data-camera')).toBe(`${before.x} ${before.y} ${before.zoom}`)
  expect(emitted()).toBe(published)
  await pointer('pointerup', 100, 160)
  expect(emitted()).toBe(published + 1)
  expect(wrapper.props('view').camera).toEqual({ ...before, x: before.x + 100, y: before.y + 40 })
  await nextTick()
  expect(map.attributes('data-camera')).toBe(`${before.x + 100} ${before.y + 40} ${before.zoom}`)
  expect(offset()).toEqual([start[0]! - 100, start[1]! - 40])
})

it('draws every prerequisite edge whole, highlights the selected skill\'s, and leaves them alone while panning', async () => {
  // Zero-padded so ID order is row order: three skills per row, ten rows.
  const id = (index: number) => `s${String(index).padStart(2, '0')}`
  const many = Array.from({ length: 30 }, (_, index) => skill(id(index), `Skill ${index}`, 'Long field'))
  const edge = (from: number, to: number) => ({ skill_id: id(to), skill_name: `Skill ${to}`, skill_bloom: 'create',
    prerequisite_id: id(from), prerequisite_name: `Skill ${from}`, prerequisite_bloom: 'create' })
  const wrapper = setup()
  await wrapper.setProps({ model: buildSkillAtlas({ skills: many, edges: [edge(0, 1), edge(27, 28)], activeCredentials: [], subjectDid: 'did:me' }) })
  await button(wrapper, 'Explore Long field').trigger('click')
  const curves = (selector: string) => wrapper.get(selector).attributes('d')?.match(/M /g)?.length ?? 0
  expect(curves('.map-edge:not(.map-edge-active)')).toBe(2)
  expect(curves('.map-edge-head:not(.map-edge-active)')).toBe(2)
  expect(curves('.map-edge.map-edge-active')).toBe(0)
  await wrapper.setProps({ view: { ...wrapper.props('view'), selectedSkillId: id(28) } })
  expect(curves('.map-edge:not(.map-edge-active)')).toBe(1)
  expect(curves('.map-edge.map-edge-active')).toBe(1)
  expect(curves('.map-edge-head.map-edge-active')).toBe(1)
  const plain = wrapper.get('.map-edge:not(.map-edge-active)').attributes('d')
  for (let step = 0; step < 8; step++) await wrapper.get('.atlas-map').trigger('keydown', { key: 'ArrowDown' })
  expect(wrapper.get('.map-edge:not(.map-edge-active)').attributes('d')).toBe(plain)
})

it('draws nodes past the screen edge without letting them take focus, and follows the screen as it moves', async () => {
  const id = (index: number) => `s${String(index).padStart(2, '0')}`
  const many = Array.from({ length: 60 }, (_, index) => skill(id(index), `Skill ${index}`, 'Long field'))
  const wrapper = setup()
  await wrapper.setProps({ model: buildSkillAtlas({ skills: many, edges: [], activeCredentials: [], subjectDid: 'did:me' }) })
  await button(wrapper, 'Explore Long field').trigger('click')
  const nodes = () => wrapper.findAll('.map-node')
  const focusable = () => nodes().filter(node => node.attributes('inert') === undefined).map(node => node.find('.skill-name').text())
  const drawnOnly = () => nodes().filter(node => node.attributes('inert') !== undefined).map(node => node.find('.skill-name').text())
  expect(focusable()).toContain('Skill 0')
  expect(focusable()).not.toContain('Skill 59')
  // Nodes past the edge join one at a time when idle.
  for (let frame = 0; frame < 60 && drawnOnly().length === 0; frame++) await new Promise(resolve => setTimeout(resolve, 20))
  expect(drawnOnly().length).toBeGreaterThan(0)
  expect(nodes().map(node => node.find('.skill-name').text())).not.toContain('Skill 59')
  for (let step = 0; step < 40; step++) await wrapper.get('.atlas-map').trigger('keydown', { key: 'ArrowDown' })
  expect(focusable()).toContain('Skill 59')
  expect(nodes().map(node => node.find('.skill-name').text())).not.toContain('Skill 0')
})

it('renders a long match list a window at a time, keeping the full count and the window while selecting', async () => {
  const id = (index: number) => `s${String(index).padStart(3, '0')}`
  const many = Array.from({ length: 120 }, (_, index) => skill(id(index), `Skill ${index}`, 'Big field'))
  const wrapper = setup(initialView(), false, null, document.body.appendChild(document.createElement('div')))
  await wrapper.setProps({ model: buildSkillAtlas({ skills: many, edges: [], activeCredentials: [], subjectDid: 'did:me' }) })
  const rows = () => wrapper.findAll('.atlas-list li button')
  expect(wrapper.get('.atlas-list summary').text()).toContain('120 matching skills')
  expect(rows()).toHaveLength(50)
  await button(wrapper, 'Show 50 more skills').trigger('click')
  expect(rows()).toHaveLength(100)
  expect(document.activeElement).toBe(rows()[50]!.element)
  await rows()[99]!.trigger('click')
  expect(wrapper.props('view').selectedSkillId).toBe('s099')
  expect(rows()).toHaveLength(100)
  await button(wrapper, 'Show 20 more skills').trigger('click')
  expect(rows()).toHaveLength(120)
  expect(wrapper.findAll('button').some(item => item.text().startsWith('Show '))).toBe(false)
  await wrapper.get('input[type="search"]').setValue('Skill 1')
  // "Skill 1", "Skill 10"–"Skill 19" and "Skill 100"–"Skill 119".
  expect(wrapper.get('.atlas-list summary').text()).toContain('31 matching skills')
  expect(rows()).toHaveLength(31)
  await wrapper.get('input[type="search"]').setValue('')
  expect(rows()).toHaveLength(50)
})

it('keeps every existing field in its colour and map region when a new field is added', async () => {
  const fields = [{ id: 'sf_math', name: 'Mathematics', atlas_slot: 0 }, { id: 'sf_eng', name: 'Engineering', atlas_slot: 1 }]
  const skills = [skill('a', 'Algebra', 'sf_math'), skill('b', 'Bridges', 'sf_eng'), skill('e', 'Engines', 'sf_eng')]
  const atlas = (subjectFields: typeof fields, extra: SkillInfo[] = []) =>
    buildSkillAtlas({ skills: [...skills, ...extra], edges: [], activeCredentials: [], subjectDid: 'did:me', subjectFields })
  const wrapper = setup()
  await wrapper.setProps({ model: atlas(fields) })
  await button(wrapper, 'Explore Mathematics').trigger('click')
  const regions = () => Object.fromEntries(wrapper.findAll('.atlas-canvas > g:not(.map-edges)').map(region => [
    region.get('.region-heading strong').text(),
    { x: region.get('rect').attributes('x'), y: region.get('rect').attributes('y'), colour: region.attributes('style') },
  ]))
  const before = regions()
  expect(before.Mathematics?.colour).toContain('var(--atlas-field-0)')
  expect(before.Engineering?.colour).toContain('var(--atlas-field-1)')
  // Sorts first by ID and by name, and takes the next unused slot.
  await wrapper.setProps({ model: atlas([...fields, { id: 'sf_agri', name: 'Agriculture', atlas_slot: 2 }], [skill('f', 'Farming', 'sf_agri')]) })
  const after = regions()
  expect(after.Mathematics).toEqual(before.Mathematics)
  expect(after.Engineering).toEqual(before.Engineering)
  expect(after.Agriculture?.colour).toContain('var(--atlas-field-2)')
})


it('emphasises as ready next only unearned skills whose counted prerequisites are all loaded and earned', async () => {
  const edge = (skillId: string, prerequisiteId: string) => ({ skill_id: skillId, skill_name: skillId, skill_bloom: 'create',
    prerequisite_id: prerequisiteId, prerequisite_name: prerequisiteId, prerequisite_bloom: 'create' })
  const atlas = (earned: VerifiableCredential[]) => buildSkillAtlas({
    skills: [skill('a', 'Algebra', 'Mathematics'), skill('g', 'Geometry', 'Mathematics', { prerequisite_count: 1, bloom_level: 'apply' }),
      skill('b', 'Bridges', 'Mathematics', { prerequisite_count: 2 }), skill('p', 'Proofs', 'Mathematics')],
    edges: [edge('g', 'a'), edge('b', 'a'), edge('b', 'missing')],
    activeCredentials: earned, subjectDid: 'did:me' })
  const GEOMETRY_NEXT = 'Geometry, not yet earned, ready next, learning-outcome Bloom level 3 Apply'
  const wrapper = setup()
  await wrapper.setProps({ model: atlas([credential('a', 3), credential('missing', 2)]) })
  const emphasised = (selector: string) => wrapper.findAll(`${selector}.is-ready-next`).map(item => item.attributes('aria-label'))
  // The list: only Geometry, whose one prerequisite is earned. Bridges waits on an
  // unloaded prerequisite and Proofs has none.
  expect(emphasised('.atlas-list li button')).toEqual([GEOMETRY_NEXT])
  expect(wrapper.get('.atlas-list .is-ready-next').text()).toContain('Ready next')
  expect(button(wrapper, 'Bridges, not yet earned, Ready, learning-outcome Bloom level 6 Create').classes()).not.toContain('is-ready-next')
  await button(wrapper, 'Explore Mathematics').trigger('click')
  expect(emphasised('.map-node')).toEqual([GEOMETRY_NEXT])
  expect(wrapper.get('.map-node.is-ready-next').text()).toContain('Ready next')
  expect(wrapper.findAll('.map-node').map(node => node.attributes('aria-label'))).toEqual(expect.arrayContaining([
    'Bridges, not yet earned, Ready, learning-outcome Bloom level 6 Create', 'Proofs, not yet earned, Ready, learning-outcome Bloom level 6 Create']))
  // The inspector says it, and so do relations that point at it.
  await wrapper.get('.map-node.is-ready-next').trigger('click')
  const inspector = wrapper.get('[aria-label="Skill inspector"]')
  expect(inspector.get('.inspector-status').text()).toBe('◎ Ready next')
  expect(inspector.get('.inspector-status [aria-hidden="true"]').text()).toBe('◎')
  expect(inspector.text()).toContain('Prerequisite evidence available')
  await button(wrapper, 'Inspect Algebra, earned').trigger('click')
  expect(inspector.findAll('.inspector-relations button').map(item => item.attributes('aria-label')))
    .toEqual(['Inspect Bridges, not yet earned, Ready', 'Inspect Geometry, not yet earned, ready next'])
  // A memoised node follows the model: without Algebra, Geometry is no longer next.
  await wrapper.setProps({ model: atlas([]) })
  expect(wrapper.findAll('.is-ready-next')).toHaveLength(0)
  expect(wrapper.findAll('.map-node').map(node => node.attributes('aria-label'))).toContain('Geometry, not yet earned, Ready, learning-outcome Bloom level 3 Apply')
  await wrapper.setProps({ model: atlas([credential('a', 1)]) })
  expect(emphasised('.map-node')).toEqual([GEOMETRY_NEXT])
})
