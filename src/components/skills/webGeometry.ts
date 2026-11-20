import type { AtlasModel, BloomRank } from '@/utils/skillAtlas'

/** What the Web layout reads from the atlas model: plain data, no credentials. */
export interface WebGraph {
  /** Every skill with its field's atlas slot; null for unassigned skills. */
  nodes: readonly { id: string; slot: number | null }[]
  /** `skill` builds on `prerequisite`. Links to skills not in `nodes` are ignored. */
  prerequisites: readonly { skill: string; prerequisite: string }[]
  /** Non-prerequisite relations, undirected; may be empty. */
  relations: readonly (readonly [string, string])[]
}

export type WebPoint = { x: number; y: number }

/**
 * The Web graph of an atlas model: every skill with its field's slot, and the
 * prerequisites and relations between loaded skills. A relation sits on both
 * of its skills in the model and appears here once, lower ID first. The
 * layout always uses relations, whether or not they are drawn.
 */
export function webGraphFromAtlas(model: AtlasModel): WebGraph {
  const relations: (readonly [string, string])[] = []
  for (const node of model.nodes) {
    for (const relation of node.relations) {
      if (relation.loaded && model.nodeById.has(relation.skillId) && node.skill.id < relation.skillId) relations.push([node.skill.id, relation.skillId])
    }
  }
  return {
    nodes: model.nodes.map(node => ({ id: node.skill.id, slot: model.fieldById.get(node.fieldId)?.slot ?? null })),
    prerequisites: model.edges
      .filter(edge => model.nodeById.has(edge.skill_id) && model.nodeById.has(edge.prerequisite_id))
      .map(edge => ({ skill: edge.skill_id, prerequisite: edge.prerequisite_id })),
    relations,
  }
}

// Coordinates are layout units centred on the origin; the camera scales them.
// They depend only on the graph: never on credentials, sizes or view state.
/** Seed of the starting scatter. */
export const WEB_SEED = 7
/** Ticks run by `webLayout`; its strength cools linearly from 1 to `WEB_MIN_ALPHA`. */
export const WEB_TICKS = 120
export const WEB_MIN_ALPHA = 0.05
/** Each field starts as a disc holding one skill per square of this side. */
export const WEB_START_SPACING = 40
/** Repulsion between two skills falls with the square of their distance and stops at the cut-off. */
export const WEB_REPULSION = 520
export const WEB_REPULSION_CUTOFF = 50
/**
 * Rest length and stiffness of a prerequisite spring. A spring pulls no
 * harder than it would stretched by `WEB_MAX_STRETCH`, so a link across the
 * graph cannot drag a skill out of its field.
 */
export const WEB_PREREQUISITE_LENGTH = 40
export const WEB_PREREQUISITE_STRENGTH = 0.14
export const WEB_MAX_STRETCH = 40
/** Relations pull more weakly toward a longer rest length. */
export const WEB_RELATION_LENGTH = 64
export const WEB_RELATION_STRENGTH = 0.04
/** Pull toward the centroid of a skill's field, and toward the origin. */
export const WEB_FIELD_PULL = 0.035
export const WEB_CENTRE_PULL = 0.012
/**
 * Every skill adds to the squeeze a pull puts on the middle, so above these
 * sizes each skill pulls in proportion less: a field past
 * `WEB_FIELD_REFERENCE` skills, and the centre past `WEB_CENTRE_REFERENCE`.
 * Without it a large taxonomy packs tighter than a small one.
 */
export const WEB_FIELD_REFERENCE = 20
export const WEB_CENTRE_REFERENCE = 100
/** Share of its velocity a skill keeps from one tick to the next, and the furthest it moves in one tick. */
export const WEB_VELOCITY_KEEP = 0.8
export const WEB_MAX_STEP = 20
/** No two dots end closer than this, centre to centre. */
export const WEB_MIN_DISTANCE = 24
/**
 * Skills closer than the collision distance are pushed apart by this share
 * of the overlap every tick, however cool the run. The distance sits a
 * little past the minimum, so the run itself ends clear of it.
 */
