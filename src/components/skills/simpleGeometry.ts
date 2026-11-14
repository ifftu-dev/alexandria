import type { AtlasModel, AtlasNode } from '@/utils/skillAtlas'
import { isDenseLayer, SIMPLE_DISC_SIZE, SIMPLE_LABEL_BAND, SIMPLE_LABEL_MAX_WIDTH, SIMPLE_RING_ROOM, type SimpleLayout } from './simpleLayout'

// What the Simple view draws from its layout: node boxes, edges with their
// heads, and which nodes the selection emphasises, dims or quiets.

type Edge = AtlasModel['edges'][number]

/** Where a node's element sits on the canvas. */
export interface NodeBox { x: number; y: number; width: number; height: number }

/** A node's box: its disc with room above for the ready-next halo, and its name below. */
export const SIMPLE_NODE_HEIGHT = SIMPLE_RING_ROOM + SIMPLE_DISC_SIZE + SIMPLE_LABEL_BAND
/** Space between an edge's end and the rim of the disc it points at, so the head stays clear of the disc. */
export const SIMPLE_ARROW_GAP = 4
/** Length of an arrowhead along its edge, plain and highlighted (both views). */
export const EDGE_HEAD_LENGTH = 10
export const ACTIVE_EDGE_HEAD_LENGTH = 14

/** Each node's box, as wide as the layout lets its name grow. */
export function simpleNodeBoxes(layout: SimpleLayout): Map<string, NodeBox> {
  const boxes = new Map<string, NodeBox>()
  for (const [skillId, point] of layout.positions) {
    const width = layout.labelWidths.get(skillId) ?? SIMPLE_LABEL_MAX_WIDTH
    boxes.set(skillId, { x: point.x - width / 2, y: point.y - SIMPLE_DISC_SIZE / 2 - SIMPLE_RING_ROOM, width, height: SIMPLE_NODE_HEIGHT })
  }
  return boxes
}

/** An edge's curve and where and which way it ends, for its arrowhead. */
export interface EdgeShape { edge: Edge; path: string; x2: number; y2: number; tx: number; ty: number }

const round = (value: number) => Math.round(value * 10) / 10

/**
 * Edges run disc rim to disc rim, leaving along the reading direction
 * (rightward in columns, downward in rows). Edges with an unplaced end are
 * left out; unloaded endpoints remain explicit in the inspector.
 */
export function simpleEdgeShapes(edges: readonly Edge[], layout: SimpleLayout): EdgeShape[] {
  const { positions } = layout, rows = layout.flow === 'rows', radius = SIMPLE_DISC_SIZE / 2
  return edges.flatMap(edge => {
    const from = positions.get(edge.prerequisite_id), to = positions.get(edge.skill_id)
    if (!from || !to) return []
    const length = Math.hypot(to.x - from.x, to.y - from.y)
    if (length === 0) return []
    const ux = (to.x - from.x) / length, uy = (to.y - from.y) / length
    const x1 = round(from.x + ux * radius), y1 = round(from.y + uy * radius)
    const x2 = round(to.x - ux * (radius + SIMPLE_ARROW_GAP)), y2 = round(to.y - uy * (radius + SIMPLE_ARROW_GAP))
    const c1x = rows ? x1 : round((x1 + x2) / 2), c1y = rows ? round((y1 + y2) / 2) : y1
    const c2x = rows ? x2 : c1x, c2y = rows ? c1y : y2
    const end = Math.hypot(x2 - c2x, y2 - c2y)
    const tx = end ? (x2 - c2x) / end : ux, ty = end ? (y2 - c2y) / end : uy
    return [{ edge, path: `M ${x1} ${y1} C ${c1x} ${c1y}, ${c2x} ${c2y}, ${x2} ${y2}`, x2, y2, tx, ty }]
  })
}

/** An arrowhead with its tip at the end of an edge, pointing along its end direction. */
export function simpleArrowHead(shape: Pick<EdgeShape, 'x2' | 'y2' | 'tx' | 'ty'>, length: number): string {
  const { x2, y2, tx, ty } = shape
  const bx = x2 - tx * length, by = y2 - ty * length, half = length / 2
  return `M ${x2} ${y2} L ${round(bx - ty * half)} ${round(by + tx * half)} L ${round(bx + ty * half)} ${round(by - tx * half)} z`
}

/** One drawn set of edges: their curves and their heads, each one path. */
export interface EdgePathSet { path: string; heads: string }
export interface EdgePaths { plain: EdgePathSet; active: EdgePathSet; faded: EdgePathSet }
export type EdgeEmphasis = keyof EdgePaths

/** Sorts edges into plain, active and faded sets, each drawn as two paths. */
export function edgePaths(shapes: readonly EdgeShape[], emphasisOf: (edge: Edge) => EdgeEmphasis,
  head: (shape: EdgeShape, length: number) => string): EdgePaths {
  const parts: Record<EdgeEmphasis, { paths: string[]; heads: string[] }> = {
    plain: { paths: [], heads: [] }, active: { paths: [], heads: [] }, faded: { paths: [], heads: [] },
  }
  for (const shape of shapes) {
    const emphasis = emphasisOf(shape.edge), set = parts[emphasis]
    set.paths.push(shape.path)
    set.heads.push(head(shape, emphasis === 'active' ? ACTIVE_EDGE_HEAD_LENGTH : EDGE_HEAD_LENGTH))
  }
  const joined = (set: { paths: string[]; heads: string[] }): EdgePathSet => ({ path: set.paths.join(' '), heads: set.heads.join(' ') })
  return { plain: joined(parts.plain), active: joined(parts.active), faded: joined(parts.faded) }
}

/**
 * The emphasised nodes: an inspector focus when one is active, otherwise the
 * selection and its loaded one-hop prerequisites and dependents.
 */
export function simpleEmphasis(selected: AtlasNode | undefined, focus: ReadonlySet<string>,
  nodesById: ReadonlyMap<string, AtlasNode>): ReadonlySet<string> {
  if (focus.size > 0) return focus
  const ids = new Set<string>()
  if (!selected) return ids
  ids.add(selected.skill.id)
  for (const relation of [...selected.prerequisites, ...selected.dependents]) if (nodesById.has(relation.skillId)) ids.add(relation.skillId)
  return ids
}

/** Nodes outside an active emphasis dim. */
export function simpleDimmed(emphasis: ReadonlySet<string>, skillId: string): boolean {
  return emphasis.size > 0 && !emphasis.has(skillId)
}

/** An edge leaving the emphasised neighbourhood fades. */
export function simpleEdgeFaded(emphasis: ReadonlySet<string>, edge: Edge): boolean {
  return emphasis.size > 0 && !(emphasis.has(edge.prerequisite_id) && emphasis.has(edge.skill_id))
}

/** Nodes in layers too dense for every name. */
export function simpleDenseIds(layout: SimpleLayout, viewportWidth: number): ReadonlySet<string> {
  return new Set(layout.layers.filter(ids => isDenseLayer(ids.length, viewportWidth, layout.flow)).flat())
}

/** A plain Ready name in a dense layer waits for hover or focus unless the node is emphasised. */
export function simpleLabelQuiet(node: AtlasNode, dense: ReadonlySet<string>, emphasis: ReadonlySet<string>): boolean {
  return dense.has(node.skill.id) && node.status === 'ready' && !node.readyNext && !emphasis.has(node.skill.id)
}
