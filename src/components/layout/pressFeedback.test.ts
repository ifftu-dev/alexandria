import { flushPromises, mount } from '@vue/test-utils'
import { computed, ref } from 'vue'
import { createMemoryHistory, createRouter } from 'vue-router'
import { expect, test, vi } from 'vitest'
import { i18n } from '@/i18n'
import MobileTabBar from './MobileTabBar.vue'
import ModeSwitcher from './ModeSwitcher.vue'

// jsdom does not apply main.css, so these check the shared opt-in contract:
// the `pressable` class carries press feedback and the `selectable` class
// draws the selection indicator from the control's ARIA state. The desktop UI
// suite proves the cascade itself in Chromium and WebKit.

const mode = ref<'learner' | 'instructor'>('learner')
vi.mock('@/composables/useMode', () => ({
  useMode: () => ({ mode: computed(() => mode.value), canSwitchModes: computed(() => true), setMode: async (next: 'learner' | 'instructor') => { mode.value = next } }),
}))

async function routerAt(path: string) {
  const router = createRouter({ history: createMemoryHistory(), routes: [{ path: '/:pathMatch(.*)*', component: { render: () => null } }] })
  await router.push(path)
  return router
}

test('every tab bar item takes the shared press feedback and only the current one shows the selection indicator', async () => {
  const router = await routerAt('/skills/skill-sorting')
  const bar = mount(MobileTabBar, { global: { plugins: [i18n, router] } })
  const tabs = bar.findAll('button')
  expect(tabs).toHaveLength(4)
  for (const tab of tabs) expect(tab.classes()).toEqual(expect.arrayContaining(['pressable', 'selectable']))
  expect(tabs.map(tab => tab.attributes('aria-current') ?? null)).toEqual([null, null, null, 'page'])

  await tabs[0]!.trigger('click')
  await flushPromises()
  expect(tabs.map(tab => tab.attributes('aria-current') ?? null)).toEqual(['page', null, null, null])
})

test('the mode switcher segments take the shared press feedback and selection indicator', async () => {
  const router = await routerAt('/home')
  const switcher = mount(ModeSwitcher, { global: { plugins: [i18n, router] } })
  const radios = switcher.findAll('[role="radio"]')
  for (const radio of radios) expect(radio.classes()).toEqual(expect.arrayContaining(['pressable', 'selectable']))
  expect(radios.map(radio => radio.attributes('aria-checked'))).toEqual(['true', 'false'])

  await radios[1]!.trigger('click')
  await flushPromises()
  expect(radios.map(radio => radio.attributes('aria-checked'))).toEqual(['false', 'true'])
})
