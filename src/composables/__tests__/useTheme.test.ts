import { mount } from '@vue/test-utils'
import { defineComponent, nextTick, ref } from 'vue'
import { beforeEach, expect, it, vi } from 'vitest'

const mocks = vi.hoisted(() => ({
  native: true,
  mobile: false,
  setTheme: vi.fn<(theme: 'light' | 'dark' | null) => Promise<void>>(),
}))
const themeSetting = ref('system')

vi.mock('@tauri-apps/api/core', () => ({ isTauri: () => mocks.native }))
vi.mock('@tauri-apps/api/window', () => ({
  getCurrentWindow: () => ({ setTheme: mocks.setTheme }),
}))
vi.mock('../usePlatform', () => ({ get isMobilePlatform() { return mocks.mobile } }))
vi.mock('../useSettings', () => ({
  useSetting: () => ({ ref: themeSetting, set: async (value: string) => { themeSetting.value = value } }),
  useSettings: () => ({ initialize: async () => {} }),
}))

beforeEach(() => {
  vi.resetModules()
  mocks.native = true
  mocks.mobile = false
  mocks.setTheme.mockReset().mockResolvedValue()
  themeSetting.value = 'system'
  localStorage.clear()
  document.documentElement.classList.remove('dark')
  vi.stubGlobal('matchMedia', vi.fn(() => ({
    matches: false,
    addEventListener: vi.fn(),
    removeEventListener: vi.fn(),
  })))
})

it('applies cached dark appearance to the page and native title bar before unlock', async () => {
  localStorage.setItem('alexandria-theme-pre-unlock', 'dark')
  const { initTheme } = await import('../useTheme')
  initTheme()
  expect(document.documentElement.classList.contains('dark')).toBe(true)
  expect(mocks.setTheme).toHaveBeenLastCalledWith('dark')
})

it('switches native appearance with profile settings and restores the system preference', async () => {
  const { useTheme, initThemeFromSettings } = await import('../useTheme')
  const wrapper = mount(defineComponent({ setup() { useTheme(); return () => null } }))
  themeSetting.value = 'dark'
  await initThemeFromSettings()
  await nextTick()
  expect(document.documentElement.classList.contains('dark')).toBe(true)
  expect(mocks.setTheme).toHaveBeenLastCalledWith('dark')
  themeSetting.value = 'light'
  await nextTick()
  expect(document.documentElement.classList.contains('dark')).toBe(false)
  expect(mocks.setTheme).toHaveBeenLastCalledWith('light')
  themeSetting.value = 'system'
  await nextTick()
  expect(mocks.setTheme).toHaveBeenLastCalledWith(null)
  wrapper.unmount()
})

it.each(['browser', 'mobile'])('does not call unsupported native theme APIs in %s', async (environment) => {
  mocks.native = environment !== 'browser'
  mocks.mobile = environment === 'mobile'
  localStorage.setItem('alexandria-theme-pre-unlock', 'dark')
  const { initTheme } = await import('../useTheme')
  initTheme()
  expect(document.documentElement.classList.contains('dark')).toBe(true)
  expect(mocks.setTheme).not.toHaveBeenCalled()
})

it('keeps the page usable if native theme synchronization fails', async () => {
  const warning = vi.spyOn(console, 'warn').mockImplementation(() => {})
  mocks.setTheme.mockRejectedValueOnce(new Error('unavailable'))
  localStorage.setItem('alexandria-theme-pre-unlock', 'dark')
  const { initTheme } = await import('../useTheme')
  initTheme()
  await nextTick()
  expect(document.documentElement.classList.contains('dark')).toBe(true)
  expect(warning).toHaveBeenCalledOnce()
  warning.mockRestore()
})
