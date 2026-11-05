import { mount } from '@vue/test-utils'
import { expect, test, vi } from 'vitest'
import { defineComponent, h, nextTick, ref } from 'vue'

// jsdom does not run CSS transitions, so this checks the contract the desktop
// UI motion specs rely on: one `motion-list` group on a positioned container
// that holds a leaving item in its place while the rest move.

test('a motion list renders its items in a positioned motion-list group and holds a leaving item in place', async () => {
  vi.resetModules()
  localStorage.clear()
  vi.stubGlobal('matchMedia', () => Object.assign(new EventTarget(), { matches: false }))
  const { default: MotionList } = await import('./MotionList.vue')
  const { useMotion } = await import('@/composables/useMotion')
  useMotion().setPreference('system')
  const items = ref(['algebra', 'geometry'])
  const wrapper = mount(defineComponent({
    setup: () => () => h(MotionList, { tag: 'ul', appear: true, class: 'grid gap-4', 'data-list': '' }, () =>
      items.value.map(item => h('li', { key: item, 'data-item': item }, item))),
  }), { attachTo: document.body, global: { stubs: { 'transition-group': false } } })

  const list = wrapper.get('[data-list]')
  expect(list.element.tagName).toBe('UL')
  expect(list.classes()).toEqual(expect.arrayContaining(['relative', 'grid', 'gap-4']))
  expect(list.findAll('[data-item]').map(item => item.text())).toEqual(['algebra', 'geometry'])

  items.value = ['geometry']
  await nextTick()
  const leaving = list.find<HTMLElement>('[data-item="algebra"]')
  // Still on screen while it fades, but out of flow so the rest can move.
  expect(leaving.exists()).toBe(true)
  expect(leaving.element.style.position).toBe('absolute')
  expect(leaving.classes()).toContain('motion-list-leave-active')
  wrapper.unmount()
  vi.unstubAllGlobals()
})

test('in Reduced and Off a leaving item is not held in place', async () => {
  vi.resetModules()
  localStorage.clear()
  vi.stubGlobal('matchMedia', () => Object.assign(new EventTarget(), { matches: false }))
  const { default: MotionList } = await import('./MotionList.vue')
  const { useMotion } = await import('@/composables/useMotion')
  useMotion().setPreference('off')
  const items = ref(['algebra', 'geometry'])
  const wrapper = mount(defineComponent({
    setup: () => () => h(MotionList, null, () => items.value.map(item => h('div', { key: item, 'data-item': item }, item))),
  }), { attachTo: document.body, global: { stubs: { 'transition-group': false } } })
  expect(wrapper.element.tagName).toBe('DIV')
  items.value = ['geometry']
  await nextTick()
  const leaving = wrapper.find<HTMLElement>('[data-item="algebra"]')
  expect(leaving.exists()).toBe(true)
  expect(leaving.element.style.position).toBe('')
  wrapper.unmount()
  vi.unstubAllGlobals()
})
