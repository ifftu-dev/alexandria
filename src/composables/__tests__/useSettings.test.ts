import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { SettingEntry } from '../useSettings'

const mocks = vi.hoisted(() => ({
  invoke: vi.fn<(command: string, args?: Record<string, unknown>) => Promise<unknown>>(),
  listen: vi.fn<(event: string, callback: () => Promise<void>) => Promise<() => void>>(),
  unlisten: vi.fn(),
}))

vi.mock('../useLocalApi', () => ({ useLocalApi: () => ({ invoke: mocks.invoke }) }))
vi.mock('@tauri-apps/api/event', () => ({ listen: mocks.listen }))

function deferred<T>() {
  let resolve!: (value: T) => void
  const promise = new Promise<T>(res => { resolve = res })
  return { promise, resolve }
}

function entry(value: string): SettingEntry {
  return {
    key: 'ui.theme', scope: 'sync', category: 'ui', label: 'Theme', description: '',
    kind: 'string', default_value: 'system', current_value: value, is_default: false,
  }
}

async function freshSettings() {
  vi.resetModules()
  return import('../useSettings')
}

beforeEach(() => {
  mocks.invoke.mockReset().mockResolvedValue([entry('dark')])
  mocks.unlisten.mockReset()
  mocks.listen.mockReset().mockResolvedValue(mocks.unlisten)
})

describe('profile-scoped settings lifecycle', () => {
  it('clearing the cache discards an outstanding refresh result', async () => {
    const { useSettings, clearSettingsCache } = await freshSettings()
    const service = useSettings()
    const response = deferred<SettingEntry[]>()
    mocks.invoke.mockReturnValue(response.promise)
    const refreshing = service.refresh()
    clearSettingsCache()
    response.resolve([entry('old-profile')])
    await refreshing
    expect(service.entries.value).toEqual([])
    expect(service.ready.value).toBe(false)
  })

  it('concurrent initialization callers wait for the same work', async () => {
    const service = (await freshSettings()).useSettings()
    const response = deferred<SettingEntry[]>()
    mocks.invoke.mockReturnValue(response.promise)
    const first = service.initialize()
    expect(service.initialize()).toBe(first)
    expect(service.ready.value).toBe(false)
    response.resolve([entry('dark')])
    await first
    expect(mocks.invoke).toHaveBeenCalledOnce()
    expect(mocks.listen).toHaveBeenCalledOnce()
    expect(service.ready.value).toBe(true)
  })

  it('releases a late listener without replacing the next profile listener', async () => {
    const { useSettings, clearSettingsCache } = await freshSettings()
    const service = useSettings()
    const listener = deferred<() => void>()
    const requested = deferred<void>()
    const oldRelease = vi.fn()
    mocks.listen.mockImplementationOnce(async () => {
      requested.resolve()
      return listener.promise
    })
    const first = service.initialize()
    await requested.promise
    clearSettingsCache()
    await service.initialize()
    listener.resolve(oldRelease)
    await first
    expect(oldRelease).toHaveBeenCalledOnce()
    expect(mocks.unlisten).not.toHaveBeenCalled()
    expect(service.ready.value).toBe(true)
    clearSettingsCache()
    expect(mocks.unlisten).toHaveBeenCalledOnce()
  })

  it('ignores events delivered to an old listener after clearing', async () => {
    const { useSettings, clearSettingsCache } = await freshSettings()
    await useSettings().initialize()
    const callback = mocks.listen.mock.calls[0]![1]
    clearSettingsCache()
    mocks.invoke.mockClear()
    await callback()
    expect(mocks.invoke).not.toHaveBeenCalled()
    expect(mocks.unlisten).toHaveBeenCalledOnce()
  })

  it('does not apply a late setting write to the next profile cache', async () => {
    const { useSettings, clearSettingsCache } = await freshSettings()
    const service = useSettings()
    await service.initialize()
    const write = deferred<void>()
    mocks.invoke.mockImplementation(async command => command === 'set_setting' ? write.promise : [entry('new-profile')])
    const saving = service.setSetting('ui.theme', 'old-profile-change')
    clearSettingsCache()
    await service.initialize()
    write.resolve()
    await saving
    expect(service.byKey.value.get('ui.theme')?.current_value).toBe('new-profile')
  })
})
