import { flushPromises, mount } from '@vue/test-utils'
import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { CREDENTIALS_CHANGED } from '@/composables/credentialEvents'
import type { Element, PluginManifest } from '@/types'
import PluginHost from './PluginHost.vue'

const invoke = vi.hoisted(() => vi.fn<(command: string, args?: Record<string, unknown>) => Promise<unknown>>())
vi.mock('@/composables/useLocalApi', () => ({ useLocalApi: () => ({ invoke }) }))
vi.mock('vue-i18n', () => ({ useI18n: () => ({ t: (key: string) => key }) }))
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn() }))
vi.mock('@tauri-apps/plugin-fs', () => ({ readFile: vi.fn() }))

const { PluginIframe, resolveSubmit } = vi.hoisted(() => {
  const resolveSubmit = vi.fn()
  // A stand-in frame: the host drives it through the exposed port methods.
  const PluginIframe = {
    name: 'PluginIframe',
    emits: ['ready', 'request-capability', 'persist-state', 'emit-event', 'submit', 'complete', 'pick-files', 'error'],
    setup(_: unknown, { expose }: { expose: (api: Record<string, unknown>) => void }) {
      expose({ resolveSubmit, sendSubmitAck: vi.fn(), resolveCapabilityRequest: vi.fn(), resolveEvent: vi.fn(), resolvePickFiles: vi.fn(), sendCapabilityGranted: vi.fn(), sendCapabilityRevoked: vi.fn() })
      return () => null
    },
  }
  return { PluginIframe, resolveSubmit }
})
vi.mock('./PluginIframe.vue', () => ({ default: PluginIframe }))

const manifest = { name: 'Code runner', kinds: ['graded'], grader: 'grader.wasm', capabilities: [], entry: 'index.html', author_did: 'did:key:z' } as unknown as PluginManifest
const element = { id: 'el-1', plugin_cid: 'cid-1', content_inline: '{}' } as unknown as Element

function backend(grade: () => Promise<unknown>) {
  invoke.mockImplementation(async (command) => {
    if (command === 'plugin_list') return []
    if (command === 'plugin_get_manifest') return manifest
    if (command === 'plugin_list_permissions') return []
    if (command === 'plugin_load_element_state') return null
    if (command === 'plugin_submit_and_grade') return grade()
    return undefined
  })
}

const announced = vi.fn()
beforeEach(() => { invoke.mockReset(); resolveSubmit.mockReset(); window.addEventListener(CREDENTIALS_CHANGED, announced) })
afterEach(() => { window.removeEventListener(CREDENTIALS_CHANGED, announced); announced.mockReset() })

it.each([
  ['a graded submission announces credential changes', () => Promise.resolve({ version: '1', score: 0.9, details: null }), 1],
  ['a failed grade stays silent', () => Promise.reject(new Error('grader trapped')), 0],
])('%s', async (_, grade, times) => {
  backend(grade)
  const wrapper = mount(PluginHost, { props: { element, enrollmentId: 'enr-1' }, global: { mocks: { $t: (key: string) => key } } })
  await flushPromises()
  wrapper.getComponent(PluginIframe).vm.$emit('submit', 7, { answer: 1 }, null)
  await flushPromises()
  expect(invoke).toHaveBeenCalledWith('plugin_submit_and_grade', expect.objectContaining({ elementId: 'el-1', enrollmentId: 'enr-1' }))
  expect(resolveSubmit).toHaveBeenCalledTimes(1)
  expect(announced).toHaveBeenCalledTimes(times)
  wrapper.unmount()
})
