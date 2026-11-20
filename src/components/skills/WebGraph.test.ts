import { mount, type VueWrapper } from '@vue/test-utils'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { nextTick } from 'vue'
import { createI18n } from 'vue-i18n'
import en from '@/locales/en'
import { buildSkillAtlas } from '@/utils/skillAtlas'
import type { AtlasCamera } from '@/utils/skillAtlasView'
import type { SkillInfo, VerifiableCredential } from '@/types'
import type { WebPoint } from './webGeometry'
import WebGraph from './WebGraph.vue'

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

const faded = (wrapper: VueWrapper) => dots(wrapper).filter(item => item.find('.is-faded').exists()).map(item => item.attributes('data-skill')).sort()
const focusedSkill = () => (document.activeElement as HTMLElement | null)?.dataset.skill
const key = async (wrapper: VueWrapper, name: string) => {
  const target = (document.activeElement as HTMLElement | null) ?? wrapper.element
  target.dispatchEvent(new KeyboardEvent('keydown', { key: name, bubbles: true, cancelable: true }))
  await nextTick()
}

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
    dot(wrapper, ALGEBRA).element.focus()
    await nextTick()
    await key(wrapper, 'ArrowRight')
    expect(focusedSkill()).toBe('b')
    expect(dot(wrapper, BRIDGES).attributes('tabindex')).toBe('0')
    expect(dot(wrapper, ALGEBRA).attributes('tabindex')).toBe('-1')
    // Bridges builds on Algebra and leads to Calculus.
    expect(faded(wrapper)).toEqual(['d', 'e'])
    expect(wrapper.emitted('active')?.at(-1)).toEqual(['b'])
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
    expect(wrapper.emitted('select')?.at(-1)).toEqual([null])
    // Escape that cleared a selection stops there, so it never also closes a modal.
    expect(outside).not.toHaveBeenCalled()
    // Arrow keys never scroll or pan while the graph has focus.
    const arrow = new KeyboardEvent('keydown', { key: 'ArrowUp', bubbles: true, cancelable: true })
    document.activeElement!.dispatchEvent(arrow)
    expect(arrow.defaultPrevented).toBe(true)
    expect(wrapper.emitted('update:camera')).toBeUndefined()
  })
})
