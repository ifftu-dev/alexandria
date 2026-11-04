import { flushPromises, mount } from '@vue/test-utils'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { ref } from 'vue'
import { createI18n } from 'vue-i18n'
import en from '@/locales/en'
import type { PushStatus } from '@/types'
import NotificationsPanel from './NotificationsPanel.vue'

const mocks = vi.hoisted(() => ({ status: null as PushStatus | null, register: vi.fn() }))
vi.mock('@/composables/usePlatform', () => ({ isMobilePlatform: true }))
vi.mock('@/composables/profileSession', () => ({ getProfileSessionToken: () => 'session' }))
vi.mock('@/composables/useSettings', () => ({ useSetting: () => ({ ref: ref(true), set: vi.fn() }) }))
vi.mock('@/composables/useNotifications', () => ({
  useNotifications: () => ({
    pushStatus: ref(mocks.status),
    refreshPushStatus: vi.fn(async () => mocks.status),
    registerPush: mocks.register,
    setPushEnabled: vi.fn(),
    requestPermission: vi.fn(),
  }),
}))

function status(overrides: Partial<PushStatus>): PushStatus {
  return { enabled: true, pendingCleanup: false, pendingRegistration: false, pausedByOrganisations: false,
    supported: true, registered: false, platform: 'fcm', cloudOrigin: 'https://cloud.example', lastError: null, ...overrides }
}

const RouterLink = { props: ['to'], template: '<a :href="to" data-testid="router-link"><slot /></a>' }
const render = () => mount(NotificationsPanel, {
  global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en } })], stubs: { RouterLink } },
})

beforeEach(() => {
  mocks.register.mockReset()
})

describe('NotificationsPanel push status', () => {
  it('explains that push is paused by the organisations switch and links to it', async () => {
    mocks.status = status({ pausedByOrganisations: true })
    const wrapper = render()
    await flushPromises()
    const paused = wrapper.find('[data-testid=push-paused-by-organisations]')
    expect(paused.exists()).toBe(true)
    expect(paused.text()).toContain(en.notifications.settings.pushPausedByOrganisations)
    expect(paused.find('a').attributes('href')).toBe('/settings/organisations')
    expect(paused.find('a').text()).toBe(en.notifications.settings.pushPausedOpenOrganisations)
    expect(wrapper.findAll('button').some((b) => b.text() === en.notifications.settings.register)).toBe(false)
  })

  it('says nothing about organisations while they are on', async () => {
    mocks.status = status({})
    const wrapper = render()
    await flushPromises()
    expect(wrapper.find('[data-testid=push-paused-by-organisations]').exists()).toBe(false)
    expect(wrapper.findAll('button').some((b) => b.text() === en.notifications.settings.register)).toBe(true)
  })
})
