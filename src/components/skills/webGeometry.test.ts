import { describe, expect, it } from 'vitest'
import taxonomy from '../../../bootstrap/public_taxonomy.json'
import { scaleTaxonomy } from '../../../tests/desktop-ui/atlasBackend'
import { WEB_MIN_DISTANCE, continueWebLayout, webLayout, type WebGraph } from './webGeometry'

type Point = { x: number; y: number }

/** A taxonomy as the Web layout reads it: skills with their field's slot, prerequisites and relations. */
function graphOf(
  skills: readonly { id: string; subject_id: string }[],
  prerequisites: readonly { skill_id: string; prerequisite_id: string }[],
  relations: readonly { skill_id: string; related_skill_id: string }[] = [],
): WebGraph {
  const fieldOfSubject = new Map(taxonomy.subjects.map(subject => [subject.id, subject.subject_field_id]))
  const slotOfField = new Map(taxonomy.subject_fields.map(field => [field.id, field.atlas_slot]))
  return {
    nodes: skills.map(skill => ({ id: skill.id, slot: slotOfField.get(fieldOfSubject.get(skill.subject_id) ?? '') ?? null })),
    prerequisites: prerequisites.map(edge => ({ skill: edge.skill_id, prerequisite: edge.prerequisite_id })),
    relations: relations.map(relation => [relation.skill_id, relation.related_skill_id] as const),
  }
}
const BUNDLED = graphOf(taxonomy.skills, taxonomy.skill_prerequisites, taxonomy.skill_relations)
const scale = scaleTaxonomy()
const SCALE = graphOf(scale.skills, scale.edges, taxonomy.skill_relations)

describe('Web layout', () => {
  it('places every skill once at finite coordinates, the same on every call', () => {
    const layout = webLayout(BUNDLED)
    expect([...layout.keys()].sort()).toEqual(taxonomy.skills.map(skill => skill.id).sort())
    for (const point of layout.values()) {
      expect(Number.isFinite(point.x)).toBe(true)
      expect(Number.isFinite(point.y)).toBe(true)
    }
    expect(webLayout(BUNDLED)).toEqual(layout)
  })

  it('depends only on the graph: input order and duplicate links never move a skill', () => {
    const shuffled: WebGraph = {
      nodes: [...BUNDLED.nodes].reverse(),
      prerequisites: [...BUNDLED.prerequisites, ...BUNDLED.prerequisites.slice(0, 5)].reverse(),
      relations: BUNDLED.relations.map(([a, b]) => [b, a] as const).reverse(),
    }
    expect(webLayout(shuffled)).toEqual(webLayout(BUNDLED))
  })

  it('keeps every dot clear of the others and the whole web in a bounded disc around the origin', () => {
    for (const graph of [BUNDLED, SCALE]) {
      const points = [...webLayout(graph).values()]
      // About one dot per 44-unit square: the web grows with the square root of its size.
      const radius = 25 * Math.sqrt(points.length)
      const xs = points.map(point => point.x), ys = points.map(point => point.y)
      expect(Math.max(...points.map(point => Math.hypot(point.x, point.y)))).toBeLessThan(radius)
      expect(Math.abs(Math.min(...xs) + Math.max(...xs))).toBeLessThan(1e-6)
      expect(Math.abs(Math.min(...ys) + Math.max(...ys))).toBeLessThan(1e-6)
      expect(closestPair(points)).toBeGreaterThanOrEqual(WEB_MIN_DISTANCE)
    }
    // A single skill sits at the origin; an empty graph places nothing.
    expect(webLayout({ nodes: [{ id: 'solo', slot: 0 }], prerequisites: [], relations: [] })).toEqual(new Map([['solo', { x: 0, y: 0 }]]))
    expect(webLayout({ nodes: [], prerequisites: [], relations: [] }).size).toBe(0)
  })

  it('gathers each field into a cluster: skills of one field sit closer than skills of different fields', () => {
    // Half the links here cross fields, so the field pull has to hold each field together.
    const id = (index: number) => `s${String(index % 60).padStart(2, '0')}`
    const crossLinked: WebGraph = {
      nodes: Array.from({ length: 60 }, (_, index) => ({ id: id(index), slot: index % 3 })),
      prerequisites: Array.from({ length: 60 }, (_, index) => ({ skill: id(index), prerequisite: id(index + (index % 2 ? 1 : 3)) })),
      relations: [],
    }
    for (const [graph, factor] of [[BUNDLED, 2], [SCALE, 2], [crossLinked, 1.5]] as const) {
      const layout = webLayout(graph)
      const slot = new Map(graph.nodes.map(node => [node.id, node.slot]))
      const same = meanDistance(layout, (a, b) => slot.get(a) === slot.get(b))
      const different = meanDistance(layout, (a, b) => slot.get(a) !== slot.get(b))
      expect(same * factor).toBeLessThan(different)
    }
  })

  it('always lets relations shape the web, so drawing them or not can never move a dot', () => {
    // The layout takes no drawing toggle: the Related toggle only hides lines. Relations pull their skills together.
    const withRelations = webLayout(BUNDLED)
    const without = webLayout({ ...BUNDLED, relations: [] })
    const related = (a: string, b: string) => BUNDLED.relations.some(([p, q]) => pairKey(p, q) === pairKey(a, b))
    expect(meanDistance(withRelations, related)).toBeLessThan(meanDistance(without, related))
  })

  it('lays the 1,500-skill scale taxonomy out inside the time budget', () => {
    const graph = SCALE
    webLayout(graph)
    const times = [0, 1, 2].map(() => {
      const started = performance.now()
      webLayout(graph)
      return performance.now() - started
    })
    console.log('web layout, 1,500 skills (ms):', times.map(time => time.toFixed(1)).join(', '))
    expect(Math.min(...times)).toBeLessThan(50)
  })

  it('draws linked skills closer together than unlinked ones of the same field, on average', () => {
    const layout = webLayout(BUNDLED)
    const linked = new Set([
      ...BUNDLED.prerequisites.map(edge => pairKey(edge.skill, edge.prerequisite)),
      ...BUNDLED.relations.map(([a, b]) => pairKey(a, b)),
    ])
    const sameField = (a: string, b: string) => slotOf(BUNDLED, a) === slotOf(BUNDLED, b)
    const near = meanDistance(layout, (a, b) => sameField(a, b) && linked.has(pairKey(a, b)))
    const far = meanDistance(layout, (a, b) => sameField(a, b) && !linked.has(pairKey(a, b)))
    // Inside one field the springs, not the field pull, set who sits beside whom.
    expect(near * 1.2).toBeLessThan(far)
  })
})

