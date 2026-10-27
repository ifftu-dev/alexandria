import { afterEach, describe, expect, it, vi } from 'vitest'
import { cameraDeviceId, openCameraPreview } from '../callCamera'

const devices = [
  { kind: 'videoinput', label: 'iPhone Camera', deviceId: 'cam-iphone' },
  { kind: 'audioinput', label: 'MacBook Pro Camera', deviceId: 'not-a-camera' },
  { kind: 'videoinput', label: 'MacBook Pro Camera', deviceId: 'cam-mbp' },
]
const getUserMedia = vi.fn()

function stubMedia() {
  Object.defineProperty(navigator, 'mediaDevices', {
    configurable: true,
    value: { enumerateDevices: vi.fn().mockResolvedValue(devices), getUserMedia },
  })
}

afterEach(() => { getUserMedia.mockReset() })

describe('cameraDeviceId', () => {
  it('finds the webview camera by the name the call reports', async () => {
    stubMedia()
    expect(await cameraDeviceId('MacBook Pro Camera')).toBe('cam-mbp')
  })

  it('has nothing for an unknown or missing name', async () => {
    stubMedia()
    expect(await cameraDeviceId('OBS Virtual Camera')).toBeNull()
    expect(await cameraDeviceId(null)).toBeNull()
  })
})

describe('openCameraPreview', () => {
  it('opens exactly that camera, never the system default', async () => {
    stubMedia()
    const stream = {} as MediaStream
    getUserMedia.mockResolvedValue(stream)
    expect(await openCameraPreview('MacBook Pro Camera')).toBe(stream)
    expect(getUserMedia.mock.calls[0]![0].video.deviceId).toEqual({ exact: 'cam-mbp' })
  })

  it('opens nothing when the camera cannot be matched', async () => {
    stubMedia()
    expect(await openCameraPreview('Unknown Camera')).toBeNull()
    expect(getUserMedia).not.toHaveBeenCalled()
  })
})
