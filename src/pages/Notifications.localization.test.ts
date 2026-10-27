import { afterEach, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import { createMemoryHistory, createRouter } from 'vue-router'
import { createI18n } from 'vue-i18n'
import Notifications from './Notifications.vue'
import { useNotifications } from '@/composables/useNotifications'
import en from '@/locales/en/notifications.json'
import fr from '@/locales/fr/notifications.json'

const ipc = vi.hoisted(() => ({ invoke: vi.fn() }))
vi.mock('@tauri-apps/api/core', () => ({ invoke: ipc.invoke }))
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(async () => () => {}) }))

afterEach(() => useNotifications().teardown())

it('renders a completed guardian link in French without displaying stored identifying copy', async () => {
  ipc.invoke.mockImplementation(async (command: string) => command === 'notifications_list' ? [{
    id: 'link', kind: 'guardian_request', title: 'Private name', body: 'Private organisation',
    route: null, source: 'local', createdAt: '2026-10-27T00:00:00Z', readAt: null, payload: null,
  }] : 1)
  const router = createRouter({ history: createMemoryHistory(), routes: [{ path: '/', component: Notifications }] })
  await router.push('/')
  const wrapper = mount(Notifications, { global: { plugins: [router, createI18n({
    legacy: false, locale: 'fr', fallbackLocale: 'en', messages: { en: { notifications: en }, fr: { notifications: fr } },
  })] } })
  await flushPromises()
  expect(wrapper.text()).toContain('Un responsable légal est lié à votre compte')
  expect(wrapper.text()).not.toContain('Private')
  wrapper.unmount()
})

it.each(['fr-CA', 'unknown'])('uses English fallback rather than stored payload copy when %s has no catalog', async (locale) => {
  ipc.invoke.mockImplementation(async (command: string) => command === 'notifications_list' ? [
    { id: 'offer', kind: 'offer_sent', title: 'Private name', body: 'Private organisation', route: null, source: 'push', createdAt: '', readAt: null, payload: null },
    { id: 'future', kind: 'future_kind', title: 'Private name', body: 'Private organisation', route: null, source: 'push', createdAt: '', readAt: null, payload: null },
  ] : 2)
  const router = createRouter({ history: createMemoryHistory(), routes: [{ path: '/', component: Notifications }] })
  await router.push('/')
  const wrapper = mount(Notifications, { global: { plugins: [router, createI18n({
    legacy: false, locale, fallbackLocale: 'en', missingWarn: false, fallbackWarn: false,
    messages: { en: { notifications: en } },
  })] } })
  await flushPromises()
  expect(wrapper.text()).toContain('New offer')
  expect(wrapper.text()).toContain('New activity')
  expect(wrapper.text()).not.toContain('Private')
  wrapper.unmount()
})
