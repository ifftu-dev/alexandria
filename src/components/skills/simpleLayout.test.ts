import { describe, expect, it } from 'vitest'
import { buildSkillAtlas } from '@/utils/skillAtlas'
import type { SkillGraphEdge, SkillInfo, VerifiableCredential } from '@/types'
import { NARROW_MAP_WIDTH } from './atlasLayout'
import { SIMPLE_DISC_SIZE, SIMPLE_LABEL_MIN_PITCH, SIMPLE_MIN_DISC_PITCH, SIMPLE_ROW_PITCH, SIMPLE_WIDE_HEIGHT, isDenseLayer, simpleLayout } from './simpleLayout'

const skill = (id: string, name: string, field: string | null): SkillInfo => ({
  id, name, description: '', subject_id: 'subject', subject_name: 'Subject',
  subject_field_id: field, subject_field_name: field, bloom_level: 'apply',
  prerequisite_count: 0, dependent_count: 0, created_at: null,
})
const edge = (skillId: string, prerequisiteId: string): SkillGraphEdge => ({
  skill_id: skillId, skill_name: skillId, skill_bloom: 'apply',
  prerequisite_id: prerequisiteId, prerequisite_name: prerequisiteId, prerequisite_bloom: 'apply',
})
const credential = (skillId: string, level: number): VerifiableCredential => ({
  '@context': [], type: [], issuer: 'did:issuer', validFrom: '',
  credentialSubject: { id: 'did:me', skillId, level },
  proof: { type: '', cryptosuite: '', created: '', verificationMethod: '', proofPurpose: '', proofValue: '' },
})
const SKILLS = [
  skill('m1', 'Numbers', 'math'), skill('d1', 'Counting', 'data'),
  skill('m2', 'Algebra', 'math'), skill('m4', 'Angles', 'math'), skill('d2', 'Abacus', 'data'), skill('u1', 'Craft', null),
  skill('m3', 'Calculus', 'math'),
]
// Calculus also names Numbers directly; the longer path through Algebra wins.
const EDGES = [edge('m2', 'm1'), edge('m4', 'm1'), edge('d2', 'm1'), edge('u1', 'd1'), edge('m3', 'm2'), edge('m3', 'm1'), edge('m3', 'unloaded')]
const atlas = (skills = SKILLS, edges = EDGES, activeCredentials: VerifiableCredential[] = []) => buildSkillAtlas({
  skills, edges, activeCredentials, subjectDid: 'did:me',
  subjectFields: [{ id: 'math', name: 'Mathematics', atlas_slot: 0 }, { id: 'data', name: 'Data', atlas_slot: 1 }],
})
const WIDE = 1200
/** Three layers of 40, 9 and 25 skills: each follower needs one root. */
function crowded() {
  const roots = Array.from({ length: 40 }, (_, index) => skill(`r${String(index).padStart(2, '0')}`, `Root ${index}`, index % 2 ? 'math' : 'data'))
  const middle = Array.from({ length: 9 }, (_, index) => skill(`m${index}`, `Middle ${index}`, 'math'))
  const leaves = Array.from({ length: 25 }, (_, index) => skill(`l${String(index).padStart(2, '0')}`, `Leaf ${index}`, 'data'))
  const edges = [...middle.map((item, index) => edge(item.id, `r${String(index).padStart(2, '0')}`)), ...leaves.map((item, index) => edge(item.id, `m${index % 9}`))]
  return atlas([...roots, ...middle, ...leaves], edges)
}
// A name starts 6px under its disc and runs to two 12px lines at 1.4 line
// height plus 1px padding each side (stylesheet); a ready-next disc's dashed
// halo reaches 6px past the disc.
const NAME_TOP = SIMPLE_DISC_SIZE / 2 + 6
const NAME_HEIGHT = 36
const HALO = 6
interface Rect { left: number; right: number; top: number; bottom: number }
const intersects = (a: Rect, b: Rect) => a.left < b.right && b.left < a.right && a.top < b.bottom && b.top < a.bottom

