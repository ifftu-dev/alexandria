// Signing in to an AI provider in the browser instead of pasting a key.
//
// Only providers whose terms allow it for third-party apps are offered
// (ChatGPT's plan sign-in for open-source local apps, OpenRouter's PKCE
// sign-in). On desktop the backend runs the flow on a loopback listener.
// Phones cannot keep that listener alive behind the browser, so they offer
// OpenRouter's paste-the-code flow instead; ChatGPT allows only a loopback
// return and is desktop-only for now.

import { ref } from 'vue'
import { useI18n } from 'vue-i18n'

import { useLocalApi } from './useLocalApi'
import { isMobilePlatform } from './usePlatform'
import type { StudioConnection, StudioDocument } from '@/types'

export type SignInProvider = 'chatgpt' | 'openrouter'
export type ProviderPage = 'chatgpt_usage' | 'openrouter_keys'

const signingIn = ref<SignInProvider | null>(null)

export function useProviderSignIn() {
  const { invoke } = useLocalApi()
  const { t } = useI18n()

  /** Browser sign-in that returns to the app on its own (desktop). */
  const available = !isMobilePlatform
  /** Sign-in where the user pastes the provider's code back (phones). */
  const codeAvailable = isMobilePlatform

  async function signIn(provider: SignInProvider): Promise<StudioDocument<StudioConnection>> {
    signingIn.value = provider
    try {
      return await invoke<StudioDocument<StudioConnection>>('ai_signin', {
        provider,
        page: { title: t('chat.signin.donePageTitle'), body: t('chat.signin.donePageBody') },
      })
    } finally {
      signingIn.value = null
    }
  }

  /** Opens OpenRouter; it then shows a code for {@link finishCode}. */
  function startCode(provider: 'openrouter'): Promise<void> {
    return invoke('ai_signin_code_start', { provider })
  }

  function finishCode(code: string): Promise<StudioDocument<StudioConnection>> {
    return invoke<StudioDocument<StudioConnection>>('ai_signin_code_finish', { code })
  }

  async function cancel(): Promise<void> {
    await invoke('ai_signin_cancel')
  }

  /** Removes a connection; a ChatGPT sign-in is revoked with OpenAI first. */
  function disconnect(connectionId: string): Promise<{ revoked: boolean }> {
    return invoke<{ revoked: boolean }>('ai_disconnect', { connectionId })
  }

  function openPage(page: ProviderPage): Promise<void> {
    return invoke('ai_open_provider_page', { page })
  }

  return { available, codeAvailable, signingIn, signIn, startCode, finishCode, cancel, disconnect, openPage }
}