export const WEB_COLLISION_DISTANCE = 28
export const WEB_COLLISION_STRENGTH = 0.5
/** Passes spent at the end pushing apart dots still closer than the minimum distance. */
const WEB_SEPARATION_PASSES = 60

function compareIds(left: string, right: string): number {
  return left < right ? -1 : left > right ? 1 : 0
}

/** The graph in index form, in ID order, with links deduplicated and sorted so input order never matters. */
interface Prepared {
  ids: string[]
  group: Int32Array
  groupCount: number
  /** Flat [a, b, a, b, …] index pairs. */
  prerequisites: Int32Array
  relations: Int32Array
  /** Per link, a share of the link kind's strength. */
  prerequisiteStiffness: Float64Array
  relationStiffness: Float64Array
}

function prepare(graph: WebGraph): Prepared {
  const slotOf = new Map<string, number | null>()
  for (const node of graph.nodes) if (!slotOf.has(node.id)) slotOf.set(node.id, node.slot)
  const ids = [...slotOf.keys()].sort(compareIds)
  const index = new Map(ids.map((id, i) => [id, i]))
  const slots = [...new Set(ids.map(id => slotOf.get(id) ?? null))]
    .sort((a, b) => a === null ? 1 : b === null ? -1 : a - b)
  const group = Int32Array.from(ids, id => slots.indexOf(slotOf.get(id) ?? null))
  const pairs = (list: readonly (readonly [string, string])[]) => {
    const seen = new Set<number>()
    for (const [a, b] of list) {
      const i = index.get(a), j = index.get(b)
      if (i === undefined || j === undefined || i === j) continue
      seen.add(Math.min(i, j) * ids.length + Math.max(i, j))
    }
    const flat = new Int32Array(seen.size * 2)
    ;[...seen].sort((a, b) => a - b).forEach((key, k) => {
      flat[k * 2] = Math.floor(key / ids.length)
      flat[k * 2 + 1] = key % ids.length
    })
    return flat
  }
  const prerequisites = pairs(graph.prerequisites.map(edge => [edge.skill, edge.prerequisite] as const))
  const relations = pairs(graph.relations)
  const degree = new Float64Array(ids.length)
  for (const list of [prerequisites, relations]) for (const i of list) degree[i]!++
  // A link between two well-linked skills is softer, so a densely linked
  // patch is not crushed together: stiffness is one over the smaller degree.
  const stiffness = (list: Int32Array) => Float64Array.from({ length: list.length / 2 },
    (_, k) => 1 / Math.min(degree[list[k * 2]!]!, degree[list[k * 2 + 1]!]!))
  return {
    ids, group, groupCount: slots.length,
    prerequisites, relations,
    prerequisiteStiffness: stiffness(prerequisites), relationStiffness: stiffness(relations),
  }
}

/**
 * Finds every pair of skills closer than a reach by sorting them into square
 * cells at least that wide: such a pair shares a cell or sits in touching
 * ones, so each skill only meets its near neighbours and cost grows close to
 * linearly. Pairs come out in a fixed order for fixed positions.
 */
class NearPairs {
  /** Flat [i, j, i, j, …] pairs found by the last `find`; `count` pairs are valid. */
  list = new Int32Array(1024)
  count = 0
  private readonly cellOf: Int32Array
  private readonly order: Int32Array
  private readonly sortedX: Float64Array
  private readonly sortedY: Float64Array
  private start = new Int32Array(0)

  constructor(size: number) {
    this.cellOf = new Int32Array(size)
    this.order = new Int32Array(size)
    this.sortedX = new Float64Array(size)
    this.sortedY = new Float64Array(size)
  }

