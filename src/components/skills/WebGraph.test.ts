import { mount, type VueWrapper } from '@vue/test-utils'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { nextTick } from 'vue'
import { createI18n } from 'vue-i18n'
import en from '@/locales/en'
import { buildSkillAtlas, type AtlasModel } from '@/utils/skillAtlas'
import type { AtlasCamera } from '@/utils/skillAtlasView'
import type { SkillInfo, VerifiableCredential } from '@/types'
import type { WebPoint } from '@/utils/webGeometry'
import WebGraph from './WebGraph.vue'
import { motionDuration } from '@/composables/useMotion'

const motionDurationForTest = () => motionDuration('emphasis')

const skill = (id: string, name: string, field: string | null, overrides: Partial<SkillInfo> = {}): SkillInfo => ({
  id, name, description: '', subject_id: 'subject', subject_name: 'Subject',
  subject_field_id: field, subject_field_name: field, bloom_level: 'apply',
  prerequisite_count: 0, dependent_count: 0, created_at: null, ...overrides,
})
const edge = (skillId: string, prerequisiteId: string) => ({
  skill_id: skillId, skill_name: skillId, skill_bloom: 'apply', prerequisite_id: prerequisiteId, prerequisite_name: prerequisiteId, prerequisite_bloom: 'apply',
})
const credential = (skillId: string, level: number): VerifiableCredential => ({
  '@context': [], type: [], issuer: 'did:issuer', validFrom: '',
  credentialSubject: { id: 'did:me', skillId, level },
  proof: { type: '', cryptosuite: '', created: '', verificationMethod: '', proofPurpose: '', proofValue: '' },
})
// Algebra (earned) → Bridges → Calculus; Data is related to Algebra; Essays stands alone.
const model = buildSkillAtlas({
  skills: [
    skill('a', 'Algebra', 'math'), skill('b', 'Bridges', 'math', { prerequisite_count: 1 }), skill('c', 'Calculus', 'math', { prerequisite_count: 1 }),
    skill('d', 'Data', 'data'), skill('e', 'Essays', 'writing'),
  ],
  edges: [edge('b', 'a'), edge('c', 'b')],
  relations: [{ skill_id: 'a', related_skill_id: 'd', relation_type: 'related' }],
  activeCredentials: [credential('a', 3)], subjectDid: 'did:me',
})
const ALGEBRA = 'Algebra, earned, Bloom level 4 Analyze'
const BRIDGES = 'Bridges, not yet earned, ready next, learning-outcome Bloom level 3 Apply'
const CALCULUS = 'Calculus, not yet earned, Ready, learning-outcome Bloom level 3 Apply'
const DATA = 'Data, not yet earned, Ready, learning-outcome Bloom level 3 Apply'
const ESSAYS = 'Essays, not yet earned, Ready, learning-outcome Bloom level 3 Apply'
// A row: Essays, Algebra, Bridges, Calculus left to right; Data below Algebra.
const positions: ReadonlyMap<string, WebPoint> = new Map([
  ['e', { x: -100, y: 0 }], ['a', { x: 0, y: 0 }], ['b', { x: 100, y: 0 }], ['c', { x: 200, y: 0 }], ['d', { x: 0, y: 100 }],
])
const CAMERA: AtlasCamera = { x: 50, y: 50, zoom: 1 }

interface Props {
  model?: AtlasModel
  positions?: ReadonlyMap<string, WebPoint>
  selectedId?: string | null
  names?: boolean
  arrows?: boolean
  related?: boolean
  narrowed?: boolean
  camera?: AtlasCamera | null
  compact?: boolean
  reducedMotion?: boolean
  shownIds?: ReadonlySet<string> | null
  matchIds?: ReadonlySet<string> | null
}
const mounted: VueWrapper[] = []
afterEach(() => { mounted.splice(0).forEach(wrapper => wrapper.unmount()); vi.useRealTimers() })
/** Mounts the graph as its owners do: selection, camera and positions are fed back from its events. */
function setup(props: Props = {}) {
  const wrapper = mount(WebGraph, {
    attachTo: document.body,
    props: {
      model, positions, camera: CAMERA, label: 'Skill web', ...props,
      onSelect: (id: string | null) => { void wrapper.setProps({ selectedId: id }) },
      'onUpdate:camera': (camera: AtlasCamera) => { void wrapper.setProps({ camera }) },
      onDrag: (next: ReadonlyMap<string, WebPoint>) => { void wrapper.setProps({ positions: next }) },
    },
    global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en } })] },
  })
  mounted.push(wrapper)
  return wrapper
}
const dots = (wrapper: VueWrapper) => wrapper.findAll('[data-skill]')
function dot(wrapper: VueWrapper, name: string) {
  const found = dots(wrapper).find(item => item.attributes('aria-label') === name)
  if (!found) throw new Error(`Missing dot: ${name}`)
  return found
}

