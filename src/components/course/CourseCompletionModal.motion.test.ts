import { mount, flushPromises } from '@vue/test-utils'
import { createRouter, createMemoryHistory } from 'vue-router'
import { mockIPC, clearMocks } from '@tauri-apps/api/mocks'
import { afterEach, expect, test } from 'vitest'
import { i18n } from '@/i18n'
import { useProfiles } from '@/composables/useProfiles'
import { useCourseCompletion } from '@/composables/useCourseCompletion'
import CourseCompletionModal from './CourseCompletionModal.vue'

afterEach(() => { useCourseCompletion().close(); clearMocks(); document.body.innerHTML = '' })
test('course celebration receives dialog focus and Escape dismisses it without waiting for effects', async () => {
  mockIPC(command => {
    if (command === 'unlock_profile') return { profile: { display_name: 'Learner', stake_address: 'test' }, wallet: { stake_address: 'test' } }
    if (command === 'get_profile_session_token') return 'test-session'
    if (command === 'list_profiles') return [{ id: 'test', display_name: 'Learner' }]
    return null
  })
  await useProfiles().unlockProfile('test', 'test-only-password')
  const router = createRouter({ history: createMemoryHistory(), routes: [{ path: '/', component: { template: '<div />' } }] })
  const modal = mount(CourseCompletionModal, { global: { plugins: [i18n, router] }, attachTo: document.body })
  await useCourseCompletion().open({ courseTitle: 'Completed learning', courseId: 'course', skillIds: [], credentialIds: [], txHash: null })
  await flushPromises()
  const dialog = document.querySelector('[role="dialog"]')
  expect(dialog).not.toBeNull()
  expect(dialog?.contains(document.activeElement)).toBe(true)
  document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }))
  await flushPromises()
  expect(useCourseCompletion().isOpen.value).toBe(false)
  modal.unmount()
})
