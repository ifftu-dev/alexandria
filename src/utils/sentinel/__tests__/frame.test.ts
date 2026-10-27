import { describe, expect, it } from 'vitest'
import { frameSize } from '../frame'

function still(complete: boolean, width: number, height: number): HTMLImageElement {
  const img = new Image()
  Object.defineProperty(img, 'complete', { value: complete })
  Object.defineProperty(img, 'naturalWidth', { value: width })
  Object.defineProperty(img, 'naturalHeight', { value: height })
  return img
}

function video(readyState: number, width: number, height: number): HTMLVideoElement {
  const el = document.createElement('video')
  Object.defineProperty(el, 'readyState', { value: readyState })
  Object.defineProperty(el, 'videoWidth', { value: width })
  Object.defineProperty(el, 'videoHeight', { value: height })
  return el
}

describe('frameSize', () => {
  it('reads a decoded still at its natural size', () => {
    expect(frameSize(still(true, 480, 640))).toEqual({ width: 480, height: 640 })
  })

  it('has nothing to give for a still that has not decoded', () => {
    expect(frameSize(still(false, 480, 640))).toBeNull()
    expect(frameSize(still(true, 0, 0))).toBeNull()
  })

  it('reads a video once it has a current frame', () => {
    expect(frameSize(video(4, 1280, 720))).toEqual({ width: 1280, height: 720 })
    expect(frameSize(video(1, 1280, 720))).toBeNull()
  })

  it('falls back to 640×480 for a playing video that has not reported its size', () => {
    expect(frameSize(video(2, 0, 0))).toEqual({ width: 640, height: 480 })
  })
})
