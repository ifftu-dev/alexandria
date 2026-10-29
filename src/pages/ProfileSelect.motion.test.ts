import { flushPromises, mount } from '@vue/test-utils'
import { createRouter, createMemoryHistory } from 'vue-router'
import { expect, test, vi } from 'vitest'
import { mockIPC } from '@tauri-apps/api/mocks'
import { i18n } from '@/i18n'
import ProfileSelect from './ProfileSelect.vue'

test('choosing a profile exposes the password form immediately with truthful pending feedback', async () => {
  vi.useFakeTimers()
  mockIPC((command) => {
    if (command === 'list_profiles') return [{ id: 'test-profile', display_name: 'Test learner', avatar: { kind: 'emoji', value: '🌱' }, color: '#6366f1', last_unlocked_at: null }]
    if (command === 'plugin:biometry|status') return { isAvailable: false }
    if (command === 'unlock_profile') return new Promise(() => {})
    return null
  })
  const router = createRouter({ history: createMemoryHistory(), routes: [{ path: '/profiles', component: ProfileSelect }] })
  await router.push('/profiles')
  const page = mount(ProfileSelect, { global: { plugins: [router, i18n] }, attachTo: document.body })
  await flushPromises()
  const tile = page.findAll('button').find(button => button.text().includes('Test learner'))
  expect(tile).toBeDefined()
  await tile!.trigger('click')
  const input = page.get('input[type="password"]')
  await input.setValue('test password long enough')
  await page.get('form').trigger('submit')
  await vi.advanceTimersByTimeAsync(3000)
  expect(page.text()).toContain('Unlocking your account')
  expect(page.text()).not.toContain('Getting your data ready')
  page.unmount()
  vi.useRealTimers()
})
