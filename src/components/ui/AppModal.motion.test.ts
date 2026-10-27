import { mount, flushPromises } from '@vue/test-utils'
import { expect, test } from 'vitest'
import { i18n } from '@/i18n'
import AppModal from './AppModal.vue'

test('unmounting a protected surface removes its teleported dialog immediately', async () => {
  const wrapper = mount(AppModal, { props: { open: true, title: 'Private profile' }, slots: { default: 'Private learning data' }, global: { plugins: [i18n], stubs: { transition: false } }, attachTo: document.body })
  await flushPromises()
  expect(document.querySelector('[role="dialog"]')?.textContent).toContain('Private learning data')
  wrapper.unmount()
  expect(document.body.textContent).not.toContain('Private learning data')
})