describe('Simple layout', () => {
  it('layers skills by longest prerequisite path, ordered by field slot then name within a layer', () => {
    expect(simpleLayout(atlas(), WIDE).layers).toEqual([['m1', 'd1'], ['m2', 'm4', 'd2', 'u1'], ['m3']])
  })

  it('runs layers left to right on a wide viewport, each spread evenly down the fixed height', () => {
    const layout = simpleLayout(atlas(), WIDE)
    const at = (id: string) => layout.positions.get(id) ?? { x: NaN, y: NaN }
    expect(layout.layers.map(layer => layer.map(id => at(id).x))).toEqual([[200, 200], [600, 600, 600, 600], [1000]])
    for (const layer of layout.layers) {
      const ys = layer.map(id => at(id).y)
      const gaps = ys.slice(1).map((y, index) => y - (ys[index] ?? NaN))
      gaps.forEach(gap => expect(gap).toBeCloseTo(SIMPLE_WIDE_HEIGHT / layer.length))
      expect((ys[0] ?? NaN) + (ys[ys.length - 1] ?? NaN)).toBeCloseTo(SIMPLE_WIDE_HEIGHT)
    }
    const lone = simpleLayout(atlas([skill('solo', 'Solo', 'math')], []), WIDE)
    expect(lone.positions.get('solo')).toEqual({ x: WIDE / 2, y: SIMPLE_WIDE_HEIGHT / 2 })
    expect(layout.height).toBe(lone.height)
    expect(layout.height).toBeGreaterThan(SIMPLE_WIDE_HEIGHT)
  })

  it('runs layers top to bottom below the narrow-map width, staggering alternate nodes only in layers of more than three', () => {
    const phone = 360
    const layout = simpleLayout(atlas(), phone)
    const at = (id: string) => layout.positions.get(id) ?? { x: NaN, y: NaN }
    const rowY = layout.layers.map(layer => Math.min(...layer.map(id => at(id).y)))
    expect(rowY[1]! - rowY[0]!).toBe(SIMPLE_ROW_PITCH)
    expect(rowY[2]! - rowY[1]!).toBe(SIMPLE_ROW_PITCH)
    for (const layer of layout.layers) {
      const xs = layer.map(id => at(id).x)
      const gaps = xs.slice(1).map((x, index) => x - (xs[index] ?? NaN))
      gaps.forEach(gap => { expect(gap).toBeGreaterThan(0); expect(gap).toBeCloseTo(gaps[0] ?? NaN) })
      expect((xs[0] ?? NaN) + (xs[xs.length - 1] ?? NaN)).toBeCloseTo(phone)
    }
    // Layer 1 holds four skills: odd positions drop by one shared offset.
    const four = layout.layers[1]!.map(id => at(id).y)
    expect(four[0]).toBe(four[2])
    expect(four[1]).toBe(four[3])
    expect(four[1]!).toBeGreaterThan(four[0]!)
    for (const small of [layout.layers[0]!, layout.layers[2]!]) expect(new Set(small.map(id => at(id).y)).size).toBe(1)

    const lowest = Math.max(...[...layout.positions.values()].map(point => point.y))
    expect(layout.height).toBeGreaterThan(lowest + SIMPLE_DISC_SIZE / 2)
    const oneRow = simpleLayout(atlas([skill('solo', 'Solo', 'math')], []), phone)
    expect(layout.height - oneRow.height).toBe(2 * SIMPLE_ROW_PITCH)

    // The first layer (Numbers, Counting) is a row just below the threshold and a column at it.
    const justNarrow = simpleLayout(atlas(), NARROW_MAP_WIDTH - 1).positions
    expect(justNarrow.get('m1')?.y).toBe(justNarrow.get('d1')?.y)
    const justWide = simpleLayout(atlas(), NARROW_MAP_WIDTH).positions
    expect(justWide.get('m1')?.x).toBe(justWide.get('d1')?.x)
  })

  it('grows a wide layout so no layer packs its discs closer than a disc and its label, centring smaller layers', () => {
    expect(SIMPLE_MIN_DISC_PITCH).toBeGreaterThan(SIMPLE_DISC_SIZE)
    const roots = Array.from({ length: 14 }, (_, index) => skill(`r${String(index).padStart(2, '0')}`, `Root ${index}`, 'math'))
    const followers = [skill('f1', 'Follow 1', 'math'), skill('f2', 'Follow 2', 'math')]
    const layout = simpleLayout(atlas([...roots, ...followers], [edge('f1', 'r00'), edge('f2', 'r01')]), WIDE)
    const at = (id: string) => layout.positions.get(id) ?? { x: NaN, y: NaN }
    const big = layout.layers[0]!.map(id => at(id).y)
    const gaps = big.slice(1).map((y, index) => y - big[index]!)
    gaps.forEach(gap => expect(gap).toBeGreaterThanOrEqual(SIMPLE_MIN_DISC_PITCH))
    // The fixed height is the floor: the tall layer pushes the layout past it.
    const extent = 14 * SIMPLE_MIN_DISC_PITCH
    expect(extent).toBeGreaterThan(SIMPLE_WIDE_HEIGHT)
    expect(big[0]! + big[big.length - 1]!).toBeCloseTo(extent)
    expect(layout.height).toBeGreaterThan(big[big.length - 1]! + SIMPLE_DISC_SIZE / 2)
    // The two-node layer keeps its usual spacing, centred on the taller extent.
    const small = layout.layers[1]!.map(id => at(id).y)
    expect(small[1]! - small[0]!).toBeCloseTo(SIMPLE_WIDE_HEIGHT / 2)
    expect(small[0]! + small[1]!).toBeCloseTo(extent)
    // A crowded wide layer grows rather than holding names back.
    expect(isDenseLayer(14, WIDE)).toBe(false)
  })

  it('wraps a crowded narrow layer onto extra rows so no two discs overlap down to 320px', () => {
    const model = crowded()
    for (let width = 320; width < NARROW_MAP_WIDTH; width += 7) {
      const layout = simpleLayout(model, width)
      const points = [...layout.positions.values()]
      expect(points).toHaveLength(74)
      for (const [index, a] of points.entries()) {
        expect(a.x - SIMPLE_DISC_SIZE / 2).toBeGreaterThanOrEqual(0)
        expect(a.x + SIMPLE_DISC_SIZE / 2).toBeLessThanOrEqual(width)
        for (const b of points.slice(index + 1)) expect(Math.hypot(a.x - b.x, a.y - b.y)).toBeGreaterThanOrEqual(SIMPLE_DISC_SIZE)
      }
      // Layers still read top to bottom: every disc of a later layer sits below every disc of an earlier one.
      const ys = layout.layers.map(layer => layer.map(id => layout.positions.get(id)!.y))
      for (let layer = 1; layer < ys.length; layer++) expect(Math.min(...ys[layer]!)).toBeGreaterThan(Math.max(...ys[layer - 1]!) + SIMPLE_DISC_SIZE)
      const lowest = Math.max(...points.map(point => point.y))
      expect(layout.height).toBeGreaterThan(lowest + SIMPLE_DISC_SIZE / 2)
    }
  })

  it('gives every name a width that stays inside the map and clear of other discs and names, at every width', () => {
    const chain = atlas(Array.from({ length: 7 }, (_, index) => skill(`c${index}`, `Chain ${index}`, 'math')),
      Array.from({ length: 6 }, (_, index) => edge(`c${index + 1}`, `c${index}`)))
    for (const model of [crowded(), atlas(), chain]) {
      for (let width = 320; width <= 1400; width += 9) {
        const layout = simpleLayout(model, width)
        const ids = [...layout.positions.keys()]
        const name = (id: string): Rect => {
          const { x, y } = layout.positions.get(id)!
          const half = (layout.labelWidths.get(id) ?? NaN) / 2
          return { left: x - half, right: x + half, top: y + NAME_TOP, bottom: y + NAME_TOP + NAME_HEIGHT }
        }
        const disc = (id: string): Rect => {
          const { x, y } = layout.positions.get(id)!
          const reach = SIMPLE_DISC_SIZE / 2 + HALO
          return { left: x - reach, right: x + reach, top: y - reach, bottom: y + reach }
        }
        const problems: string[] = []
        for (const id of ids) {
          // At least as wide as the disc, never past the map's sides.
          if (!((layout.labelWidths.get(id) ?? 0) >= SIMPLE_DISC_SIZE)) problems.push(`${id} name narrower than its disc`)
          // On a phone a name has room for a long word ("Accessibility" at 12px) rather than breaking inside it.
          if (width < NARROW_MAP_WIDTH && !((layout.labelWidths.get(id) ?? 0) >= 88)) problems.push(`${id} name too narrow for a word`)
          if (name(id).left < 0 || name(id).right > width) problems.push(`${id} name outside the map`)
          for (const other of ids) {
            if (other === id) continue
            if (intersects(name(id), disc(other))) problems.push(`${id} name over ${other} disc`)
            if (intersects(name(id), name(other))) problems.push(`${id} name over ${other} name`)
          }
        }
        expect(problems.slice(0, 3), `at ${width}px`).toEqual([])
      }
    }
  })

  it('places every skill exactly once at both widths', () => {
    const ids = SKILLS.map(item => item.id).sort()
    for (const width of [WIDE, 360]) {
      const layout = simpleLayout(atlas(), width)
      expect([...layout.positions.keys()].sort()).toEqual(ids)
      expect(layout.layers.flat().sort()).toEqual(ids)
      const points = [...layout.positions.values()].map(point => `${point.x},${point.y}`)
      expect(new Set(points).size).toBe(ids.length)
      for (const point of layout.positions.values()) {
        expect(point.x).toBeGreaterThan(0)
        expect(point.x).toBeLessThan(width)
      }
    }
  })

  it('survives a prerequisite cycle, placing every skill without an empty layer', () => {
    // Cycle protection matches the public graph: x and y require each other, z requires x.
    const cyclic = atlas([skill('x', 'Xray', 'math'), skill('y', 'Yank', 'math'), skill('z', 'Zulu', 'data')], [edge('x', 'y'), edge('y', 'x'), edge('z', 'x')])
    const wide = simpleLayout(cyclic, WIDE)
    expect(wide.layers).toEqual([['y'], ['x'], ['z']])
    expect(wide.positions.get('y')).toEqual({ x: 200, y: SIMPLE_WIDE_HEIGHT / 2 })
    expect(simpleLayout(cyclic, 360).positions.size).toBe(3)
  })

  it('calls a layer dense exactly when its nodes sit closer than the label legibility threshold', () => {
    for (const width of [WIDE, 360]) {
      const verdicts = new Set<boolean>()
      for (let count = 1; count <= 12; count++) {
        const roots = Array.from({ length: count }, (_, index) => skill(`r${String(index).padStart(2, '0')}`, `Root ${index}`, 'math'))
        const layout = simpleLayout(atlas(roots, []), width)
        const [first, second] = (layout.layers[0] ?? []).map(id => layout.positions.get(id) ?? { x: NaN, y: NaN })
        // Pitch runs along the layer: down a wide column, across a narrow row (ignoring stagger).
        const pitch = first && second ? (width < NARROW_MAP_WIDTH ? second.x - first.x : second.y - first.y) : Infinity
        expect(isDenseLayer(count, width)).toBe(pitch < SIMPLE_LABEL_MIN_PITCH)
        verdicts.add(isDenseLayer(count, width))
      }
      // Wide layers grow past crowding instead, so only narrow rows ever thin their names.
      expect([...verdicts].sort()).toEqual(width < NARROW_MAP_WIDTH ? [false, true] : [false])
    }
  })

  it('depends only on the taxonomy: repeat calls, input order and credentials never move a node', () => {
    for (const width of [WIDE, 360]) {
      const baseline = simpleLayout(atlas(), width)
      expect(simpleLayout(atlas(), width)).toEqual(baseline)
      expect(simpleLayout(atlas([...SKILLS].reverse(), [...EDGES].reverse()), width)).toEqual(baseline)
      expect(simpleLayout(atlas(SKILLS, EDGES, [credential('m1', 3), credential('m2', 1), credential('u1', 5)]), width)).toEqual(baseline)
    }
  })
})
