import type { AtlasModel } from '@/utils/skillAtlas'

// Coordinates depend only on taxonomy membership and IDs, never credentials or view state.
export const NODE_WIDTH = 224
export const NODE_HEIGHT = 116
export function atlasLayout(model: AtlasModel) {
  const positions = new Map<string, { x: number; y: number }>()
  let rowY = 0
  const regions = model.fields.map((field, index) => {
    if (index > 0 && index % 3 === 0) {
      const previous = model.fields.slice(index - 3, index)
      rowY += Math.max(...previous.map(item => Math.ceil(item.total / 3))) * 156 + 180
    }
    const x = (index % 3) * 840
    const y = rowY
    field.nodeIds.forEach((id, nodeIndex) => positions.set(id, {
      x: x + 32 + (nodeIndex % 3) * 256,
      y: y + 100 + Math.floor(nodeIndex / 3) * 156,
    }))
    return { field, x, y, width: 800, height: Math.ceil(field.total / 3) * 156 + 132, color: index % 8 }
  })
  return { positions, regions }
}
