import { beforeEach, describe, expect, it, vi } from 'vitest'

const mocks = vi.hoisted(() => ({
  invoke: vi.fn<(command: string, args?: Record<string, unknown>) => Promise<unknown>>(),
}))

vi.mock('../useLocalApi', () => ({ useLocalApi: () => ({ invoke: mocks.invoke }) }))
vi.mock('../useProfiles', () => ({ onProfileLocked: () => undefined, onProfileReady: () => () => undefined }))
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn() }))

function deferred<T>() {
  let resolve!: (value: T) => void
  const promise = new Promise<T>((res) => { resolve = res })
  return { promise, resolve }
}

beforeEach(() => { mocks.invoke.mockReset() })

describe('camera, microphone and screen changes', () => {
  it('runs one at a time, so a second click cannot undo the first', async () => {
    const { useTutoringRoom } = await import('../useTutoringRoom')
    const room = useTutoringRoom()
    const toggle = deferred<boolean>()
    mocks.invoke.mockImplementation(async (command) => {
      if (command === 'tutoring_toggle_video') return toggle.promise
      if (command === 'tutoring_status') return null
      return undefined
    })

    const first = room.toggleVideo(false)
    expect(room.mediaBusy.value).toBe(true)
    // A second click while the first is applying is ignored, not sent.
    await room.toggleScreenShare(true)
    expect(mocks.invoke.mock.calls.filter(([command]) => command === 'tutoring_toggle_screen_share')).toHaveLength(0)

    toggle.resolve(false)
    await first
    expect(room.mediaBusy.value).toBe(false)
  })

  it('switches the camera and reports the one now in use', async () => {
    const { useTutoringRoom } = await import('../useTutoringRoom')
    const room = useTutoringRoom()
    mocks.invoke.mockImplementation(async (command) => (command === 'tutoring_set_camera' ? 'MacBook Pro Camera' : null))
    await expect(room.setCamera('Index(0)')).resolves.toBe('MacBook Pro Camera')
    expect(mocks.invoke).toHaveBeenCalledWith('tutoring_set_camera', { cameraId: 'Index(0)' })
  })
})
