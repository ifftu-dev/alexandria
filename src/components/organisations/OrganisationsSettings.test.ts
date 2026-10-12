import { flushPromises, mount } from '@vue/test-utils'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { computed, ref } from 'vue'
import OrganisationsSettings from './OrganisationsSettings.vue'

const mocks = vi.hoisted(() => ({ invoke: vi.fn(), push: vi.fn(), setSetting: vi.fn(), enabled: 'true' }))
vi.mock('@/composables/useLocalApi', () => ({ useLocalApi: () => ({ invoke: mocks.invoke }) }))
vi.mock('vue-i18n', () => ({ useI18n: () => ({ t: (key: string) => key }) }))
vi.mock('vue-router', () => ({ useRouter: () => ({ push: mocks.push }) }))
vi.mock('@/composables/useSettings', () => {
  const value = ref('true')
  return {
    useSettings: () => {
      value.value = mocks.enabled
      return {
        byKey: computed(() => new Map([['organisations.enabled', { key: 'organisations.enabled', current_value: value.value }]])),
        ready: ref(true),
        initialize: async () => {},
        setSetting: async (key: string, next: string) => {
          await mocks.setSetting(key, next)
          value.value = next
        },
      }
    },
  }
})

const stubs = { OrgRequests: { template: '<div data-testid="org-requests" />' }, OrgAccess: { props: ['cloudUrl'], template: '<div data-testid="org-access">{{ cloudUrl }}</div>' } }

function arm(cloud: string | null) {
  mocks.invoke.mockImplementation(async (command: string) => {
    if (command === 'org_cloud') return cloud
    throw new Error(command)
  })
}

beforeEach(() => {
  mocks.invoke.mockReset()
  mocks.push.mockReset()
  mocks.setSetting.mockReset()
  mocks.enabled = 'true'
})

describe('OrganisationsSettings', () => {
  it('shows requests and access when a Cloud is set', async () => {
    arm('https://cloud.example')
    const wrapper = mount(OrganisationsSettings, { props: { cloudSettingsSection: 'developer' }, global: { stubs } })
    await flushPromises()
    expect(wrapper.find('[data-testid=org-requests]').exists()).toBe(true)
    expect(wrapper.find('[data-testid=org-access]').text()).toBe('https://cloud.example')
  })

  it('says plainly when no Cloud is set and points developers at the setting', async () => {
    arm(null)
    const wrapper = mount(OrganisationsSettings, { props: { cloudSettingsSection: 'advanced' }, global: { stubs } })
    await flushPromises()
    expect(wrapper.find('[data-testid=organisations-no-cloud]').exists()).toBe(true)
    expect(wrapper.find('[data-testid=org-requests]').exists()).toBe(false)
    await wrapper.find('[data-testid=organisations-no-cloud] button').trigger('click')
    expect(mocks.push).toHaveBeenCalledWith('/settings/advanced')
  })

  it('hides everything else when switched off, and switches through the setting', async () => {
    mocks.enabled = 'false'
    arm(null)
    const wrapper = mount(OrganisationsSettings, { props: { cloudSettingsSection: 'developer' }, global: { stubs } })
    await flushPromises()
    expect(wrapper.find('[data-testid=organisations-off]').exists()).toBe(true)
    expect(wrapper.find('[data-testid=organisations-no-cloud]').exists()).toBe(false)

    arm('https://cloud.example')
    await wrapper.find('[role=switch]').trigger('click'); await flushPromises()
    expect(mocks.setSetting).toHaveBeenCalledWith('organisations.enabled', 'true')
    expect(wrapper.find('[data-testid=org-requests]').exists()).toBe(true)
  })
})
