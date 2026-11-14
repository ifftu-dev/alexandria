import type { AtlasModel, AtlasNode } from '@/utils/skillAtlas'
import { longestPathLayers } from '@/utils/skillLayers'
import { NARROW_MAP_WIDTH } from './atlasLayout'

// Coordinates depend only on taxonomy membership, IDs and field slots, never
// credentials or view state. All values are pixels; there is no camera.
/** Diameter of a Simple-view disc. */
export const SIMPLE_DISC_SIZE = 46
/** Left-to-right layers spread each layer's nodes down this height. */
export const SIMPLE_WIDE_HEIGHT = 540
/** How far a ready-next disc's dashed halo reaches past the disc. */
export const SIMPLE_HALO = 6
/** Room above a disc in its node box for the halo and its dashed stroke. */
export const SIMPLE_RING_ROOM = 8
/** Room below a disc for its name: up to two lines. */
export const SIMPLE_LABEL_BAND = 44
/** The widest a name under a disc grows. */
export const SIMPLE_LABEL_MAX_WIDTH = 120
/** Clear space a name keeps from its neighbours across a layer. */
const SIMPLE_LABEL_GAP = 4
/**
 * Layers run left to right only when no layer packs nodes closer than a
 * disc, its name and the next disc's halo down the fixed height; otherwise
 * they run top to bottom in rows.
 */
export const SIMPLE_MIN_DISC_PITCH = SIMPLE_DISC_SIZE + SIMPLE_LABEL_BAND + SIMPLE_HALO
/** Rows step down one layer per row pitch. */
export const SIMPLE_ROW_PITCH = 156
/** Layers with more nodes than this stagger alternate nodes when their names would otherwise crowd. */
const SIMPLE_STAGGER_ABOVE = 3
/** How far a staggered node drops below its row: the height of a two-line name, so neighbouring names never share a line. */
const SIMPLE_STAGGER = 36
/**
 * Below this pitch (the spacing between neighbours along a layer) a layer is
 * dense: names of plain Ready skills show only on hover or focus.
 */
export const SIMPLE_LABEL_MIN_PITCH = 90
/** Rows: the first row's centre, and the side margin nodes spread within. */
const SIMPLE_ROWS_TOP = 56
const SIMPLE_ROWS_INSET = 12
/**
 * Rows never pack neighbours closer than this, which leaves a staggered
 * name room for a long word between its neighbours' halos: a layer that
 * would wraps onto extra rows instead.
 */
const SIMPLE_MIN_ROW_SPACING = 80
/** A wrapped row of the same layer sits this far below the row before: clear of its staggered discs and their names. */
const SIMPLE_WRAP_PITCH = SIMPLE_STAGGER + SIMPLE_DISC_SIZE + SIMPLE_LABEL_BAND + SIMPLE_HALO

/** `columns`: layers read left to right. `rows`: layers read top to bottom, each wrapped onto rows across the width. */
export type SimpleFlow = 'columns' | 'rows'

export interface SimpleLayout {
  flow: SimpleFlow
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

/** How many nodes fit side by side in one row. */
function rowCapacity(viewportWidth: number): number {
  return Math.max(1, Math.floor((viewportWidth - SIMPLE_ROWS_INSET * 2) / SIMPLE_MIN_ROW_SPACING))
}

/** Spacing between neighbouring nodes along a layer of `nodeCount` nodes (its fullest row, in rows). */
function layerPitch(nodeCount: number, viewportWidth: number, flow: SimpleFlow): number {
  if (flow === 'rows') return (viewportWidth - SIMPLE_ROWS_INSET * 2) / Math.max(1, Math.min(nodeCount, rowCapacity(viewportWidth)))
  return SIMPLE_WIDE_HEIGHT / Math.max(1, nodeCount)
}

/** A layer is dense when its nodes sit too close for every name to be legible. */
export function isDenseLayer(nodeCount: number, viewportWidth: number, flow: SimpleFlow): boolean {
  return nodeCount > 1 && layerPitch(nodeCount, viewportWidth, flow) < SIMPLE_LABEL_MIN_PITCH
}

/** Columns while the viewport is wide and every layer fits the fixed height at the minimum pitch. */
function chooseFlow(layers: readonly string[][], viewportWidth: number): SimpleFlow {
  if (viewportWidth < NARROW_MAP_WIDTH) return 'rows'
  const fullest = Math.max(0, ...layers.map(ids => ids.length))
  return fullest * SIMPLE_MIN_DISC_PITCH <= SIMPLE_WIDE_HEIGHT ? 'columns' : 'rows'
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
 * The Simple view's layered flow. Layers are counted along the longest
 * prerequisite path, and empty layers left by a cut cycle take no space. A
 * wide viewport whose layers all fit the fixed height reads left to right,
 * one column per layer. Otherwise layers read top to bottom, and a layer too
 * crowded for one row wraps onto extra rows across the full width, in
 * reading order, so a large taxonomy stays a glance rather than a scroll.
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

  const flow = chooseFlow(layers, viewportWidth)
  const positions = new Map<string, { x: number; y: number }>()
  const labelWidths = new Map<string, number>()
  /** Names never run past the map's sides. */
  const place = (id: string, x: number, y: number, room: number) => {
    positions.set(id, { x, y })
    labelWidths.set(id, Math.min(SIMPLE_LABEL_MAX_WIDTH, room, 2 * Math.min(x, viewportWidth - x)))
  }
  if (flow === 'rows') {
    const capacity = rowCapacity(viewportWidth)
    let rowTop = SIMPLE_ROWS_TOP
    layers.forEach((ids, layer) => {
      if (layer > 0) rowTop += SIMPLE_ROW_PITCH
      // Stagger only when side-by-side names would be held short of their full width.
      const staggered = ids.length > SIMPLE_STAGGER_ABOVE && layerPitch(ids.length, viewportWidth, flow) < SIMPLE_LABEL_MAX_WIDTH + SIMPLE_LABEL_GAP
      for (let start = 0; start < ids.length; start += capacity) {
        if (start > 0) rowTop += SIMPLE_WRAP_PITCH
        const row = ids.slice(start, start + capacity)
        const pitch = layerPitch(row.length, viewportWidth, flow)
        // A staggered name sits level with its neighbours' discs, so it stops short of their halos;
        // otherwise names share a line and split the pitch.
        const room = staggered ? 2 * (pitch - SIMPLE_DISC_SIZE / 2 - SIMPLE_HALO - SIMPLE_LABEL_GAP) : pitch - SIMPLE_LABEL_GAP
        row.forEach((id, index) => place(id, SIMPLE_ROWS_INSET + pitch * (index + 0.5),
          rowTop + (staggered && index % 2 === 1 ? SIMPLE_STAGGER : 0), room))
      }
    })
    return { flow, positions, height: rowTop + SIMPLE_STAGGER + SIMPLE_DISC_SIZE / 2 + SIMPLE_LABEL_BAND, layers, labelWidths }
  }
  const columnWidth = viewportWidth / Math.max(1, layers.length)
  layers.forEach((ids, column) => {
    const pitch = layerPitch(ids.length, viewportWidth, flow)
    ids.forEach((id, index) => place(id, columnWidth * (column + 0.5), pitch * (index + 0.5), columnWidth - 2 * SIMPLE_LABEL_GAP))
  })
  return { flow, positions, height: SIMPLE_WIDE_HEIGHT + SIMPLE_LABEL_BAND, layers, labelWidths }
}