const faded = (wrapper: VueWrapper) => dots(wrapper).filter(item => item.classes('is-faded')).map(item => item.attributes('data-skill')).sort()
const focusedSkill = () => (document.activeElement as HTMLElement | null)?.dataset.skill
const key = async (wrapper: VueWrapper, name: string) => {
  const target = (document.activeElement as HTMLElement | null) ?? wrapper.element
  target.dispatchEvent(new KeyboardEvent('keydown', { key: name, bubbles: true, cancelable: true }))
  await nextTick()
}

/** Line segments drawn by the paths matching a selector: each starts with a move. */
const segments = (wrapper: VueWrapper, selector: string) => wrapper.findAll(selector).reduce((sum, path) => sum + (path.attributes('d')?.match(/M/g)?.length ?? 0), 0)
const places = (wrapper: VueWrapper) => dots(wrapper).map(item => `${item.attributes('data-skill')} ${item.attributes('style')}`)

/** A pointer event as the browser sends it, on an element of the graph. */
async function pointer(target: Element, name: string, x: number, y: number, pointerId = 1, pointerType = 'mouse') {
  const event = new MouseEvent(name, { bubbles: true, cancelable: true, button: 0, clientX: x, clientY: y })
  Object.defineProperties(event, { pointerId: { value: pointerId }, pointerType: { value: pointerType } })
  target.dispatchEvent(event)
  await nextTick()
}
const last = <T>(list: readonly T[]): T | undefined => list[list.length - 1]
const dragged = (wrapper: VueWrapper) => (wrapper.emitted('drag') ?? []).map(([next]) => next as ReadonlyMap<string, WebPoint>)

