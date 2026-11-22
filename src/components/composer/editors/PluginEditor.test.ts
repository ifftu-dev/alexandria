import { flushPromises, mount } from '@vue/test-utils'
import { expect, it, vi } from 'vitest'
import { defineComponent } from 'vue'
import type { Element } from '@/types'
import PluginEditor from './PluginEditor.vue'

const invoke = vi.hoisted(() => vi.fn())
vi.mock('@/composables/useLocalApi', () => ({ useLocalApi: () => ({ invoke }) }))
vi.mock('vue-i18n', () => ({ useI18n: () => ({ t: (key: string) => key }) }))

it('stages a coach recording in the draft until the instructor saves', async () => {
  const element = { id: 'lesson', plugin_cid: null, content_inline: null } as Element
  const plugin = { plugin_cid: 'workout', enabled: true, version: '0.1.0', manifest_json: '{"authoring":true}' }
  invoke.mockImplementation(async (cmd: string) => cmd === 'plugin_list' ? [plugin] : element)
  const PluginHost = defineComponent({ emits: ['author-config'], template: '<div />' })
  const wrapper = mount(PluginEditor, {
    props: { element },
    global: {
      mocks: { $t: (key: string) => key },
      stubs: { PluginHost, AppButton: { template: '<button><slot /></button>' }, AppBadge: true },
    },
  })
  await flushPromises()
  await wrapper.get('select').setValue('workout')
  const editor = wrapper.getComponent(PluginHost)
  expect(editor.attributes('mode')).toBe('author')
  const config = JSON.stringify({ duration: 3000, frames: [], clip: {type:'video/webm', data:'example'} })
  editor.vm.$emit('author-config', config)
  expect(invoke).not.toHaveBeenCalledWith('update_element', expect.anything())
  await wrapper.get('button').trigger('click')
  await flushPromises()
  expect(invoke).toHaveBeenCalledWith('update_element', {
    elementId: 'lesson', req: { plugin_cid: 'workout', plugin_version: '0.1.0', content_inline: config },
  })
  wrapper.unmount()
})
