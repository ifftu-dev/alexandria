import { mount } from '@vue/test-utils'
import { expect, test } from 'vitest'
import AppButton from './AppButton.vue'

test('pending action retains its name and content while preventing another submission', async () => {
  const button = mount(AppButton, { props: { loading: true }, slots: { default: 'Create account' } })
  expect(button.text()).toBe('Create account')
  expect(button.attributes('aria-busy')).toBe('true')
  expect(button.element.disabled).toBe(true)
  await button.trigger('click')
  expect(button.emitted('click')).toBeUndefined()
  await button.setProps({ loading: false })
  expect(button.element.disabled).toBe(false)
})