  find(x: Float64Array, y: Float64Array, reach: number): void {
    const n = x.length
    this.count = 0
    if (n < 2) return
    let minX = Infinity, maxX = -Infinity, minY = Infinity, maxY = -Infinity
    for (let i = 0; i < n; i++) {
      const xi = x[i]!, yi = y[i]!
      if (xi < minX) minX = xi
      if (xi > maxX) maxX = xi
      if (yi < minY) minY = yi
      if (yi > maxY) maxY = yi
    }
    // A spread-out layout gets wider cells, so the grid never outgrows a few cells per skill.
    const cell = Math.max(reach, Math.sqrt((maxX - minX + 1) * (maxY - minY + 1) / (4 * n)))
    const columns = Math.floor((maxX - minX) / cell) + 1
    const rows = Math.floor((maxY - minY) / cell) + 1
    const cells = columns * rows
    if (this.start.length < cells + 1) this.start = new Int32Array(cells + 1)
    const start = this.start, cellOf = this.cellOf, order = this.order
    start.fill(0, 0, cells + 1)
    for (let i = 0; i < n; i++) {
      const c = Math.floor((y[i]! - minY) / cell) * columns + Math.floor((x[i]! - minX) / cell)
      cellOf[i] = c
      start[c + 1]!++
    }
    for (let c = 0; c < cells; c++) start[c + 1]! += start[c]!
    const next = start.slice(0, cells)
    // Positions copied in cell order, so neighbours are read side by side.
    const sx = this.sortedX, sy = this.sortedY
    for (let i = 0; i < n; i++) {
      const slot = next[cellOf[i]!]!++
      order[slot] = i
      sx[slot] = x[i]!
      sy[slot] = y[i]!
    }
    const reach2 = reach * reach
    let list = this.list, count = 0
    for (let row = 0; row < rows; row++) {
      for (let column = 0; column < columns; column++) {
        const c = row * columns + column
        const from = start[c]!, to = start[c + 1]!
        if (from === to) continue
        // The cell itself, then each touching cell once: right, and the three below.
        for (let side = 0; side < 5; side++) {
          let otherFrom = from, otherTo = to
          if (side > 0) {
            const nc = column + FORWARD[side * 2 - 2]!, nr = row + FORWARD[side * 2 - 1]!
            if (nc < 0 || nc >= columns || nr >= rows) continue
            otherFrom = start[nr * columns + nc]!
            otherTo = start[nr * columns + nc + 1]!
          }
          for (let a = from; a < to; a++) {
            const xi = sx[a]!, yi = sy[a]!
            for (let b = side === 0 ? a + 1 : otherFrom; b < otherTo; b++) {
              const dx = sx[b]! - xi, dy = sy[b]! - yi
              if (dx * dx + dy * dy >= reach2) continue
              if (count * 2 + 2 > list.length) {
                const grown = new Int32Array(list.length * 2)
                grown.set(list)
                list = grown
              }
              list[count * 2] = order[a]!
              list[count * 2 + 1] = order[b]!
              count++
            }
          }
        }
      }
    }
    this.list = list
    this.count = count
  }
}
/** Column and row steps to the touching cells after a cell in reading order. */
const FORWARD = Int32Array.of(1, 0, -1, 1, 0, 1, 1, 1)

interface Simulation {
  graph: Prepared
  x: Float64Array
  y: Float64Array
  vx: Float64Array
  vy: Float64Array
  pinned: Uint8Array
  near: NearPairs
  /** Each skill's pull toward its field's centroid, and toward the origin. */
  fieldPull: Float64Array
  centrePull: number
}

function simulation(graph: Prepared): Simulation {
  const n = graph.ids.length
  const sizes = new Float64Array(graph.groupCount)
  for (let i = 0; i < n; i++) sizes[graph.group[i]!]!++
  return { graph, x: new Float64Array(n), y: new Float64Array(n), vx: new Float64Array(n), vy: new Float64Array(n),
    pinned: new Uint8Array(n), near: new NearPairs(n),
    fieldPull: Float64Array.from(graph.group, g => WEB_FIELD_PULL * Math.min(1, WEB_FIELD_REFERENCE / sizes[g]!)),
    centrePull: WEB_CENTRE_PULL * Math.min(1, WEB_CENTRE_REFERENCE / Math.max(1, n)) }
}

