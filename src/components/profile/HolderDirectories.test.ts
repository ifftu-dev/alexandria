import { flushPromises, mount } from '@vue/test-utils'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import HolderDirectories from './HolderDirectories.vue'
import { announceConsentChanged } from './directoryEvents'

const mocks = vi.hoisted(() => ({ invoke: vi.fn(), skills: [] as string[] }))
vi.mock('@/composables/useLocalApi', () => ({ useLocalApi: () => ({ invoke: mocks.invoke }) }))
vi.mock('vue-i18n', () => ({ useI18n: () => ({ t: (key: string) => key, locale: { value: 'en' } }) }))
vi.mock('vue-router', () => ({ useRouter: () => ({ push: vi.fn() }) }))

const empty = { items: [], problems: [] }

beforeEach(() => {
  mocks.skills = []
  mocks.invoke.mockReset().mockImplementation(async (command: string) => {
    if (command === 'list_directories') return [{ name: 'Local', url: 'http://127.0.0.1:8080' }]
    if (command === 'get_talent_index_preview') return { consent: { skills: mocks.skills } }
    if (command.startsWith('fetch_')) return empty
    throw new Error(command)
  })
})

const publishButton = (wrapper: ReturnType<typeof mount>) =>
  wrapper.findAll('button').find((b) => b.text() === 'profile.directories.publish')!

describe('HolderDirectories publishing', () => {
  it('disables publishing until a skill is consented, then enables it', async () => {
    const wrapper = mount(HolderDirectories)
    await flushPromises()
    expect(publishButton(wrapper).attributes('disabled')).toBeDefined()
    expect(wrapper.text()).toContain('profile.directories.consentHint')

    announceConsentChanged(1)
    await flushPromises()
    expect(publishButton(wrapper).attributes('disabled')).toBeUndefined()
    expect(wrapper.text()).not.toContain('profile.directories.consentHint')
    wrapper.unmount()
  })

  it('leaves publishing enabled when consent cannot be read', async () => {
    mocks.invoke.mockImplementation(async (command: string) => {
      if (command === 'list_directories') return [{ name: 'Local', url: 'http://127.0.0.1:8080' }]
      if (command === 'get_talent_index_preview') throw new Error('no identity')
      return empty
    })
    const wrapper = mount(HolderDirectories)
    await flushPromises()
    expect(publishButton(wrapper).attributes('disabled')).toBeUndefined()
    wrapper.unmount()
  })
})
