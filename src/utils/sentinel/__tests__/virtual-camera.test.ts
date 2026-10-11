import { describe, expect, it } from 'vitest'
import { VIRTUAL_CAMERA_MARKERS, isVirtualCameraLabel } from '../virtual-camera'

describe('isVirtualCameraLabel', () => {
  it('flags well-known virtual camera products regardless of case', () => {
    for (const label of ['OBS Virtual Camera', 'ManyCam Virtual Webcam', 'Snap Camera', 'XSplit VCam', 'Camo Camera', 'v4l2loopback (0x0001)']) {
      expect(isVirtualCameraLabel(label), label).toBe(true)
    }
  })

  it('passes real webcams', () => {
    for (const label of ['FaceTime HD Camera (Built-in)', 'Logitech BRIO', 'Integrated Camera', 'HD Pro Webcam C920', 'Razer Kiyo']) {
      expect(isVirtualCameraLabel(label), label).toBe(false)
    }
  })

  it('treats a missing label as unknown, not virtual', () => {
    expect(isVirtualCameraLabel(null)).toBe(false)
    expect(isVirtualCameraLabel(undefined)).toBe(false)
    expect(isVirtualCameraLabel('')).toBe(false)
  })

  it('keeps every marker lower-cased and at least four characters', () => {
    for (const marker of VIRTUAL_CAMERA_MARKERS) {
      expect(marker).toBe(marker.toLowerCase())
      expect(marker.length).toBeGreaterThanOrEqual(4)
    }
  })
})