/** Coincident skills part along a direction fixed by their indices. */
function parting(i: number, j: number, axis: 0 | 1): number {
  const angle = ((i * 7 + j * 13) % 360) * Math.PI / 180
  return (axis === 0 ? Math.cos(angle) : Math.sin(angle)) * 0.01
}

/** Repulsion between near skills, plus a push out of any overlap. */
function repel(sim: Simulation, alpha: number): void {
  const { x, y, vx, vy, near } = sim
  near.find(x, y, WEB_REPULSION_CUTOFF)
  const list = near.list, strength = WEB_REPULSION * alpha
  for (let k = 0; k < near.count; k++) {
    const i = list[k * 2]!, j = list[k * 2 + 1]!
    let dx = x[j]! - x[i]!, dy = y[j]! - y[i]!
    if (dx === 0 && dy === 0) { dx = parting(i, j, 0); dy = parting(i, j, 1) }
    const d2 = dx * dx + dy * dy, d = Math.sqrt(d2)
    let scale = strength / (d2 * d)
    if (d < WEB_COLLISION_DISTANCE) scale += (WEB_COLLISION_DISTANCE - d) / d * WEB_COLLISION_STRENGTH
    vx[i]! -= dx * scale; vy[i]! -= dy * scale
    vx[j]! += dx * scale; vy[j]! += dy * scale
  }
}

function pull(sim: Simulation, pairs: Int32Array, stiffness: Float64Array, length: number, strength: number, alpha: number): void {
  const { x, y, vx, vy } = sim
  for (let k = 0; k < stiffness.length; k++) {
    const i = pairs[k * 2]!, j = pairs[k * 2 + 1]!
    const dx = x[j]! - x[i]!, dy = y[j]! - y[i]!
    const d = Math.hypot(dx, dy) || 1
    const scale = Math.min(d - length, WEB_MAX_STRETCH) / d * strength * stiffness[k]! * alpha
    vx[i]! += dx * scale; vy[i]! += dy * scale
    vx[j]! -= dx * scale; vy[j]! -= dy * scale
  }
}

function tick(sim: Simulation, alpha: number): void {
  const { graph, x, y, vx, vy, pinned, fieldPull, centrePull } = sim
  repel(sim, alpha)
  pull(sim, graph.prerequisites, graph.prerequisiteStiffness, WEB_PREREQUISITE_LENGTH, WEB_PREREQUISITE_STRENGTH, alpha)
  pull(sim, graph.relations, graph.relationStiffness, WEB_RELATION_LENGTH, WEB_RELATION_STRENGTH, alpha)
  const sumX = new Float64Array(graph.groupCount), sumY = new Float64Array(graph.groupCount), count = new Float64Array(graph.groupCount)
  for (let i = 0; i < x.length; i++) {
    const g = graph.group[i]!
    sumX[g]! += x[i]!; sumY[g]! += y[i]!; count[g]!++
  }
  for (let i = 0; i < x.length; i++) {
    if (pinned[i]) { vx[i] = 0; vy[i] = 0; continue }
    const g = graph.group[i]!
    vx[i]! += ((sumX[g]! / count[g]! - x[i]!) * fieldPull[i]! - x[i]! * centrePull) * alpha
    vy[i]! += ((sumY[g]! / count[g]! - y[i]!) * fieldPull[i]! - y[i]! * centrePull) * alpha
    vx[i]! *= WEB_VELOCITY_KEEP; vy[i]! *= WEB_VELOCITY_KEEP
    const speed = Math.hypot(vx[i]!, vy[i]!)
    if (speed > WEB_MAX_STEP) { vx[i]! *= WEB_MAX_STEP / speed; vy[i]! *= WEB_MAX_STEP / speed }
    x[i]! += vx[i]!; y[i]! += vy[i]!
  }
}

