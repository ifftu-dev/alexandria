// How a call lays out its video: equal tiles, as large as the space allows.
//
// Every participant gets the same box, like the tiled view of other video
// apps. For n tiles in a w x h area, each column count gives a tile size
// (limited by the width or by the rows' height, whichever binds first); the
// largest wins. Rows are then filled left to right, and a short last row is
// centred by the grid.

export interface VideoGridLayout {
  columns: number
  rows: number
  /** Tile size in CSS pixels. */
  width: number
  height: number
}

export function bestGrid(
  count: number,
  width: number,
  height: number,
  aspect = 16 / 9,
  gap = 12,
): VideoGridLayout {
  const none = { columns: 1, rows: Math.max(count, 1), width: 0, height: 0 }
  if (count <= 0 || width <= 0 || height <= 0) return none
  let best = none
  for (let columns = 1; columns <= count; columns++) {
    const rows = Math.ceil(count / columns)
    const byWidth = (width - gap * (columns - 1)) / columns
    const byHeight = ((height - gap * (rows - 1)) / rows) * aspect
    const tile = Math.floor(Math.min(byWidth, byHeight))
    if (tile > best.width) {
      best = { columns, rows, width: tile, height: Math.floor(tile / aspect) }
    }
  }
  return best
}
