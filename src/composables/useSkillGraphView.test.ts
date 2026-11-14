import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { SettingEntry } from './useSettings'

const mocks = vi.hoisted(() => ({
  invoke: vi.fn<(command: string, args?: Record<string, unknown>) => Promise<unknown>>(),
}))

vi.mock('./useLocalApi', () => ({ useLocalApi: () => ({ invoke: mocks.invoke }) }))
vi.mock('@tauri-apps/api/event', () => ({ listen: async () => () => {} }))

function viewEntry(value: string): SettingEntry {
  return {
    key: 'ui.skill_graph_view', scope: 'sync', category: 'Appearance', label: 'Skill graph view',
    description: '', kind: 'string', default_value: 'simple', current_value: value, is_default: false,
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
  it('reads Simple when the setting is missing', async () => {
    const { useSkillGraphView } = await loaded()
    expect(useSkillGraphView().value).toBe('simple')
  })

  it('reads the remembered view and narrows unknown values to Simple', async () => {
    stored = [viewEntry('detailed')]
    expect((await loaded()).useSkillGraphView().value).toBe('detailed')
    stored = [viewEntry('fancy')]
    expect((await loaded()).useSkillGraphView().value).toBe('simple')
  })

  it('writes the chosen view through to the setting', async () => {
    stored = [viewEntry('simple')]
    const { useSkillGraphView } = await loaded()
    const view = useSkillGraphView()
    view.value = 'detailed'
    await vi.waitFor(() => expect(view.value).toBe('detailed'))
    expect(mocks.invoke).toHaveBeenCalledWith('set_setting', { key: 'ui.skill_graph_view', value: 'detailed' })
  })
})

describe('isSkillGraphView', () => {
  it('accepts only the two views', async () => {
    const { isSkillGraphView } = await import('./useSkillGraphView')
    expect(isSkillGraphView('simple')).toBe(true)
    expect(isSkillGraphView('detailed')).toBe(true)
    for (const value of ['Simple', 'fancy', '', null, undefined, 1, {}]) {
      expect(isSkillGraphView(value)).toBe(false)
    }
  })
})