/** Pushes apart every pair closer than the minimum distance; a pinned skill holds still and its partner moves the whole way. */
function separate(sim: Simulation, passes: number): void {
  const { x, y, pinned, near } = sim
  for (let pass = 0; pass < passes; pass++) {
    near.find(x, y, WEB_MIN_DISTANCE)
    if (near.count === 0) return
    const list = near.list
    for (let k = 0; k < near.count; k++) {
      const i = list[k * 2]!, j = list[k * 2 + 1]!
      if (pinned[i] && pinned[j]) continue
      let dx = x[j]! - x[i]!, dy = y[j]! - y[i]!
      if (dx === 0 && dy === 0) { dx = parting(i, j, 0); dy = parting(i, j, 1) }
      const d = Math.hypot(dx, dy)
      if (d >= WEB_MIN_DISTANCE) continue
      // A hair past the minimum, so rounding never leaves a pair just short of it.
      const push = (WEB_MIN_DISTANCE * 1.001 - d) / d
      const share = pinned[i] ? 0 : pinned[j] ? 1 : 0.5
      x[i]! -= dx * push * share; y[i]! -= dy * push * share
      x[j]! += dx * push * (1 - share); y[j]! += dy * push * (1 - share)
    }
  }
}

/**
 * Starting positions: each field's skills scattered over a disc sized for
 * them at the starting spacing, the discs side by side on a ring in slot
 * order, so a large taxonomy starts close to its resting density.
 */
function seed(sim: Simulation): void {
  const { graph, x, y } = sim
  let state = WEB_SEED
  const random = () => (state = (state * 16807) % 2147483647) / 2147483647
  const sizes = new Float64Array(graph.groupCount)
  for (let i = 0; i < graph.ids.length; i++) sizes[graph.group[i]!]!++
  const discs = Array.from(sizes, size => WEB_START_SPACING * Math.sqrt(size / Math.PI))
  const circumference = discs.reduce((sum, radius) => sum + 2 * radius, 0)
  const ring = graph.groupCount > 1 ? circumference / (2 * Math.PI) : 0
  for (let i = 0; i < graph.ids.length; i++) {
    const g = graph.group[i]!
    const angle = g / graph.groupCount * Math.PI * 2
    const r = discs[g]! * Math.sqrt(random()), turn = random() * Math.PI * 2
    x[i] = Math.cos(angle) * ring + Math.cos(turn) * r
    y[i] = Math.sin(angle) * ring + Math.sin(turn) * r
  }
}

function positionsOf(sim: Simulation): Map<string, WebPoint> {
  return new Map(sim.graph.ids.map((id, i) => [id, { x: sim.x[i]!, y: sim.y[i]! }]))
}

/**
 * The Web view's positions: a force layout run to rest. Skills start on a
 * ring by field slot, repel their near neighbours, are pulled together along
 * prerequisites (and, more loosely, relations), toward their field's
 * centroid and toward the centre. The same graph always yields the same
 * positions, whatever its input order; the result is centred on the origin.
 */
export function webLayout(graph: WebGraph): Map<string, WebPoint> {
  const sim = simulation(prepare(graph))
  const { x, y } = sim
  seed(sim)
  for (let t = 0; t < WEB_TICKS; t++) tick(sim, Math.max(WEB_MIN_ALPHA, 1 - t / WEB_TICKS))
  separate(sim, WEB_SEPARATION_PASSES)
  let minX = Infinity, maxX = -Infinity, minY = Infinity, maxY = -Infinity
  for (let i = 0; i < x.length; i++) {
    minX = Math.min(minX, x[i]!); maxX = Math.max(maxX, x[i]!)
    minY = Math.min(minY, y[i]!); maxY = Math.max(maxY, y[i]!)
  }
  const midX = x.length ? (minX + maxX) / 2 : 0, midY = x.length ? (minY + maxY) / 2 : 0
  for (let i = 0; i < x.length; i++) { x[i]! -= midX; y[i]! -= midY }
  return positionsOf(sim)
}

