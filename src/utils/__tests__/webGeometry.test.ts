import { describe, expect, it } from 'vitest'
import taxonomy from '../../../bootstrap/public_taxonomy.json'
import { scaleTaxonomy } from '../../../tests/desktop-ui/atlasBackend'
import type { SkillGraphEdge, SkillInfo } from '@/types'
import { buildSkillAtlas } from '@/utils/skillAtlas'
import {
  WEB_MIN_DISTANCE, WEB_SIDEBAR_NODE_LIMIT, continueWebLayout, webDotRadius, webLayout, webNearestInDirection, webNearestTo, webSidebarSubset,
  webGraphFromAtlas, webLayoutEntry, webNamePlaces, webNarrowedIds,
  type WebGraph, type WebNamePlacement,
} from '../webGeometry'

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
    // The atlas budget is no main-thread task over 50 ms; this runs near 40 ms
    // on a laptop, and the bound allows twice that for shared CI runners. A
    // layout that lost its grid and went quadratic takes seconds on every run.
    // Other test files share the machine, so the run is timed in this
    // process's CPU time, which waiting for a core does not add to, and the
    // best of up to ten runs must meet the bound.
    const BUDGET_MS = 100
    const cpuMs = () => { const { user, system } = process.cpuUsage(); return (user + system) / 1000 }
    webLayout(SCALE)
    let best = Infinity
    for (let run = 0; run < 10 && best >= BUDGET_MS; run++) {
      const started = cpuMs()
      webLayout(SCALE)
      best = Math.min(best, cpuMs() - started)
    }
    expect(best).toBeLessThan(BUDGET_MS)
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

