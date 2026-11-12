import { describe, expect, it } from 'vitest'
import { downscaledSize, frameHeaders, frameSize, rgbaBody } from '../frame'

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

describe('binary frame IPC', () => {
  it('caps the longest side at 224 px and keeps the aspect ratio', () => {
    expect(downscaledSize({ width: 1280, height: 720 })).toEqual({ width: 224, height: 126 })
    expect(downscaledSize({ width: 480, height: 640 })).toEqual({ width: 168, height: 224 })
    expect(downscaledSize({ width: 100, height: 80 })).toEqual({ width: 100, height: 80 })
  })

  it('sends the pixels as bytes over the same buffer, not a number array', () => {
    const data = new Uint8ClampedArray([1, 2, 3, 255, 4, 5, 6, 255])
    const body = rgbaBody({ data, width: 2, height: 1 } as ImageData)
    expect(body).toBeInstanceOf(Uint8Array)
    expect(body.buffer).toBe(data.buffer)
    expect(Array.from(body)).toEqual([1, 2, 3, 255, 4, 5, 6, 255])
  })

  it('carries the frame size and identity in headers', () => {
    expect(frameHeaders(224, 168, { user: 'stake1', device: 'abc', preview: true })).toEqual({
      'x-sentinel-frame-width': '224',
      'x-sentinel-frame-height': '168',
      'x-sentinel-user': 'stake1',
      'x-sentinel-device': 'abc',
      'x-sentinel-preview': '1',
    })
    expect(frameHeaders(10, 20)).toEqual({
      'x-sentinel-frame-width': '10',
      'x-sentinel-frame-height': '20',
    })
  })
})