/** Ticks and strength of one call of `continueWebLayout`: one animation frame of a live drag. */
export const WEB_LIVE_TICKS = 1
export const WEB_LIVE_ALPHA = 0.3

export interface WebContinueOptions {
  /** Skills that hold still where `positions` puts them, such as the dot under the pointer. */
  pinned?: ReadonlySet<string>
  ticks?: number
  /** Strength of every force but the overlap push, as in the layout's cooling run. */
  alpha?: number
}

/**
 * The layout's forces run on from the given positions, for a live drag:
 * pinned skills hold still and the rest follow and settle. A skill with no
 * position starts where the layout would seed it; positions of skills not in
 * the graph are ignored. Nothing is re-centred, so the web stays under the
 * pointer.
 */
export function continueWebLayout(graph: WebGraph, positions: ReadonlyMap<string, WebPoint>,
  options: WebContinueOptions = {}): Map<string, WebPoint> {
  const sim = simulation(prepare(graph))
  const { x, y, pinned } = sim
  const { ticks = WEB_LIVE_TICKS, alpha = WEB_LIVE_ALPHA } = options
  seed(sim)
  sim.graph.ids.forEach((id, i) => {
    const point = positions.get(id)
    if (point) { x[i] = point.x; y[i] = point.y }
    if (options.pinned?.has(id)) pinned[i] = 1
  })
  for (let t = 0; t < ticks; t++) tick(sim, alpha)
  return positionsOf(sim)
}

// Dot size is the Bloom level earned and never feeds the layout, so earning
// a level never moves a dot. Radii are layout units, like positions.
/** Radius of a dot earned at the first Bloom level (Remember) and at the sixth (Create); the four between step evenly in area. */
export const WEB_DOT_FIRST_RADIUS = 5
export const WEB_DOT_SIXTH_RADIUS = 10
/** Radius of a dot not yet earned, or earned at a level that is unavailable, so no level is ever invented. */
export const WEB_DOT_BASE_RADIUS = 4

/** A dot's radius for the Bloom rank earned (0 is Remember, 5 is Create), or the base size for none. */
export function webDotRadius(rank: BloomRank | null | undefined): number {
  if (rank == null) return WEB_DOT_BASE_RADIUS
  const first = WEB_DOT_FIRST_RADIUS ** 2, sixth = WEB_DOT_SIXTH_RADIUS ** 2
  return Math.sqrt(first + (sixth - first) * rank / 5)
}

/** An arrow key, in screen terms: down is toward larger y. */
export type WebDirection = 'up' | 'down' | 'left' | 'right'

const DIRECTIONS: Record<WebDirection, WebPoint> = { up: { x: 0, y: -1 }, down: { x: 0, y: 1 }, left: { x: -1, y: 0 }, right: { x: 1, y: 0 } }

/**
 * The dot the keyboard moves to from `from` for an arrow key: the nearest
 * shown dot within 45 degrees of the arrow, or failing that the nearest on
 * the arrow's side; null when no shown dot lies on that side. Exact ties go
 * to the lower skill ID. With no `shown` set, every positioned dot is shown.
 */
export function webNearestInDirection(from: string, direction: WebDirection, positions: ReadonlyMap<string, WebPoint>,
  shown?: ReadonlySet<string>): string | null {
  const origin = positions.get(from)
  if (!origin) return null
  const arrow = DIRECTIONS[direction]
  let best: { id: string; inCone: boolean; distance: number } | null = null
  for (const [id, point] of positions) {
    if (id === from || (shown && !shown.has(id))) continue
    const dx = point.x - origin.x, dy = point.y - origin.y
    const along = dx * arrow.x + dy * arrow.y
    if (along <= 0) continue
    const inCone = Math.abs(dx * arrow.y - dy * arrow.x) <= along
    const distance = Math.hypot(dx, dy)
    const better = !best || (inCone !== best.inCone ? inCone
      : distance !== best.distance ? distance < best.distance : compareIds(id, best.id) < 0)
    if (better) best = { id, inCone, distance }
  }
  return best?.id ?? null
}

