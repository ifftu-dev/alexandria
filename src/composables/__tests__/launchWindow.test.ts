import { afterEach, expect, test, vi } from 'vitest'
import { FIRST_PAINT_FALLBACK_MS, showOnFirstPaint } from '../launchWindow'

afterEach(() => { vi.useRealTimers() })

test('the window is shown once, on the first frame', () => {
  vi.useFakeTimers()
  const frames: FrameRequestCallback[] = []
  const show = vi.fn()
  showOnFirstPaint(show, { requestFrame: cb => { frames.push(cb); return frames.length } })
  expect(show).not.toHaveBeenCalled()
  frames.forEach(cb => cb(16))
  expect(show).toHaveBeenCalledTimes(1)
  vi.advanceTimersByTime(FIRST_PAINT_FALLBACK_MS)
  expect(show).toHaveBeenCalledTimes(1)
  expect(vi.getTimerCount()).toBe(0)
})

test('a hidden webview that never schedules a frame is shown after the fallback timeout', () => {
  vi.useFakeTimers()
  const show = vi.fn()
  showOnFirstPaint(show, { requestFrame: () => 0 })
  vi.advanceTimersByTime(FIRST_PAINT_FALLBACK_MS - 1)
  expect(show).not.toHaveBeenCalled()
  vi.advanceTimersByTime(1)
  expect(show).toHaveBeenCalledTimes(1)
  expect(FIRST_PAINT_FALLBACK_MS).toBeLessThanOrEqual(250)
})
