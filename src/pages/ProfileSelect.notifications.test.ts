import { beforeEach, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import { createI18n } from 'vue-i18n'

const mocks = vi.hoisted(() => ({
  currentRoute: { value: { fullPath: '/' } },
  invoke: vi.fn<(command: string) => Promise<unknown>>(),
  push: vi.fn().mockResolvedValue(undefined),
  replace: vi.fn().mockResolvedValue(undefined),
  biometric: vi.fn().mockResolvedValue(false),
  vaultPassword: vi.fn().mockResolvedValue('password'),
}))
vi.mock('@/composables/useLocalApi', () => ({ useLocalApi: () => ({ invoke: mocks.invoke }) }))
vi.mock('@/router', () => ({ default: { push: mocks.push, resolve: (path: string) => ({ matched: [{}], fullPath: path }), currentRoute: mocks.currentRoute, beforeResolve: () => () => {} } }))
vi.mock('vue-router', async () => ({ ...await vi.importActual<typeof import('vue-router')>('vue-router'), useRouter: () => ({ replace: mocks.replace }), RouterLink: { template: '<a><slot /></a>' } }))
vi.mock('@/composables/useBiometricVault', () => ({
  biometricSupported: mocks.biometric,
  biometricCredentialExists: () => Promise.resolve(true),
  getVaultPasswordViaBiometric: mocks.vaultPassword,
}))

beforeEach(() => {
  vi.resetModules()
  mocks.push.mockReset().mockImplementation(async (path: string) => { mocks.currentRoute.value.fullPath = path })
  mocks.replace.mockClear()
  mocks.biometric.mockResolvedValue(false)
  mocks.vaultPassword.mockReset().mockResolvedValue('password')
  mocks.invoke.mockImplementation(async command => {
    if (command === 'list_profiles') return [{ id: 'A', display_name: 'Learner', avatar: { kind: 'emoji', value: 'A' }, color: '#000', created_at: '' }]
    if (command === 'unlock_profile') return { wallet: {}, profile: {} }
    if (command === 'get_profile_session_token') return 'session-A'
    return null
  })
})

it.each(['password', 'biometric'])('preserves a held tap through the real picker %s unlock flow', async method => {
  mocks.biometric.mockResolvedValue(method === 'biometric')
  const links = (await import('@/deeplink/useDeepLinks')).useDeepLinks()
  await links.openNotification({ id: 'tap-A', profileId: 'A', networkId: 'network', recipient: 'binding', route: '/settings/organisations' })
  const Picker = (await import('./ProfileSelect.vue')).default
  const wrapper = mount(Picker, { global: {
    plugins: [createI18n({ legacy: false, locale: 'en', missingWarn: false, fallbackWarn: false })],
    stubs: { Starfield: true, Transition: true, AddProfileTile: true },
  } })
  await flushPromises()
  await wrapper.get('button[aria-pressed]').trigger('click')
  await flushPromises()
  if (method === 'password') {
    await wrapper.get('input[type=password]').setValue('password')
    await wrapper.get('form').trigger('submit')
    await flushPromises()
  }
  expect(mocks.push).toHaveBeenCalledExactlyOnceWith('/settings/organisations')
  expect(mocks.replace).not.toHaveBeenCalledWith('/home')
  wrapper.unmount()
})

it('does not unlock from a biometric result after the picker has been left', async () => {
  mocks.biometric.mockResolvedValue(true)
  let finish!: (password: string) => void
  mocks.vaultPassword.mockImplementation(() => new Promise(resolve => { finish = resolve }))
  const Picker = (await import('./ProfileSelect.vue')).default
  const wrapper = mount(Picker, { global: {
    plugins: [createI18n({ legacy: false, locale: 'en', missingWarn: false, fallbackWarn: false })],
    stubs: { Starfield: true, Transition: true, AddProfileTile: true },
  } })
  await flushPromises()
  await wrapper.get('button[aria-pressed]').trigger('click')
  await flushPromises()
  wrapper.unmount()
  finish('late-password')
  await flushPromises()
  expect(mocks.invoke.mock.calls.some(([command]) => command === 'unlock_profile')).toBe(false)
})
