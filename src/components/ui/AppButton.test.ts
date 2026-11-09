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

test('a pending button marks its spinner as progress so motion settings slow or still it rather than hide it', () => {
  const button = mount(AppButton, { props: { loading: true }, slots: { default: 'Create account' } })
  expect(button.find('[data-progress]').exists()).toBe(true)
  expect(button.get('[data-progress]').attributes('aria-hidden')).toBe('true')
  expect(button.text()).toBe('Create account')
})
