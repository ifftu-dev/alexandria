import { beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import { ref } from 'vue'
import { createI18n } from 'vue-i18n'
import ChatConnectionForm from '../chat/ChatConnectionForm.vue'
import { signInErrorCode } from '@/composables/useProviderSignIn'
import en from '@/locales/en'
import type { StudioConnection, StudioDocument } from '@/types'

const { signIn, openPage, cancel, minor } = vi.hoisted(() => ({
  signIn: vi.fn(),
  openPage: vi.fn(),
  cancel: vi.fn(),
  minor: { value: false },
}))
vi.mock('@/composables/useProviderSignIn', async (original) => ({
  ...(await original<typeof import('@/composables/useProviderSignIn')>()),
  useProviderSignIn: () => ({ available: true, signingIn: ref(null), signIn, cancel, openPage, disconnect: vi.fn() }),
}))
vi.mock('@/composables/useAccountStatus', () => ({ useAccountStatus: () => ({ isMinor: ref(minor.value) }) }))
vi.mock('@/composables/useLocalApi', () => ({ useLocalApi: () => ({ invoke: vi.fn() }) }))

const chatgpt = (revision: number): StudioDocument<StudioConnection> => ({
  id: 'chatgpt-1',
  revision,
  value: { id: 'chatgpt-1', name: 'ChatGPT', endpoint: 'https://api.openai.com/v1', model: 'gpt-x', location: 'cloud', capability: 'text', enabled: true, has_key: true, protocol: 'responses', auth: 'chatgpt_plan' },
})
const render = () => mount(ChatConnectionForm, { global: { plugins: [createI18n({ legacy: false, locale: 'en', messages: { en } })] } })
const button = (w: ReturnType<typeof render>, text: string) => w.findAll('button').find((b) => b.text() === text)

beforeEach(() => {
  for (const f of [signIn, openPage, cancel]) f.mockReset()
  minor.value = false
})

describe('signing in to a provider instead of pasting a key', () => {
  it('shows the ChatGPT plan welcome once, then hands over the connection', async () => {
    signIn.mockResolvedValue(chatgpt(1))
    const w = render()
    await button(w, 'Continue with ChatGPT')!.trigger('click')
    await flushPromises()
    expect(signIn).toHaveBeenCalledWith('chatgpt')
    expect(w.text()).toContain("You're using your ChatGPT plan")
    expect(w.emitted('saved')).toBeUndefined()
    await button(w, 'Manage usage')!.trigger('click')
    expect(openPage).toHaveBeenCalledWith('chatgpt_usage')
    await button(w, 'Got it')!.trigger('click')
    expect(w.emitted('saved')).toEqual([[chatgpt(1)]])
  })

  it('skips the welcome when an existing ChatGPT connection signs in again', async () => {
    signIn.mockResolvedValue(chatgpt(3))
    const w = render()
    await button(w, 'Continue with ChatGPT')!.trigger('click')
    await flushPromises()
    expect(w.emitted('saved')).toEqual([[chatgpt(3)]])
  })

  it('explains refusals and stays quiet when the user cancels', async () => {
    const w = render()
    signIn.mockRejectedValue('plan_not_granted: ChatGPT plan use was not allowed')
    await button(w, 'Continue with ChatGPT')!.trigger('click')
    await flushPromises()
    expect(w.text()).toContain("Alexandria wasn't allowed to use your ChatGPT plan")
    signIn.mockRejectedValue('too_large: Model response exceeds the size limit')
    await button(w, 'Continue with ChatGPT')!.trigger('click')
    await flushPromises()
    expect(w.text()).toContain("Sign-in didn't finish (too_large). Try again.")
    signIn.mockRejectedValue('signin_cancelled: sign-in was cancelled')
    await button(w, 'Sign in with OpenRouter')!.trigger('click')
    await flushPromises()
    expect(w.find('[role="alert"]').exists()).toBe(false)
  })

  it('does not offer OpenRouter to minors, whose terms require 18+', () => {
    minor.value = true
    const w = render()
    expect(button(w, 'Continue with ChatGPT')).toBeDefined()
    expect(button(w, 'Sign in with OpenRouter')).toBeUndefined()
    expect(w.text()).toContain(en.chat.connection.guardianNote)
  })

  it('reads the stable code from backend errors', () => {
    expect(signInErrorCode('different_account: that is another account')).toBe('different_account')
    expect(signInErrorCode(new Error('under_13: no'))).toBe('under_13')
    expect(signInErrorCode('Something odd')).toBe('signin_failed')
  })
})
