import { describe, it, expect, vi } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'
import TickerText from './TickerText.vue'

describe('sidebar ticker isolation', () => {
  it('reveals the focused or hovered row’s full name and dismisses it with Escape', async () => {
    vi.stubGlobal('ResizeObserver', class { observe() {} disconnect() {} })
    const wrapper = mount({ components: { TickerText }, template: '<div><button><TickerText text="A long classroom title" /></button><button><TickerText text="Another long tutoring session" /></button></div>' }, { attachTo: document.body })
    await flushPromises()
    const buttons = wrapper.findAll('button')
    await buttons[0]!.trigger('mouseenter')
    expect(document.querySelector('[role="tooltip"]')?.textContent).toBe('A long classroom title')
    await buttons[0]!.trigger('mouseleave')
    await buttons[1]!.trigger('focus')
    expect(document.querySelector('[role="tooltip"]')?.textContent).toBe('Another long tutoring session')
    await buttons[1]!.trigger('keydown', { key: 'Escape' })
    expect(document.querySelector('[role="tooltip"]')).toBeNull()
    wrapper.unmount(); vi.unstubAllGlobals()
  })
})
