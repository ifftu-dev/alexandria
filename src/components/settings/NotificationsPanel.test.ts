import { flushPromises, mount } from '@vue/test-utils'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { ref } from 'vue'
import { createI18n } from 'vue-i18n'
import en from '@/locales/en'
import fr from '@/locales/fr'
import type { NotificationPermissionOutcome, PushStatus } from '@/types'
import NotificationsPanel from './NotificationsPanel.vue'

const mocks = vi.hoisted(() => ({ status: null as PushStatus | null, register: vi.fn(), requestPermission: vi.fn() }))
vi.mock('@/composables/usePlatform', () => ({ isMobilePlatform: true }))
vi.mock('@/composables/profileSession', () => ({ getProfileSessionToken: () => 'session' }))
vi.mock('@/composables/useSettings', () => ({ useSetting: () => ({ ref: ref(true), set: vi.fn() }) }))
vi.mock('@/composables/useNotifications', () => ({
  useNotifications: () => ({
    pushStatus: ref(mocks.status),
    refreshPushStatus: vi.fn(async () => mocks.status),
    registerPush: mocks.register,
    setPushEnabled: vi.fn(),
    requestPermission: mocks.requestPermission,
  }),
}))

function status(overrides: Partial<PushStatus>): PushStatus {
  return { enabled: true, pending_cleanup: false, pending_registration: false, paused_by_organisations: false,
    supported: true, registered: false, platform: 'fcm', cloud_origin: 'https://cloud.example', last_error_code: null, last_error_detail: null, ...overrides }
}

const RouterLink = { props: ['to'], template: '<a :href="to" data-testid="router-link"><slot /></a>' }
const render = (locale: 'en' | 'fr' = 'en') => mount(NotificationsPanel, {
  global: { plugins: [createI18n({ legacy: false, locale, messages: { en, fr } })], stubs: { RouterLink } },
})

beforeEach(() => {
  mocks.register.mockReset()
  mocks.requestPermission.mockReset()
})

describe('NotificationsPanel push status', () => {
  it('explains that push is paused by the organisations switch and links to it', async () => {
    mocks.status = status({ paused_by_organisations: true })
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

describe('NotificationsPanel push errors', () => {
  // Written out rather than read from the type, so a renamed code fails here.
  const messages = {
    permission_denied: 'Notifications are not allowed for Alexandria. Allow them in the device settings, then try again.',
    token_unavailable: 'This device could not get a push token. Check that it has a network connection and try again.',
    insecure_origin: 'The Alexandria Cloud address is not secure, so this device will not register with it.',
    cloud_unreachable: 'Alexandria Cloud could not be reached. Check your connection; Alexandria will try again.',
    cloud_rejected: 'Alexandria Cloud turned the request down. Alexandria will try again later.',
    cloud_unacknowledged: 'Alexandria Cloud did not confirm the change. Alexandria will try again.',
    invalid_response: 'Alexandria Cloud sent an answer this app does not understand. Alexandria will try again.',
  } as const

  it.each(Object.entries(messages))('shows the translated message for %s and never the raw detail', async (code, message) => {
    mocks.status = status({ last_error_code: code as keyof typeof messages, last_error_detail: 'Cloud push request returned 503' })
    const wrapper = render()
    await flushPromises()
    const error = wrapper.find('[data-testid=push-error]')
    expect(error.text()).toBe(message)
    expect(wrapper.text()).not.toContain('Cloud push request returned 503')
  })

  it('shows the message in the profile language', async () => {
    mocks.status = status({ last_error_code: 'cloud_unreachable', last_error_detail: 'Cloud push request failed' })
    const wrapper = render('fr')
    await flushPromises()
    expect(wrapper.find('[data-testid=push-error]').text()).toBe(fr.notifications.settings.pushErrors.cloud_unreachable)
    expect(fr.notifications.settings.pushErrors.cloud_unreachable).not.toBe(messages.cloud_unreachable)
  })

  it('shows no error line when the last attempt succeeded', async () => {
    mocks.status = status({})
    const wrapper = render()
    await flushPromises()
    expect(wrapper.find('[data-testid=push-error]').exists()).toBe(false)
  })
})

describe('NotificationsPanel permission request', () => {
  // Written out rather than read from the catalog, so a dropped case fails here.
  const outcomes: Record<NotificationPermissionOutcome, string> = {
    granted: 'The system allows banners.',
    denied: "The system is blocking banners. Allow Alexandria in the device's notification settings.",
    prompt_unavailable: "The system could not ask for permission. Allow Alexandria in the device's notification settings.",
  }

  it.each(Object.entries(outcomes))('says what %s means', async (outcome, message) => {
    mocks.status = status({})
    mocks.requestPermission.mockResolvedValue(outcome)
    const wrapper = render()
    await flushPromises()
    await wrapper.findAll('button').find((b) => b.text() === en.notifications.settings.permission)!.trigger('click')
    await flushPromises()
    expect(wrapper.find('[data-testid=permission-outcome]').text()).toBe(message)
  })
})