/** The shown dot nearest a point, such as the centre of the view; ties go to the lower skill ID. */
export function webNearestTo(point: WebPoint, positions: ReadonlyMap<string, WebPoint>, shown?: ReadonlySet<string>): string | null {
  let best: { id: string; distance: number } | null = null
  for (const [id, at] of positions) {
    if (shown && !shown.has(id)) continue
    const distance = Math.hypot(at.x - point.x, at.y - point.y)
    if (!best || distance < best.distance || (distance === best.distance && compareIds(id, best.id) < 0)) best = { id, distance }
  }
  return best?.id ?? null
}

/** How far the Web graph can be narrowed around the selection, in steps. */
export type WebNarrowingDepth = 1 | 2 | 3

/**
 * The skills within `depth` steps of `id`: along prerequisites in either
 * direction and, when `related` is on (the dotted lines are shown), along
 * relations. Empty when the graph does not hold `id`.
 */
export function webNarrowedIds(graph: WebGraph, id: string, depth: WebNarrowingDepth, related: boolean): Set<string> {
  const steps = new Map<string, string[]>()
  const link = (a: string, b: string) => { steps.set(a, [...steps.get(a) ?? [], b]); steps.set(b, [...steps.get(b) ?? [], a]) }
  for (const { skill, prerequisite } of graph.prerequisites) link(skill, prerequisite)
  if (related) for (const [a, b] of graph.relations) link(a, b)
  if (!graph.nodes.some(node => node.id === id)) return new Set()
  const ids = new Set([id])
  let frontier = [id]
  for (let step = 0; step < depth && frontier.length; step++) {
    const next: string[] = []
    for (const from of frontier) for (const to of steps.get(from) ?? []) if (!ids.has(to)) { ids.add(to); next.push(to) }
    frontier = next
  }
  return ids
}

/**
 * The most skills the sidebar graph draws whole. Tuned so the bundled
 * taxonomy (about a hundred skills) always shows in full and the 1,500-skill
 * scale fixture does not.
 */
export const WEB_SIDEBAR_NODE_LIMIT = 300

export interface WebSidebarSubset {
  /** The skills the sidebar graph draws. */
  ids: Set<string>
  /** True when that is less than the whole graph, so the sidebar says so. */
  partial: boolean
}

/**
 * Which skills the sidebar graph draws. Up to `limit` skills, all of them.
 * Above it, the earned skills and every skill one prerequisite step from
 * them in either direction; with nothing earned, the skills that build on
 * nothing in the graph. Relations are not steps.
 */
export function webSidebarSubset(graph: Pick<WebGraph, 'nodes' | 'prerequisites'>, earned: ReadonlySet<string>,
  limit: number = WEB_SIDEBAR_NODE_LIMIT): WebSidebarSubset {
  const all = new Set(graph.nodes.map(node => node.id))
  if (all.size <= limit) return { ids: all, partial: false }
  const links = graph.prerequisites.filter(link => all.has(link.skill) && all.has(link.prerequisite) && link.skill !== link.prerequisite)
  const ids = new Set([...earned].filter(id => all.has(id)))
  if (ids.size === 0) {
    const buildsOnSomething = new Set(links.map(link => link.skill))
    return { ids: new Set([...all].filter(id => !buildsOnSomething.has(id))), partial: true }
  }
  for (const link of links) {
    if (earned.has(link.skill)) ids.add(link.prerequisite)
    if (earned.has(link.prerequisite)) ids.add(link.skill)
  }
  return { ids, partial: true }
}
