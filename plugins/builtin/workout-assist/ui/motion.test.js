import { expect, it } from 'vitest'
import { armRaise, compare, poseAt, trackable, validFrames } from './motion.js'

it('compares joint angles across different body sizes and positions', () => {
  const frames = armRaise()
  expect(validFrames(frames)).toBe(true)
  const raised = poseAt(frames, 2000)
  const shifted = raised.map(([x, y, c]) => [x * 0.6 + 20, y * 0.6 - 50, c])
  expect(trackable(shifted)).toBe(true)
  expect(compare(shifted, raised)[0].delta).toBeLessThan(0.001)
  expect(compare(raised, frames[0][1])[0].name).toMatch(/shoulder/)
})

it('withholds feedback across occlusions and tracking gaps', () => {
  const frames = armRaise()
  const missing = frames[0][1].map(([x, y]) => [x, y, 10])
  expect(trackable(missing)).toBe(false)
  expect(compare(frames[0][1], missing)).toEqual([])
  expect(poseAt([frames[0], frames.at(-1)], 2000)).toBeNull()
})

it('rejects malformed or unordered recorded lesson content', () => {
  const frames = armRaise()
  expect(validFrames([...frames.slice(0, 4), null])).toBe(false)
  expect(validFrames([...frames].reverse())).toBe(false)
  frames[1][1][8] = [Infinity, 100, 99]
  expect(validFrames(frames)).toBe(false)
})
