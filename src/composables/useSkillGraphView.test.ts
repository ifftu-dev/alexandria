import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { SettingEntry } from './useSettings'

const mocks = vi.hoisted(() => ({
  invoke: vi.fn<(command: string, args?: Record<string, unknown>) => Promise<unknown>>(),
}))

vi.mock('./useLocalApi', () => ({ useLocalApi: () => ({ invoke: mocks.invoke }) }))
vi.mock('@tauri-apps/api/event', () => ({ listen: async () => () => {} }))

const PAGE_KEY = 'ui.skill_graph_view'
const EXPANDED_KEY = 'ui.skill_graph_expanded_view'

function viewEntry(key: string, value: string): SettingEntry {
  return {
    key, scope: 'sync', category: 'Appearance', label: 'Skill graph view',
    description: '', kind: 'string', default_value: key === PAGE_KEY ? 'simple' : 'web', current_value: value, is_default: false,
  }
}

let stored: SettingEntry[]

async function loaded() {
  vi.resetModules()
  const { useSettings } = await import('./useSettings')
  await useSettings().initialize()
  return import('./useSkillGraphView')
}

beforeEach(() => {
  stored = []
  mocks.invoke.mockReset().mockImplementation(async (command, args) => {
    if (command === 'list_settings') return stored
    if (command === 'set_setting') return null
    throw new Error(`Unexpected IPC: ${command} ${JSON.stringify(args)}`)
  })
})

describe('useSkillGraphView', () => {
  it('reads each surface\'s own default when its setting is missing: Simple for the page, Web for the expanded graph', async () => {
    const { useSkillGraphView } = await loaded()
    expect(useSkillGraphView('page').value).toBe('simple')
    expect(useSkillGraphView('modal').value).toBe('web')
  })

  it('reads each surface\'s remembered view and narrows unknown values to that surface\'s default', async () => {
    stored = [viewEntry(PAGE_KEY, 'web'), viewEntry(EXPANDED_KEY, 'detailed')]
    let accessor = (await loaded()).useSkillGraphView
    expect(accessor('page').value).toBe('web')
    expect(accessor('modal').value).toBe('detailed')
    stored = [viewEntry(PAGE_KEY, 'fancy'), viewEntry(EXPANDED_KEY, 'Web')]
    accessor = (await loaded()).useSkillGraphView
    expect(accessor('page').value).toBe('simple')
    expect(accessor('modal').value).toBe('web')
  })

  it('writes the chosen view to that surface\'s key only', async () => {
    stored = [viewEntry(PAGE_KEY, 'simple'), viewEntry(EXPANDED_KEY, 'web')]
    const { useSkillGraphView } = await loaded()
    const page = useSkillGraphView('page'), expanded = useSkillGraphView('modal')
    expanded.value = 'detailed'
    await vi.waitFor(() => expect(expanded.value).toBe('detailed'))
    expect(mocks.invoke).toHaveBeenCalledWith('set_setting', { key: EXPANDED_KEY, value: 'detailed' })
    expect(page.value).toBe('simple')
    page.value = 'web'
    await vi.waitFor(() => expect(page.value).toBe('web'))
    expect(mocks.invoke).toHaveBeenCalledWith('set_setting', { key: PAGE_KEY, value: 'web' })
    expect(expanded.value).toBe('detailed')
  })
})

describe('isSkillGraphView', () => {
  it('accepts only the three views', async () => {
    const { isSkillGraphView } = await import('./useSkillGraphView')
    for (const value of ['web', 'simple', 'detailed']) expect(isSkillGraphView(value)).toBe(true)
    for (const value of ['Web', 'Simple', 'fancy', '', null, undefined, 1, {}]) {
      expect(isSkillGraphView(value)).toBe(false)
    }
  })
})
