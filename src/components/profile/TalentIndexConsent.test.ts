import { flushPromises, mount } from '@vue/test-utils'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { computed, ref } from 'vue'
import TalentIndexConsent from './TalentIndexConsent.vue'

const mocks = vi.hoisted(() => ({ invoke: vi.fn(), setSetting: vi.fn(), sync: 'false' }))
vi.mock('@/composables/useLocalApi', () => ({ useLocalApi: () => ({ invoke: mocks.invoke }) }))
vi.mock('vue-i18n', () => ({ useI18n: () => ({ t: (key: string) => key }) }))
vi.mock('@/composables/useSettings', () => {
  const value = ref('false')
  return {
    useSettings: () => {
      value.value = mocks.sync
      return {
        byKey: computed(() => new Map([['talent_index.sync', { key: 'talent_index.sync', current_value: value.value }]])),
        initialize: async () => {},
        setSetting: async (key: string, next: string) => {
          await mocks.setSetting(key, next)
          value.value = next
        },
      }
    },
  }
})

function preview(consented: string[]) {
  return {
    candidates: [{ skillId: 'skill_big_o', name: 'Big-O Analysis', level: 5, issuerClusters: 0, bloomLevel: 0, consented: consented.length > 0 }],
    consent: { skills: consented, displayName: false, bio: false },
    record: null,
  }
}

function arm(consented: string[], publish: () => unknown = () => null) {
  mocks.invoke.mockImplementation(async (command: string) => {
    if (command === 'get_talent_index_preview') return preview(consented)
    if (command === 'publish_listing') return publish()
    throw new Error(command)
  })
}

const publishButton = (wrapper: ReturnType<typeof mount>) =>
  wrapper.findAll('button').find((b) => b.text() === 'organisations.listing.publishNow')!

beforeEach(() => {
  mocks.invoke.mockReset()
  mocks.setSetting.mockReset()
  mocks.sync = 'false'
})

describe('TalentIndexConsent publishing', () => {
  it('says saving and publishing are separate steps', async () => {
    arm([])
    const wrapper = mount(TalentIndexConsent); await flushPromises()
    expect(wrapper.find('[data-testid=talent-index-framing]').text()).toBe('organisations.listing.framing')
  })

  it('cannot publish until a skill has been saved', async () => {
    arm([])
    const wrapper = mount(TalentIndexConsent); await flushPromises()
    expect(publishButton(wrapper).attributes('disabled')).toBeDefined()
    expect(wrapper.text()).toContain('organisations.listing.nothingToPublish')
  })

  it('publishes now to the built-in Cloud, with no address to choose', async () => {
    arm(['skill_big_o'])
    const wrapper = mount(TalentIndexConsent); await flushPromises()
    await publishButton(wrapper).trigger('click'); await flushPromises()
    expect(mocks.invoke).toHaveBeenCalledWith('publish_listing')
    expect(wrapper.text()).toContain('organisations.listing.published')
  })

  it('shows why publishing failed', async () => {
    arm(['skill_big_o'], () => { throw new Error('no Alexandria Cloud is set') })
    const wrapper = mount(TalentIndexConsent); await flushPromises()
    await publishButton(wrapper).trigger('click'); await flushPromises()
    expect(wrapper.find('[data-testid=talent-index-publish] [role=alert]').text()).toBe('organisations.listing.publishFailed')
  })

  it('keeps the listing up to date through the talent_index.sync setting', async () => {
    arm(['skill_big_o'])
    const wrapper = mount(TalentIndexConsent); await flushPromises()
    const toggle = wrapper.find('[data-testid=talent-index-publish] [role=switch]')
    expect(toggle.attributes('aria-checked')).toBe('false')
    await toggle.trigger('click'); await flushPromises()
    expect(mocks.setSetting).toHaveBeenCalledWith('talent_index.sync', 'true')
    expect(wrapper.find('[data-testid=talent-index-publish] [role=switch]').attributes('aria-checked')).toBe('true')
  })

  it('shows the switch on when the setting already is', async () => {
    mocks.sync = 'true'
    arm(['skill_big_o'])
    const wrapper = mount(TalentIndexConsent); await flushPromises()
    expect(wrapper.find('[data-testid=talent-index-publish] [role=switch]').attributes('aria-checked')).toBe('true')
  })
})
