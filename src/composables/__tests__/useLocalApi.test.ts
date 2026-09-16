import { beforeEach, expect, it, vi } from 'vitest'

const transport = vi.hoisted(() => ({ invoke: vi.fn() }))
vi.mock('@tauri-apps/api/core', () => ({ invoke: transport.invoke }))

beforeEach(() => {
  vi.resetModules()
  transport.invoke.mockReset()
})

it('captures the session before sending an IPC request', async () => {
  const { setProfileSessionToken } = await import('../profileSession')
  const { useLocalApi } = await import('../useLocalApi')
  setProfileSessionToken('session-a')
  transport.invoke.mockResolvedValue(['course'])
  await expect(useLocalApi().invoke('list_courses')).resolves.toEqual(['course'])
  expect(transport.invoke).toHaveBeenCalledWith('list_courses', undefined, {
    headers: { 'x-alexandria-profile-session': 'session-a' },
  })
})

it('rejects a late private response after switching or locking', async () => {
  const { setProfileSessionToken } = await import('../profileSession')
  const { useLocalApi } = await import('../useLocalApi')
  let resolve!: (value: unknown) => void
  transport.invoke.mockReturnValue(new Promise(res => { resolve = res }))
  setProfileSessionToken('session-a')
  const pending = useLocalApi().invoke('get_wallet_info')
  const assertion = expect(pending).rejects.toThrow('Profile session changed')
  setProfileSessionToken('session-b')
  resolve({ private: 'profile-a' })
  await assertion
})

it('allows device-level responses to complete across profile changes', async () => {
  const { setProfileSessionToken } = await import('../profileSession')
  const { useLocalApi } = await import('../useLocalApi')
  let resolve!: (value: unknown) => void
  transport.invoke.mockReturnValue(new Promise(res => { resolve = res }))
  setProfileSessionToken('session-a')
  const pending = useLocalApi().invoke('check_health')
  setProfileSessionToken(null)
  resolve({ status: 'ok' })
  await expect(pending).resolves.toEqual({ status: 'ok' })
})
