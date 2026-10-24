import { beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import { ref } from 'vue'
import { createI18n } from 'vue-i18n'
import ChatConnectionForm from '../chat/ChatConnectionForm.vue'
import en from '@/locales/en'
import type { StudioConnection, StudioDocument } from '@/types'

const { startCode, finishCode, minor } = vi.hoisted(() => ({ startCode: vi.fn(), finishCode: vi.fn(), minor: { value: false } }))
vi.mock('@/composables/useProviderSignIn', async (original) => ({
  ...(await original<typeof import('@/composables/useProviderSignIn')>()),
  // A phone: no browser round trip, OpenRouter's paste-the-code flow instead.
  useProviderSignIn: () => ({
    available: false,
    codeAvailable: true,
    signingIn: ref(null),
    signIn: vi.fn(),
    startCode,
    finishCode,
    cancel: vi.fn(),
    openPage: vi.fn(),
    disconnect: vi.fn(),
  }),
}))
vi.mock('@/composables/useAccountStatus', () => ({ useAccountStatus: () => ({ isMinor: ref(minor.value) }) }))
vi.mock('@/composables/useLocalApi', () => ({ useLocalApi: () => ({ invoke: vi.fn() }) }))

const openrouter: StudioDocument<StudioConnection> = {
  id: 'openrouter-1',
  revision: 1,
  value: { id: 'openrouter-1', name: 'OpenRouter', endpoint: 'https://openrouter.ai/api/v1', model: 'openrouter/auto', location: 'cloud', capability: 'text', enabled: true, has_key: true },
}
const render = () => mount(ChatConnectionForm, { global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en } })] } })
const button = (w: ReturnType<typeof render>, text: string) => w.findAll('button').find((b) => b.text() === text)

beforeEach(() => {
  startCode.mockReset().mockResolvedValue(undefined)
  finishCode.mockReset()
  minor.value = false
})

describe('signing in to OpenRouter on a phone', () => {
  it('opens OpenRouter, then connects with the pasted code', async () => {
    finishCode.mockResolvedValue(openrouter)
    const w = render()
    expect(button(w, 'Continue with ChatGPT')).toBeUndefined()
    await button(w, 'Sign in with OpenRouter')!.trigger('click')
    await flushPromises()
    expect(startCode).toHaveBeenCalledWith('openrouter')
    expect(w.text()).toContain('Paste the code from OpenRouter')
    expect(button(w, 'Connect')!.attributes('disabled')).toBeDefined()
    await w.get('input[autocomplete="one-time-code"]').setValue('  abc-123  ')
    await button(w, 'Connect')!.trigger('click')
    await flushPromises()
    expect(finishCode).toHaveBeenCalledWith('  abc-123  ')
    expect(w.emitted('saved')).toEqual([[openrouter]])
  })

  it('explains a refused code and starts over when the sign-in expired', async () => {
    const w = render()
    await button(w, 'Sign in with OpenRouter')!.trigger('click')
    await flushPromises()
    await w.get('input[autocomplete="one-time-code"]').setValue('bad')
    finishCode.mockRejectedValue('code_rejected: the provider did not accept that code')
    await button(w, 'Connect')!.trigger('click')
    await flushPromises()
    expect(w.text()).toContain("OpenRouter didn't accept that code")
    expect(w.find('input[autocomplete="one-time-code"]').exists()).toBe(true)
    finishCode.mockRejectedValue('signin_expired: that sign-in expired')
    await button(w, 'Connect')!.trigger('click')
    await flushPromises()
    expect(w.text()).toContain('That sign-in expired')
    expect(w.find('input[autocomplete="one-time-code"]').exists()).toBe(false)
  })

  it('is not offered to minors, since OpenRouter is 18+', () => {
    minor.value = true
    const w = render()
    expect(button(w, 'Sign in with OpenRouter')).toBeUndefined()
    expect(w.text()).not.toContain(en.chat.connect.or)
  })
})
