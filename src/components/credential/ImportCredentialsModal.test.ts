import { flushPromises, mount } from '@vue/test-utils'
import { afterEach, expect, it, vi } from 'vitest'
import { createI18n } from 'vue-i18n'
import en from '@/locales/en'
import { CREDENTIALS_CHANGED } from '@/composables/credentialEvents'
import ImportCredentialsModal from './ImportCredentialsModal.vue'

const invoke = vi.hoisted(() => vi.fn<(command: string, args?: Record<string, unknown>) => Promise<unknown>>())
vi.mock('@/composables/useLocalApi', () => ({ useLocalApi: () => ({ invoke }) }))
vi.mock('@/components/ui', async () => ({
  AppButton: (await import('@/components/ui/AppButton.vue')).default,
  AppModal: { template: '<div><slot /></div>' },
  AppTextarea: { props: ['modelValue'], emits: ['update:modelValue'], template: '<textarea :value="modelValue" @input="$emit(\'update:modelValue\', $event.target.value)" />' },
}))

const announced = vi.fn()
afterEach(() => { window.removeEventListener(CREDENTIALS_CHANGED, announced); announced.mockReset() })

it.each([
  [{ imported: 2, alreadyPresent: 0, failed: [] }, 1],
  [{ imported: 0, alreadyPresent: 1, failed: [] }, 0],
])('announces credential changes only when an import stores credentials', async (summary, times) => {
  invoke.mockReset().mockResolvedValue(summary)
  window.addEventListener(CREDENTIALS_CHANGED, announced)
  const wrapper = mount(ImportCredentialsModal, {
    props: { open: true },
    global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en } })] },
  })
  await wrapper.get('textarea').setValue('{"credentials": []}')
  await wrapper.get('button').trigger('click')
  await flushPromises()
  expect(invoke).toHaveBeenCalledWith('import_credentials', { payload: '{"credentials": []}' })
  expect(announced).toHaveBeenCalledTimes(times)
  wrapper.unmount()
})