describe('Continuing the Web layout', () => {
  // Vue.js builds on JavaScript and on nothing else: drag JavaScript aside and hold it there.
  const held = 'skill_javascript'
  const neighbour = 'skill_vue'
  const rest = webLayout(BUNDLED)
  const dragged = new Map(rest)
  dragged.set(held, { x: rest.get(held)!.x + 120, y: rest.get(held)!.y })

  it('keeps a held skill exactly where it is put while its neighbours follow it', () => {
    // A second of frames, one call per frame as a live drag makes them.
    let next: Map<string, Point> = dragged
    for (let frame = 0; frame < 60; frame++) next = continueWebLayout(BUNDLED, next, { pinned: new Set([held]) })
    expect(next.get(held)).toEqual(dragged.get(held))
    const gap = (layout: ReadonlyMap<string, Point>) => distance(layout.get(held)!, layout.get(neighbour)!)
    expect(gap(next)).toBeLessThan(gap(dragged) * 0.85)
    expect([...next.keys()].sort()).toEqual([...rest.keys()].sort())
  })

  it('moves nothing much when continued from rest, and repeats exactly', () => {
    const next = continueWebLayout(BUNDLED, rest)
    for (const [id, point] of next) expect(distance(point, rest.get(id)!)).toBeLessThan(2)
    expect(continueWebLayout(BUNDLED, dragged, { pinned: new Set([held]) })).toEqual(continueWebLayout(BUNDLED, dragged, { pinned: new Set([held]) }))
  })

  it('starts a skill it has no position for from the seeded start, and ignores positions of skills not in the graph', () => {
    const partial = new Map([...rest].filter(([id]) => id !== neighbour))
    partial.set('not-a-skill', { x: 0, y: 0 })
    const next = continueWebLayout(BUNDLED, partial)
    expect(next.has('not-a-skill')).toBe(false)
    expect(Number.isFinite(next.get(neighbour)!.x)).toBe(true)
  })
})

function distance(a: Point, b: Point): number {
  return Math.hypot(a.x - b.x, a.y - b.y)
}

/** The smallest distance between any two points, by sweeping them in x order. */
function closestPair(points: readonly Point[]): number {
  const sorted = [...points].sort((a, b) => a.x - b.x)
  let best = Infinity
  for (const [index, a] of sorted.entries()) {
    for (const b of sorted.slice(index + 1)) {
      if (b.x - a.x >= best) break
      best = Math.min(best, Math.hypot(a.x - b.x, a.y - b.y))
    }
  }
  return best
}

function slotOf(graph: WebGraph, id: string): number | null | undefined {
  return graph.nodes.find(node => node.id === id)?.slot
}

function pairKey(a: string, b: string): string {
  return a < b ? `${a}|${b}` : `${b}|${a}`
}

/** Mean distance over every unordered pair of placed skills that `include` accepts. */
function meanDistance(layout: ReadonlyMap<string, Point>, include: (a: string, b: string) => boolean): number {
  const entries = [...layout]
  let total = 0, count = 0
  for (const [index, [a, pa]] of entries.entries()) {
    for (const [b, pb] of entries.slice(index + 1)) {
      if (!include(a, b)) continue
      total += Math.hypot(pa.x - pb.x, pa.y - pb.y)
      count++
    }
  }
  return count ? total / count : NaN
}
