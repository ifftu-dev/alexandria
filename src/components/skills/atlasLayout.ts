import type { AtlasField, AtlasModel } from '@/utils/skillAtlas'

// Coordinates depend only on taxonomy membership, IDs and field slots, never
// credentials or view state.
export const NODE_WIDTH = 224
export const NODE_HEIGHT = 116
/** Nodes per row inside a region, and regions per row of the map. */
const NODE_COLUMNS = 3
const REGION_COLUMNS = 3
/** Space between neighbouring nodes, across and down. */
const NODE_GAP_X = 32
const NODE_GAP_Y = 40
/** A region's inner margin, and the band above its nodes for its heading. */
const REGION_INSET = 32
const REGION_HEADING_HEIGHT = 100
/** Space between neighbouring regions, across and down. */
const REGION_GAP_X = 40
const REGION_GAP_Y = 48

const NODE_PITCH_X = NODE_WIDTH + NODE_GAP_X
const NODE_PITCH_Y = NODE_HEIGHT + NODE_GAP_Y
export const REGION_WIDTH = REGION_INSET * 2 + NODE_COLUMNS * NODE_PITCH_X - NODE_GAP_X
const REGION_PITCH_X = REGION_WIDTH + REGION_GAP_X
/** Where a region's name and counts sit inside its heading band. */
export const REGION_HEADING_BOX = { dx: REGION_INSET, dy: 18, width: REGION_WIDTH - REGION_INSET * 2, height: 72 } as const

/** The map viewport's height, in map units at zoom 1. */
export const MAP_HEIGHT = 520
/** Below this viewport width, framing a region shows its first node column instead of its centre. */
export const NARROW_MAP_WIDTH = 600
/** How far below a region's top edge a framed view starts. */
const REGION_FRAME_TOP = 30

/** The camera that frames a region in a viewport of the given width. */
export function regionCamera(region: { x: number; y: number }, viewportWidth: number) {
  return {
    x: region.x + (viewportWidth < NARROW_MAP_WIDTH ? REGION_INSET + NODE_WIDTH / 2 : REGION_WIDTH / 2),
    y: region.y + MAP_HEIGHT / 2 + REGION_FRAME_TOP,
    zoom: 1,
  }
}

function regionHeight(field: AtlasField): number {
  return REGION_HEADING_HEIGHT + Math.ceil(field.total / NODE_COLUMNS) * NODE_PITCH_Y + REGION_INSET
}

export interface AtlasRegion { field: AtlasField; x: number; y: number; width: number; height: number }

/**
 * Each field sits in the map cell of its atlas slot, so adding a field never
 * moves another. Unassigned skills take the cell after the last slot. Rows
 * with no loaded field take no space.
 */
export function atlasLayout(model: AtlasModel) {
  const lastSlot = Math.max(-1, ...model.fields.map(field => field.slot ?? -1))
  const cells = model.fields.map(field => ({ field, cell: field.slot ?? lastSlot + 1 }))
  const rowHeights = new Map<number, number>()
  for (const { field, cell } of cells) {
    const row = Math.floor(cell / REGION_COLUMNS)
    rowHeights.set(row, Math.max(rowHeights.get(row) ?? 0, regionHeight(field)))
  }
  const rowY = new Map<number, number>()
  let y = 0
  for (const row of [...rowHeights.keys()].sort((a, b) => a - b)) {
    rowY.set(row, y)
    y += (rowHeights.get(row) ?? 0) + REGION_GAP_Y
  }
  const positions = new Map<string, { x: number; y: number }>()
  const regions: AtlasRegion[] = cells.map(({ field, cell }) => {
    const x = (cell % REGION_COLUMNS) * REGION_PITCH_X
    const y = rowY.get(Math.floor(cell / REGION_COLUMNS)) ?? 0
    field.nodeIds.forEach((id, nodeIndex) => positions.set(id, {
      x: x + REGION_INSET + (nodeIndex % NODE_COLUMNS) * NODE_PITCH_X,
      y: y + REGION_HEADING_HEIGHT + Math.floor(nodeIndex / NODE_COLUMNS) * NODE_PITCH_Y,
    }))
    return { field, x, y, width: REGION_WIDTH, height: regionHeight(field) }
  })
  return { positions, regions }
}