describe('Web graph from the atlas model', () => {
  const skill = (id: string, field: string | null): SkillInfo => ({
    id, name: id, description: '', subject_id: 'subject', subject_name: 'Subject',
    subject_field_id: field, subject_field_name: field, bloom_level: 'apply',
    prerequisite_count: 0, dependent_count: 0, created_at: null,
  })
  const edge = (skillId: string, prerequisiteId: string): SkillGraphEdge => ({
    skill_id: skillId, skill_name: skillId, skill_bloom: 'apply',
    prerequisite_id: prerequisiteId, prerequisite_name: prerequisiteId, prerequisite_bloom: 'apply',
  })
  const input = {
    skills: [skill('m1', 'math'), skill('m2', 'math'), skill('d1', 'data'), skill('u1', null)],
    edges: [edge('m2', 'm1'), edge('d1', 'm1'), edge('u1', 'unloaded')],
    relations: [{ skill_id: 'm2', related_skill_id: 'd1', relation_type: 'related' }, { skill_id: 'u1', related_skill_id: 'unloaded', relation_type: 'alternative' }],
    activeCredentials: [], subjectDid: 'did:me',
    subjectFields: [{ id: 'math', name: 'Mathematics', atlas_slot: 3 }, { id: 'data', name: 'Data', atlas_slot: 0 }],
  }
  const model = buildSkillAtlas(input)

  it('carries every skill with its field slot, and the prerequisites and relations between loaded skills, each relation once', () => {
    expect(webGraphFromAtlas(model)).toEqual({
      nodes: [{ id: 'd1', slot: 0 }, { id: 'm1', slot: 3 }, { id: 'm2', slot: 3 }, { id: 'u1', slot: null }],
      prerequisites: [{ skill: 'm2', prerequisite: 'm1' }, { skill: 'd1', prerequisite: 'm1' }],
      relations: [['d1', 'm2']],
    })
  })

  it('computes the graph and layout once per model, and a new model of the same graph reuses them', () => {
    const entry = webLayoutEntry(model)
    expect(webLayoutEntry(model)).toBe(entry)
    expect(entry.graph).toEqual(webGraphFromAtlas(model))
    expect(entry.layout).toEqual(webLayout(entry.graph))
    // A credential refresh builds a new model of the same graph.
    const refreshed = buildSkillAtlas({ ...input, activeCredentials: [] })
    expect(refreshed).not.toBe(model)
    expect(webLayoutEntry(refreshed).layout).toBe(entry.layout)
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

describe('Web dot size', () => {
  it('grows by even steps of area across the six Bloom ranks, from a smaller base for unearned or unknown levels', () => {
    const radii = ([0, 1, 2, 3, 4, 5] as const).map(rank => webDotRadius(rank))
    const areas = radii.map(radius => Math.PI * radius * radius)
    const steps = areas.slice(1).map((area, index) => area - areas[index]!)
    for (const step of steps) {
      expect(step).toBeGreaterThan(0)
      expect(step).toBeCloseTo(steps[0]!, 6)
    }
    expect(webDotRadius(null)).toBeLessThan(radii[0]!)
    expect(webDotRadius(undefined)).toBe(webDotRadius(null))
    // The biggest dots still never touch: the layout keeps centres further apart than two of them.
    expect(2 * radii[5]!).toBeLessThan(WEB_MIN_DISTANCE)
  })
})

describe('Keyboard roving between Web dots', () => {
  // Screen coordinates: y grows downward.
  const dots = new Map<string, Point>([
    ['centre', { x: 0, y: 0 }],
    ['right-far', { x: 100, y: 10 }],
    ['right-steep', { x: 30, y: 50 }],
    ['up', { x: 5, y: -40 }],
    ['left', { x: -60, y: 0 }],
    ['down', { x: -10, y: 80 }],
  ])

  it('moves to the nearest dot within 45 degrees of the arrow, before any nearer dot further off it', () => {
    expect(webNearestInDirection('centre', 'right', dots)).toBe('right-far')
    expect(webNearestInDirection('centre', 'up', dots)).toBe('up')
    expect(webNearestInDirection('centre', 'left', dots)).toBe('left')
    // Down: right-steep (58 away, 31 degrees off the arrow) is nearer than down (81 away).
    expect(webNearestInDirection('centre', 'down', dots)).toBe('right-steep')
  })

  it('falls back to the nearest dot on that side when none lies within 45 degrees, and stays put when none lies on it', () => {
    const sparse = new Map<string, Point>([['a', { x: 0, y: 0 }], ['b', { x: 20, y: -60 }]])
    expect(webNearestInDirection('a', 'right', sparse)).toBe('b')
    expect(webNearestInDirection('a', 'left', sparse)).toBeNull()
    expect(webNearestInDirection('a', 'down', sparse)).toBeNull()
    expect(webNearestInDirection('missing', 'up', sparse)).toBeNull()
  })

  it('skips dots that are not shown and breaks exact ties by skill ID', () => {
    expect(webNearestInDirection('centre', 'right', dots, new Set(['centre', 'right-steep', 'left']))).toBe('right-steep')
    const tied = new Map<string, Point>([['o', { x: 0, y: 0 }], ['q', { x: 10, y: 10 }], ['p', { x: 10, y: -10 }]])
    expect(webNearestInDirection('o', 'right', tied)).toBe('p')
  })

  it('finds the shown dot nearest a point, where roving starts with nothing selected', () => {
    expect(webNearestTo({ x: 0, y: 0 }, dots)).toBe('centre')
    expect(webNearestTo({ x: 0, y: 0 }, dots, new Set(['left', 'up']))).toBe('up')
    expect(webNearestTo({ x: 0, y: 0 }, dots, new Set())).toBeNull()
  })
})

describe('Sidebar subset of the Web graph', () => {
  // a → b → c → d is a chain of prerequisites (b builds on a); e builds on nothing and leads nowhere; f builds on a.
  const chain: WebGraph = {
    nodes: ['a', 'b', 'c', 'd', 'e', 'f'].map(id => ({ id, slot: 0 })),
    prerequisites: [{ skill: 'b', prerequisite: 'a' }, { skill: 'c', prerequisite: 'b' }, { skill: 'd', prerequisite: 'c' }, { skill: 'f', prerequisite: 'a' }],
    relations: [['d', 'e']],
  }
  const sorted = (ids: ReadonlySet<string>) => [...ids].sort()

  it('shows the whole graph up to the node limit', () => {
    const whole = webSidebarSubset(chain, new Set(['b']), 6)
    expect(sorted(whole.ids)).toEqual(['a', 'b', 'c', 'd', 'e', 'f'])
    expect(whole.partial).toBe(false)
  })

  it('above the limit shows earned skills and every skill one prerequisite step from them, either way', () => {
    const part = webSidebarSubset(chain, new Set(['b']), 5)
    // Relations are not steps: e stays out although d relates to it.
    expect(sorted(part.ids)).toEqual(['a', 'b', 'c'])
    expect(part.partial).toBe(true)
  })

  it('above the limit with nothing earned shows the skills that build on nothing', () => {
    for (const earned of [new Set<string>(), new Set(['not-in-the-graph'])]) {
      const part = webSidebarSubset(chain, earned, 5)
      expect(sorted(part.ids)).toEqual(['a', 'e'])
      expect(part.partial).toBe(true)
    }
  })

  it('shows the bundled taxonomy in full and only part of the scale taxonomy at the default limit', () => {
    expect(webSidebarSubset(BUNDLED, new Set()).partial).toBe(false)
    const part = webSidebarSubset(SCALE, new Set(['skill_big_o']))
    expect(part.partial).toBe(true)
    expect(part.ids.size).toBeLessThan(WEB_SIDEBAR_NODE_LIMIT)
    expect(part.ids.has('skill_big_o')).toBe(true)
  })
})

describe('Placing Web names', () => {
  const label = (id: string, x: number, y: number, width = 80, optional?: boolean) => ({ id, x, y, radius: 5, width, height: 16, optional })
  type Box = { left: number; top: number; right: number; bottom: number }
  /** Where each placed name lands, from its dot, place and offset, as the dot skin draws it. */
  function boxes(labels: ReturnType<typeof label>[], places: ReadonlyMap<string, WebNamePlacement>): Box[] {
    return labels.flatMap(item => {
      const placement = places.get(item.id)
      if (!placement) return []
      const { place, offset } = placement
      const side = item.radius + 4 + offset, under = item.radius + 3 + offset
      const left = place === 'right' ? item.x + side : place === 'left' ? item.x - side - item.width : item.x - item.width / 2
      const top = place === 'below' ? item.y + under : place === 'above' ? item.y - under - item.height : item.y - item.height / 2
      return [{ left, top, right: left + item.width, bottom: top + item.height }]
    })
  }
  const overlapping = (all: Box[]) => all.some((a, i) => all.slice(i + 1).some(b => a.left < b.right && b.left < a.right && a.top < b.bottom && b.top < a.bottom))

  it('puts a name under its dot when there is room', () => {
    expect([...webNamePlaces([label('a', 0, 0), label('b', 0, 200)])]).toEqual([['a', { place: 'below', offset: 0 }], ['b', { place: 'below', offset: 0 }]])
  })

  it('moves a later name above, beside or to the other side of its dot so names in a cluster do not overlap', () => {
    // Three dots in a close row and one under the middle: names under the row would collide.
    const cluster = [label('first', 0, 0), label('second', 30, 0), label('third', 60, 0), label('fourth', 30, 30)]
    const places = webNamePlaces(cluster)
    expect(places.get('first')).toEqual({ place: 'below', offset: 0 })
    expect(overlapping(boxes(cluster, places))).toBe(false)
  })

  it('keeps names off other dots where there is room', () => {
    // A dot sits right under the first one: its name goes above instead.
    const places = webNamePlaces([label('first', 0, 0)], { obstacles: [{ x: 0, y: 16, radius: 5 }] })
    expect(places.get('first')?.place).toBe('above')
  })

  it('moves a name further out from its dot until it is free when no side is, so no two names ever overlap', () => {
    // A selected skill and its neighbours packed tighter than their names: the
    // selected name goes first and keeps its place; every other name is shown
    // and clear of all the rest, the selected one included.
    const crowd = [label('selected', 0, 0, 96), ...Array.from({ length: 11 }, (_, index) => label(`n${index}`, (index % 4) * 9 - 12, Math.floor(index / 4) * 9 - 9, 110))]
    const places = webNamePlaces(crowd, { obstacles: crowd.map(({ x, y, radius }) => ({ x, y, radius })) })
    expect(places.size).toBe(12)
    expect(places.get('selected')).toEqual({ place: 'below', offset: 0 })
    expect([...places.values()].some(placement => placement.offset > 0)).toBe(true)
    expect(overlapping(boxes(crowd, places))).toBe(false)
  })

  it('never covers a firm dot, such as the selected one, moving the name out instead', () => {
    // A big selected dot right beside a small one: every side of the small dot's name at the usual gap reaches it.
    const small = label('small', 0, 0, 120)
    const places = webNamePlaces([small], { obstacles: [{ x: 0, y: 0, radius: 5 }, { x: 14, y: 0, radius: 40, firm: true }] })
    const [box] = boxes([small], places)
    const reaches = (b: Box) => b.left < 54 && b.right > -26 && b.top < 40 && b.bottom > -40
    expect(reaches(box!)).toBe(false)
    expect(places.get('small')!.offset).toBeGreaterThan(0)
  })

  it('in compact placement leaves out an optional name with no free side, and still moves out a required one', () => {
    const crowd = [label('hovered', 0, 0), label('kept', 200, 0, 80, true), ...Array.from({ length: 6 }, (_, index) => label(`n${index}`, index * 6, 4, 80, true))]
    const places = webNamePlaces(crowd, { compact: true })
    expect(places.get('hovered')).toEqual({ place: 'below', offset: 0 })
    expect(places.get('kept')).toEqual({ place: 'below', offset: 0 })
    // Some optional neighbours had nowhere to go at the normal gap, so they are left out, never offset.
    expect(places.size).toBeLessThan(crowd.length)
    expect([...places.values()].every(placement => placement.offset === 0)).toBe(true)
    expect(overlapping(boxes(crowd, places))).toBe(false)
    // Outside compact placement every name is kept.
    expect(webNamePlaces(crowd).size).toBe(crowd.length)
  })
})

describe('Narrowing the Web graph around a skill', () => {
  // one → two → three → four in a chain, one is related to five, and five to six.
  const chain: WebGraph = {
    nodes: ['one', 'two', 'three', 'four', 'five', 'six', 'seven'].map(id => ({ id, slot: 0 })),
    prerequisites: [{ skill: 'two', prerequisite: 'one' }, { skill: 'three', prerequisite: 'two' }, { skill: 'four', prerequisite: 'three' }],
    relations: [['one', 'five'], ['five', 'six']],
  }
  const sorted = (ids: ReadonlySet<string>) => [...ids].sort()

  it('takes one to three steps along prerequisites and dependents, and along relations only when they are shown', () => {
    expect(sorted(webNarrowedIds(chain, 'two', 1, true))).toEqual(['one', 'three', 'two'])
    expect(sorted(webNarrowedIds(chain, 'two', 2, true))).toEqual(['five', 'four', 'one', 'three', 'two'])
    expect(sorted(webNarrowedIds(chain, 'two', 3, true))).toEqual(['five', 'four', 'one', 'six', 'three', 'two'])
    expect(sorted(webNarrowedIds(chain, 'two', 3, false))).toEqual(['four', 'one', 'three', 'two'])
    expect(sorted(webNarrowedIds(chain, 'seven', 3, true))).toEqual(['seven'])
  })

  it('narrows to nothing around a skill the graph does not hold', () => {
    expect(webNarrowedIds(chain, 'missing', 2, true).size).toBe(0)
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
