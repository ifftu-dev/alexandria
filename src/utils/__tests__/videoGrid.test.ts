import { describe, expect, it } from 'vitest'
import { bestGrid } from '../videoGrid'

describe('video grid', () => {
  it('gives one participant the whole stage', () => {
    expect(bestGrid(1, 1600, 900, 16 / 9, 0)).toEqual({ columns: 1, rows: 1, width: 1600, height: 900 })
  })

  it('puts two side by side on a wide screen and stacks them on a phone', () => {
    expect(bestGrid(2, 1600, 600, 16 / 9, 0).columns).toBe(2)
    expect(bestGrid(2, 390, 700, 16 / 9, 0).columns).toBe(1)
  })

  it('arranges four as two by two in a 16:9 area', () => {
    const grid = bestGrid(4, 1600, 900, 16 / 9, 0)
    expect([grid.columns, grid.rows, grid.width]).toEqual([2, 2, 800])
  })

  it('keeps every tile the same size and inside the area, gaps included', () => {
    for (const count of [1, 2, 3, 5, 7, 9, 12]) {
      const grid = bestGrid(count, 1200, 700, 16 / 9, 12)
      expect(grid.columns * grid.width + (grid.columns - 1) * 12).toBeLessThanOrEqual(1200)
      expect(grid.rows * grid.height + (grid.rows - 1) * 12).toBeLessThanOrEqual(700)
      expect(grid.columns * grid.rows).toBeGreaterThanOrEqual(count)
    }
  })

  it('draws nothing until the area has been measured', () => {
    expect(bestGrid(3, 0, 0).width).toBe(0)
    expect(bestGrid(0, 800, 600).width).toBe(0)
  })
})