describe('Web graph', () => {
  it('draws every skill as a dot named as in the other views, behind one Tab stop at the selection', () => {
    const wrapper = setup({ selectedId: 'b' })
    expect(wrapper.get('[role="group"]').attributes('aria-label')).toBe('Skill web')
    expect(dots(wrapper).map(item => item.attributes('aria-label')).sort()).toEqual([ALGEBRA, BRIDGES, CALCULUS, DATA, ESSAYS].sort())
    const stops = wrapper.findAll('[tabindex]').filter(item => Number(item.attributes('tabindex')) >= 0)
    expect(stops.map(item => item.attributes('aria-label'))).toEqual([BRIDGES])
    expect(dot(wrapper, BRIDGES).attributes('aria-pressed')).toBe('true')
    expect(dot(wrapper, ALGEBRA).attributes('aria-pressed')).toBe('false')
  })

  it('moves a roving dot with the arrow keys, lighting its neighbourhood, and selects with Enter or Space and clears with Escape', async () => {
    const wrapper = setup()
    expect(faded(wrapper)).toEqual([])
    const algebra = dot(wrapper, ALGEBRA).element as HTMLElement
    algebra.focus()
    await nextTick()
    await key(wrapper, 'ArrowRight')
    expect(focusedSkill()).toBe('b')
    expect(dot(wrapper, BRIDGES).attributes('tabindex')).toBe('0')
    expect(dot(wrapper, ALGEBRA).attributes('tabindex')).toBe('-1')
    // Bridges builds on Algebra and leads to Calculus.
    expect(faded(wrapper)).toEqual(['d', 'e'])
    expect(last(wrapper.emitted('active') ?? [])).toEqual(['b'])
    await key(wrapper, 'ArrowDown')
    expect(focusedSkill()).toBe('d')
    await key(wrapper, 'ArrowUp')
    expect(focusedSkill()).toBe('a')
    expect(faded(wrapper)).toEqual(['c', 'e'])
    await key(wrapper, 'Enter')
    expect(wrapper.emitted('select')).toEqual([['a']])
    await key(wrapper, 'ArrowLeft')
    await key(wrapper, ' ')
    expect(wrapper.emitted('select')).toEqual([['a'], ['e']])
    const escape = new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true })
    const outside = vi.fn()
    document.body.addEventListener('keydown', outside)
    document.activeElement!.dispatchEvent(escape)
    document.body.removeEventListener('keydown', outside)
    await nextTick()
    expect(last(wrapper.emitted('select') ?? [])).toEqual([null])
    // Escape that cleared a selection stops there, so it never also closes a modal.
    expect(outside).not.toHaveBeenCalled()
    // Arrow keys never scroll or pan while the graph has focus.
    const arrow = new KeyboardEvent('keydown', { key: 'ArrowUp', bubbles: true, cancelable: true })
    document.activeElement!.dispatchEvent(arrow)
    expect(arrow.defaultPrevented).toBe(true)
    expect(wrapper.emitted('update:camera')).toBeUndefined()
  })

  it('lights a hovered dot and its neighbours until the pointer leaves, keeping the selection lit', async () => {
    const wrapper = setup()
    await dot(wrapper, CALCULUS).trigger('pointerenter')
    expect(faded(wrapper)).toEqual(['a', 'd', 'e'])
    expect(wrapper.emitted('active')).toEqual([['c']])
    await dot(wrapper, CALCULUS).trigger('pointerleave')
    expect(faded(wrapper)).toEqual([])
    expect(wrapper.emitted('active')).toEqual([['c'], [null]])
    await dot(wrapper, ESSAYS).trigger('click')
    expect(wrapper.emitted('select')).toEqual([['e']])
    expect(faded(wrapper)).toEqual(['a', 'b', 'c', 'd'])
  })

  it('names the lit set, every shown dot past the zoom threshold or when narrowed, and every dot with Names on', async () => {
    const names = (wrapper: VueWrapper) => wrapper.findAll('.web-name').map(item => item.text()).sort()
    const wrapper = setup()
    expect(names(wrapper)).toEqual([])
    await wrapper.setProps({ selectedId: 'b' })
    expect(names(wrapper)).toEqual(['Algebra', 'Bridges', 'Calculus'])
    await dot(wrapper, DATA).trigger('pointerenter')
    expect(names(wrapper)).toEqual(['Algebra', 'Bridges', 'Calculus', 'Data'])
    await dot(wrapper, DATA).trigger('pointerleave')
    await wrapper.setProps({ selectedId: null, camera: { ...CAMERA, zoom: 2.5 } })
    expect(names(wrapper)).toEqual(['Algebra', 'Bridges', 'Calculus', 'Data', 'Essays'])
    await wrapper.setProps({ camera: CAMERA, names: true })
    expect(names(wrapper)).toEqual(['Algebra', 'Bridges', 'Calculus', 'Data', 'Essays'])
    await wrapper.setProps({ names: false, narrowed: true, shownIds: new Set(['a', 'b']) })
    expect(names(wrapper)).toEqual(['Algebra', 'Bridges'])
    expect(dots(wrapper)).toHaveLength(2)
  })

  it('draws prerequisites as lines with arrowheads when lit or with Arrows, and relations as dotted lines that Related hides without moving a dot', async () => {
    const wrapper = setup()
    expect(segments(wrapper, '.web-line')).toBe(2)
    expect(segments(wrapper, '.web-relation')).toBe(1)
    expect(segments(wrapper, '.web-head')).toBe(0)
    // Lines run between dot centres in layout units, under the dots: the Algebra–Bridges line starts at Algebra (0, 0).
    const first = wrapper.get('.web-line').attributes('d')!.match(/M ([-\d.]+) ([-\d.]+)/)!
    expect([Number(first[1]), Number(first[2])]).toEqual([0, 0])
    await wrapper.setProps({ selectedId: 'b' })
    expect(segments(wrapper, '.web-line.is-lit')).toBe(2)
    expect(segments(wrapper, '.web-head.is-lit')).toBe(2)
    // An arrowhead's tip sits at the dependent dot's edge: Bridges (100, 0) builds on Algebra, so its tip lies short of 100.
    const tip = wrapper.get('.web-head.is-lit').attributes('d')!.match(/M ([-\d.]+) ([-\d.]+)/)!
    expect(Number(tip[1])).toBeLessThan(100)
    expect(Number(tip[1])).toBeGreaterThan(90)
    expect(segments(wrapper, '.web-head:not(.is-lit)')).toBe(0)
    await wrapper.setProps({ selectedId: null, arrows: true })
    expect(segments(wrapper, '.web-head')).toBe(2)
    const before = places(wrapper)
    await wrapper.setProps({ related: false })
    expect(segments(wrapper, '.web-relation')).toBe(0)
    expect(places(wrapper)).toEqual(before)
    // Without Related, a relation is not a step: hovering Algebra leaves Data faded.
    await dot(wrapper, ALGEBRA).trigger('pointerenter')
    expect(faded(wrapper)).toEqual(['c', 'd', 'e'])
  })

  it('drags a dot without selecting it, and with reduced motion moves only the held dot', async () => {
    const wrapper = setup({ reducedMotion: true })
    const held = dot(wrapper, BRIDGES).element
    await pointer(held, 'pointerdown', 450, 210)
    await pointer(held, 'pointermove', 470, 220)
    await pointer(held, 'pointermove', 480, 250)
    await pointer(held, 'pointerup', 480, 250)
    held.dispatchEvent(new MouseEvent('click', { bubbles: true, detail: 1 }))
    await nextTick()
    expect(wrapper.emitted('select')).toBeUndefined()
    const moves = dragged(wrapper)
    expect(moves).toHaveLength(2)
    expect(last(moves)!.get('b')).toEqual({ x: 130, y: 40 })
    for (const id of ['a', 'c', 'd', 'e']) expect(last(moves)!.get(id)).toEqual(positions.get(id))
    // The owner stores the drag; the dot is drawn where it was dropped.
    expect(wrapper.props('positions')).toEqual(last(moves))
    // A press without movement is a click, and selects.
    await pointer(held, 'pointerdown', 480, 250)
    await pointer(held, 'pointerup', 480, 250)
    held.dispatchEvent(new MouseEvent('click', { bubbles: true, detail: 1 }))
    await nextTick()
    expect(wrapper.emitted('select')).toEqual([['b']])
  })

  it('lets neighbours follow a dragged dot in a live run that stops within the emphasis duration of release', async () => {
    vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout', 'requestAnimationFrame', 'cancelAnimationFrame'] })
    const wrapper = setup()
    const held = dot(wrapper, BRIDGES).element
    await pointer(held, 'pointerdown', 450, 210)
    await pointer(held, 'pointermove', 550, 210)
    vi.advanceTimersByTime(500)
    await nextTick()
    const live = dragged(wrapper)
    expect(live.length).toBeGreaterThan(5)
    expect(last(live)!.get('b')).toEqual({ x: 200, y: 0 })
    // Calculus, which builds on Bridges, is pushed or pulled along.
    expect(last(live)!.get('c')).not.toEqual(positions.get('c'))
    await pointer(held, 'pointerup', 550, 210)
    vi.advanceTimersByTime(motionDurationForTest())
    await nextTick()
    const settled = dragged(wrapper).length
    vi.advanceTimersByTime(2000)
    await nextTick()
    expect(dragged(wrapper).length).toBe(settled)
    expect(last(dragged(wrapper))!.get('b')).toEqual({ x: 200, y: 0 })
  })

  it('pans by dragging the background, zooms with the wheel about the pointer, and pinches on touch', async () => {
    const wrapper = setup()
    const canvas = wrapper.get('[role="group"]').element
    await pointer(canvas, 'pointerdown', 100, 100)
    await pointer(canvas, 'pointermove', 140, 80)
    await pointer(canvas, 'pointerup', 140, 80)
    expect(wrapper.props('camera')).toEqual({ x: 10, y: 70, zoom: 1 })
    expect(wrapper.emitted('drag')).toBeUndefined()
    // In the full graph a plain wheel scrolls the page; a pinch on a trackpad (ctrl + wheel) zooms.
    const wheel = (options: WheelEventInit) => { const event = new WheelEvent('wheel', { bubbles: true, cancelable: true, clientX: 400, clientY: 260, ...options }); canvas.dispatchEvent(event); return event }
    expect(wheel({ deltaY: -100 }).defaultPrevented).toBe(false)
    expect(wheel({ deltaY: -100, ctrlKey: true }).defaultPrevented).toBe(true)
    await nextTick()
    const zoomed = wrapper.props('camera') as AtlasCamera
    expect(zoomed.zoom).toBeGreaterThan(1)
    // The point under the pointer (the centre) stays put.
    expect(zoomed.x).toBeCloseTo(10)
    expect(zoomed.y).toBeCloseTo(70)
    // Two touches: spreading them apart zooms in about their midpoint.
    await pointer(canvas, 'pointerdown', 300, 260, 1, 'touch')
    await pointer(canvas, 'pointerdown', 500, 260, 2, 'touch')
    await pointer(canvas, 'pointermove', 200, 260, 1, 'touch')
    await pointer(canvas, 'pointermove', 600, 260, 2, 'touch')
    expect((wrapper.props('camera') as AtlasCamera).zoom).toBeCloseTo(zoomed.zoom * 2)
    await pointer(canvas, 'pointerup', 200, 260, 1, 'touch')
    await pointer(canvas, 'pointerup', 600, 260, 2, 'touch')
    expect(wrapper.emitted('drag')).toBeUndefined()
  })

  it('in compact form zooms with a plain wheel, and with no camera fits what is shown', async () => {
    const wrapper = setup({ compact: true, camera: null })
    await nextTick()
    const canvas = wrapper.get('[role="group"]').element
    // Fitted: every dot's centre inside the compact box, its place in the layer plus the layer's pan.
    const translate = (style: string | undefined) => (style?.match(/translate\(([-\d.]+)px, ([-\d.]+)px\)/) ?? []).slice(1).map(Number)
    const [panX, panY] = translate(wrapper.get('.web-layer').attributes('style'))
    for (const item of dots(wrapper)) {
      const [x, y] = translate(item.attributes('style')), half = Number.parseFloat((item.element as HTMLElement).style.width) / 2
      expect(panX! + x! + half).toBeGreaterThanOrEqual(0)
      expect(panY! + y! + half).toBeGreaterThanOrEqual(0)
      expect(panX! + x! + half).toBeLessThanOrEqual(260)
      expect(panY! + y! + half).toBeLessThanOrEqual(200)
    }
    const event = new WheelEvent('wheel', { bubbles: true, cancelable: true, deltaY: 100, clientX: 130, clientY: 100 })
    canvas.dispatchEvent(event)
    await nextTick()
    expect(event.defaultPrevented).toBe(true)
    expect(wrapper.emitted('update:camera')).toHaveLength(1)
  })

  it('in compact form leaves out a neighbour\'s name that has no free side, always shows the hovered and selected names, and never leaves one out with Names on', async () => {
    const names = (wrapper: VueWrapper) => wrapper.findAll('.web-name').map(item => item.text()).sort()
    // A hub and six skills that build on it, all on one spot: around one spot
    // at most four names fit, one to a side.
    const spokes = ['n1', 'n2', 'n3', 'n4', 'n5', 'n6']
    const hub = buildSkillAtlas({
      skills: [skill('h', 'Hub', 'math'), ...spokes.map(id => skill(id, `Spoke ${id}`, 'math', { prerequisite_count: 1 }))],
      edges: spokes.map(id => edge(id, 'h')), relations: [], activeCredentials: [], subjectDid: 'did:me',
    })
    const spot: ReadonlyMap<string, WebPoint> = new Map(['h', ...spokes].map((id, index) => [id, { x: index * 0.5, y: 0 }]))
    const wrapper = setup({ model: hub, compact: true, positions: spot, selectedId: 'h' })
    const shown = names(wrapper)
    expect(shown).toContain('Hub')
    expect(shown.length).toBeGreaterThan(1)
    expect(shown.length).toBeLessThan(7)
    // A hovered spoke whose name was left out shows, beside the selected name.
    const hidden = spokes.find(id => !shown.includes(`Spoke ${id}`))!
    const hiddenDot = wrapper.get(`[data-skill="${hidden}"]`)
    await hiddenDot.trigger('pointerenter')
    expect(names(wrapper)).toEqual(expect.arrayContaining(['Hub', `Spoke ${hidden}`]))
    await hiddenDot.trigger('pointerleave')
    expect(names(wrapper)).toEqual(shown)
    // With Names on, and in the full graph, every lit name shows, moved further out where it must.
    await wrapper.setProps({ names: true })
    expect(names(wrapper)).toHaveLength(7)
    await wrapper.setProps({ names: false, compact: false })
    expect(names(wrapper)).toHaveLength(7)
  })

  it('fades dots outside a search or field match, and lights an owner-given neighbourhood in place of the selection', async () => {
    const wrapper = setup({ matchIds: new Set(['a', 'b']) })
    expect(faded(wrapper)).toEqual(['c', 'd', 'e'])
    // The selection never fades, even when it does not match.
    await wrapper.setProps({ selectedId: 'e' })
    expect(faded(wrapper)).toEqual(['a', 'b', 'c', 'd'])
    await wrapper.setProps({ matchIds: null, selectedId: 'c', focusIds: new Set(['a', 'b', 'c']) })
    expect(faded(wrapper)).toEqual(['d', 'e'])
    expect(segments(wrapper, '.web-line.is-lit')).toBe(2)
  })
})
