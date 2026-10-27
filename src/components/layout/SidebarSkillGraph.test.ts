import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { createI18n } from 'vue-i18n'
import { createMemoryHistory, createRouter } from 'vue-router'
import en from '@/locales/en'
import { atlasBackend } from '../../../tests/desktop-ui/atlasBackend'

const ipc = vi.hoisted(() => vi.fn())
vi.mock('@tauri-apps/api/core', () => ({ invoke: ipc, transformCallback: () => 1 }))
let wrapper: VueWrapper | undefined
let release: (() => void) | undefined
beforeEach(() => { vi.resetModules(); ipc.mockReset() })
afterEach(() => { wrapper?.unmount(); release?.(); vi.useRealTimers() })

it('shows fields and current earned levels; refreshes on evidence and navigation without duplicate subscriptions', async () => {
  const backend = atlasBackend()
  ipc.mockImplementation(backend.invoke)
  const { useProfiles } = await import('@/composables/useProfiles')
  const { installSkillAtlasRefresh } = await import('@/composables/useSkillAtlas')
  const { announceCredentialsChanged } = await import('@/composables/credentialEvents')
  const { default: Sidebar } = await import('./SidebarSkillGraph.vue')
  const router = createRouter({ history: createMemoryHistory(), routes: [{ path: '/:pathMatch(.*)*', component: {} }] })
  await router.push('/skills')
  await useProfiles().initialize()
  release = installSkillAtlasRefresh(router)
  wrapper = mount(Sidebar, { global: { plugins: [router, createI18n({ legacy: false, locale: 'en', messages: { en } })] } })
  await flushPromises()
  expect(wrapper.text()).toContain('Computer Science')
  expect(wrapper.text()).toContain('Big-O Analysis')
  expect(wrapper.text()).toContain('2 Understand')
  expect(wrapper.text()).toContain('Ready')
  expect(wrapper.text()).not.toContain('Locked')
  const before = backend.state.reads
  backend.state.level = 4
  announceCredentialsChanged()
  await flushPromises()
  expect(wrapper.text()).toContain('5 Evaluate')
  expect(backend.state.reads).toBe(before + 1)
  vi.useFakeTimers()
  await vi.advanceTimersByTimeAsync(1600)
  backend.state.earned = false
  await router.push('/other')
  await flushPromises()
  expect(wrapper.text()).not.toContain('5 Evaluate')
  const after = backend.state.reads
  await router.push('/again')
  expect(backend.state.reads).toBe(after)
  await useProfiles().lockProfile()
  await flushPromises()
  expect(wrapper.text()).not.toContain('Big-O Analysis')
  expect(backend.state.unexpected).toEqual([])
})
