import { flushPromises, mount } from '@vue/test-utils'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import TalentIndexConsent from './TalentIndexConsent.vue'
import { announceDirectoriesChanged } from './directoryEvents'

const mocks = vi.hoisted(() => ({ invoke: vi.fn() }))
vi.mock('@/composables/useLocalApi', () => ({ useLocalApi: () => ({ invoke: mocks.invoke }) }))
vi.mock('vue-i18n', () => ({ useI18n: () => ({ t: (key: string) => key }) }))

const preview = {
  candidates: [{ skillId: 'skill_big_o', name: 'Big-O Analysis', level: 5, issuerClusters: 0, bloomLevel: 0, consented: false }],
  consent: { skills: [], displayName: false, bio: false },
  record: null,
}

function arm(directories: unknown[] | Error) {
  mocks.invoke.mockImplementation(async (command: string) => {
    if (command === 'get_talent_index_preview') return preview
    if (command === 'list_directories') {
      if (directories instanceof Error) throw directories
      return directories
    }
    throw new Error(command)
  })
}

const framing = (wrapper: ReturnType<typeof mount>) => wrapper.find('[data-testid=talent-index-framing]')

beforeEach(() => { mocks.invoke.mockReset() })

describe('TalentIndexConsent framing', () => {
  it('says nothing is sent when no directory has been added', async () => {
    arm([])
    const wrapper = mount(TalentIndexConsent); await flushPromises()
    expect(framing(wrapper).text()).toBe('profile.talentIndex.notPublishedYet')
  })

  it('says publishing is a separate step once a directory exists', async () => {
    arm([{ name: 'Local demo', url: 'http://127.0.0.1:8787' }])
    const wrapper = mount(TalentIndexConsent); await flushPromises()
    expect(framing(wrapper).text()).toBe('profile.talentIndex.publishSeparately')
  })

  it('follows a directory added on the same page without a reload', async () => {
    arm([])
    const wrapper = mount(TalentIndexConsent, { attachTo: document.body }); await flushPromises()
    expect(framing(wrapper).text()).toBe('profile.talentIndex.notPublishedYet')
    announceDirectoriesChanged(1); await flushPromises()
    expect(framing(wrapper).text()).toBe('profile.talentIndex.publishSeparately')
    wrapper.unmount()
  })

  it('makes neither claim when the directory list cannot be read', async () => {
    arm(new Error('locked'))
    const wrapper = mount(TalentIndexConsent); await flushPromises()
    expect(framing(wrapper).exists()).toBe(false)
  })
})
