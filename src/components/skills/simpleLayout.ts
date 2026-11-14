import type { AtlasModel, AtlasNode } from '@/utils/skillAtlas'
import { longestPathLayers } from '@/utils/skillLayers'
import { NARROW_MAP_WIDTH } from './atlasLayout'

// Coordinates depend only on taxonomy membership, IDs and field slots, never
// credentials or view state. All values are pixels; there is no camera.
/** Diameter of a Simple-view disc. */
export const SIMPLE_DISC_SIZE = 46
/** Wide viewports spread each layer's nodes down at least this height. */
export const SIMPLE_WIDE_HEIGHT = 540
/** How far a ready-next disc's dashed halo reaches past the disc. */
const SIMPLE_HALO = 6
/** Room below a disc for its name: up to two lines. */
const SIMPLE_LABEL_BAND = 44
/** The widest a name under a disc grows. */
export const SIMPLE_LABEL_MAX_WIDTH = 120
/** Clear space a name keeps from its neighbours across a layer. */
const SIMPLE_LABEL_GAP = 4
/**
 * Wide layers never pack nodes closer than a disc, its name and the next
 * disc's halo: a layer that would grows the layout past the fixed height.
 */
export const SIMPLE_MIN_DISC_PITCH = SIMPLE_DISC_SIZE + SIMPLE_LABEL_BAND + SIMPLE_HALO
/** Narrow viewports step down one layer per row pitch. */
export const SIMPLE_ROW_PITCH = 156
/** Layers with more nodes than this stagger alternate nodes on narrow viewports. */
const SIMPLE_STAGGER_ABOVE = 3
/** How far a staggered node drops below its row: the height of a two-line name, so neighbouring names never share a line. */
const SIMPLE_STAGGER = 36
/**
 * Below this pitch (the spacing between neighbours along a layer) a layer is
 * dense: names of plain Ready skills show only on hover or focus.
 */
export const SIMPLE_LABEL_MIN_PITCH = 90
/** Narrow viewports: the first row's centre, and the side margin nodes spread within. */
const SIMPLE_NARROW_TOP = 56
const SIMPLE_NARROW_INSET = 12
/**
 * Narrow rows never pack neighbours closer than this, which leaves a
 * staggered name room for a long word between its neighbours' halos: a
 * layer that would wraps onto extra rows instead.
 */
const SIMPLE_MIN_NARROW_PITCH = 80
/** A wrapped row of the same layer sits this far below the row before: clear of its staggered discs and their names. */
const SIMPLE_WRAP_PITCH = SIMPLE_STAGGER + SIMPLE_DISC_SIZE + SIMPLE_LABEL_BAND + SIMPLE_HALO

export interface SimpleLayout {
  /** Disc centres by skill ID. */
  positions: Map<string, { x: number; y: number }>
  height: number
  /** Skill IDs per layer, in reading order. */
  layers: string[][]
  /**
   * How wide each name under a disc may grow: clear of the neighbouring
   * discs and names, and inside the map.
   */
  labelWidths: Map<string, number>
}

/** The length a wide layer of `nodeCount` nodes spreads along: the fixed height, or more when it is crowded. */
function wideExtent(nodeCount: number): number {
  return Math.max(SIMPLE_WIDE_HEIGHT, nodeCount * SIMPLE_MIN_DISC_PITCH)
}

/** How many nodes fit side by side in one narrow row. */
function narrowRowCapacity(viewportWidth: number): number {
  return Math.max(1, Math.floor((viewportWidth - SIMPLE_NARROW_INSET * 2) / SIMPLE_MIN_NARROW_PITCH))
}

/** Spacing between neighbouring nodes along a layer of `nodeCount` nodes (its fullest row, when narrow). */
function layerPitch(nodeCount: number, viewportWidth: number): number {
  if (viewportWidth < NARROW_MAP_WIDTH) {
    return (viewportWidth - SIMPLE_NARROW_INSET * 2) / Math.max(1, Math.min(nodeCount, narrowRowCapacity(viewportWidth)))
  }
  return wideExtent(nodeCount) / Math.max(1, nodeCount)
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
 * left by a cut cycle take no space. A narrow layer too crowded for one row
 * wraps onto extra rows, in reading order.
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
  const labelWidths = new Map<string, number>()
  /** Names never run past the map's sides. */
  const place = (id: string, x: number, y: number, room: number) => {
    positions.set(id, { x, y })
    labelWidths.set(id, Math.min(SIMPLE_LABEL_MAX_WIDTH, room, 2 * Math.min(x, viewportWidth - x)))
  }
  if (viewportWidth < NARROW_MAP_WIDTH) {
    const capacity = narrowRowCapacity(viewportWidth)
    let rowTop = SIMPLE_NARROW_TOP
    layers.forEach((ids, layer) => {
      if (layer > 0) rowTop += SIMPLE_ROW_PITCH
      const staggered = ids.length > SIMPLE_STAGGER_ABOVE
      for (let start = 0; start < ids.length; start += capacity) {
        if (start > 0) rowTop += SIMPLE_WRAP_PITCH
        const row = ids.slice(start, start + capacity)
        const pitch = layerPitch(row.length, viewportWidth)
        // A staggered name sits level with its neighbours' discs, so it stops short of their halos;
        // otherwise names share a line and split the pitch.
        const room = staggered ? 2 * (pitch - SIMPLE_DISC_SIZE / 2 - SIMPLE_HALO - SIMPLE_LABEL_GAP) : pitch - SIMPLE_LABEL_GAP
        row.forEach((id, index) => place(id, SIMPLE_NARROW_INSET + pitch * (index + 0.5),
          rowTop + (staggered && index % 2 === 1 ? SIMPLE_STAGGER : 0), room))
      }
    })
    return { positions, height: rowTop + SIMPLE_STAGGER + SIMPLE_DISC_SIZE / 2 + SIMPLE_LABEL_BAND, layers, labelWidths }
  }
  const columnWidth = viewportWidth / Math.max(1, layers.length)
  // Every layer keeps its own pitch; layers shorter than the tallest are centred on it.
  const extent = Math.max(SIMPLE_WIDE_HEIGHT, ...layers.map(ids => wideExtent(ids.length)))
  layers.forEach((ids, column) => {
    const pitch = layerPitch(ids.length, viewportWidth)
    const top = (extent - wideExtent(ids.length)) / 2
    ids.forEach((id, index) => place(id, columnWidth * (column + 0.5), top + pitch * (index + 0.5), columnWidth - 2 * SIMPLE_LABEL_GAP))
  })
  return { positions, height: extent + SIMPLE_LABEL_BAND, layers, labelWidths }
}
