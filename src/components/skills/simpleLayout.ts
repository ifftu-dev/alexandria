import type { AtlasModel, AtlasNode } from '@/utils/skillAtlas'
import { longestPathLayers } from '@/utils/skillLayers'
import { NARROW_MAP_WIDTH } from './atlasLayout'

// Coordinates depend only on taxonomy membership, IDs and field slots, never
// credentials or view state. All values are pixels; there is no camera.
/** Diameter of a Simple-view disc. */
export const SIMPLE_DISC_SIZE = 46
/** Wide viewports spread each layer's nodes down this height. */
export const SIMPLE_WIDE_HEIGHT = 540
/** Narrow viewports step down one layer per row pitch. */
export const SIMPLE_ROW_PITCH = 156
/** Layers with more nodes than this stagger alternate nodes on narrow viewports. */
const SIMPLE_STAGGER_ABOVE = 3
/** How far a staggered node drops below its row. */
const SIMPLE_STAGGER = 34
/**
 * Below this pitch (the spacing between neighbours along a layer) a layer is
 * dense: names of plain Ready skills show only on hover or focus.
 */
export const SIMPLE_LABEL_MIN_PITCH = 72
/** Room below the lowest disc for its name. */
const SIMPLE_LABEL_BAND = 44
/** Narrow viewports: the first row's centre, and the side margin nodes spread within. */
const SIMPLE_NARROW_TOP = 56
const SIMPLE_NARROW_INSET = 12

export interface SimpleLayout {
  /** Disc centres by skill ID. */
  positions: Map<string, { x: number; y: number }>
  height: number
  /** Skill IDs per layer, in reading order. */
  layers: string[][]
}

/** Spacing between neighbouring nodes along a layer of `nodeCount` nodes. */
function layerPitch(nodeCount: number, viewportWidth: number): number {
  const extent = viewportWidth < NARROW_MAP_WIDTH ? viewportWidth - SIMPLE_NARROW_INSET * 2 : SIMPLE_WIDE_HEIGHT
  return extent / Math.max(1, nodeCount)
}

/** A layer is dense when its nodes sit too close for every name to be legible. */
export function isDenseLayer(nodeCount: number, viewportWidth: number): boolean {
  return nodeCount > 1 && layerPitch(nodeCount, viewportWidth) < SIMPLE_LABEL_MIN_PITCH
}

function compareInLayer(model: AtlasModel, left: AtlasNode, right: AtlasNode): number {
  const leftSlot = model.fieldById.get(left.fieldId)?.slot ?? null
  const rightSlot = model.fieldById.get(right.fieldId)?.slot ?? null
  if (leftSlot !== rightSlot) return leftSlot === null ? 1 : rightSlot === null ? -1 : leftSlot - rightSlot
  const byName = left.skill.name.localeCompare(right.skill.name)
  if (byName !== 0) return byName
  return left.skill.id < right.skill.id ? -1 : left.skill.id > right.skill.id ? 1 : 0
}

/**
 * The Simple view's layered flow. Wide viewports read left to right, one
 * column per layer; below the narrow-map width layers read top to bottom.
 * Layers are counted along the longest prerequisite path, and empty layers
 * left by a cut cycle take no space.
 */
export function simpleLayout(model: AtlasModel, viewportWidth: number): SimpleLayout {
  const layerOf = longestPathLayers(model.nodes.map(node => node.skill.id), model.edges)
  const byLayer = new Map<number, AtlasNode[]>()
  for (const node of model.nodes) {
    const layer = layerOf.get(node.skill.id) ?? 0
    const list = byLayer.get(layer)
    if (list) list.push(node)
    else byLayer.set(layer, [node])
  }
  const layers = [...byLayer.keys()].sort((a, b) => a - b)
    .map(layer => (byLayer.get(layer) ?? []).sort((a, b) => compareInLayer(model, a, b)).map(node => node.skill.id))

  const positions = new Map<string, { x: number; y: number }>()
  if (viewportWidth < NARROW_MAP_WIDTH) {
    layers.forEach((ids, row) => {
      const pitch = layerPitch(ids.length, viewportWidth)
      const staggered = ids.length > SIMPLE_STAGGER_ABOVE
      ids.forEach((id, index) => positions.set(id, {
        x: SIMPLE_NARROW_INSET + pitch * (index + 0.5),
        y: SIMPLE_NARROW_TOP + row * SIMPLE_ROW_PITCH + (staggered && index % 2 === 1 ? SIMPLE_STAGGER : 0),
      }))
    })
    const lastRow = SIMPLE_NARROW_TOP + Math.max(0, layers.length - 1) * SIMPLE_ROW_PITCH
    return { positions, height: lastRow + SIMPLE_STAGGER + SIMPLE_DISC_SIZE / 2 + SIMPLE_LABEL_BAND, layers }
  }
  const columnWidth = viewportWidth / Math.max(1, layers.length)
  layers.forEach((ids, column) => {
    const pitch = layerPitch(ids.length, viewportWidth)
    ids.forEach((id, index) => positions.set(id, { x: columnWidth * (column + 0.5), y: pitch * (index + 0.5) }))
  })
  return { positions, height: SIMPLE_WIDE_HEIGHT + SIMPLE_LABEL_BAND, layers }
}
