// Signing in to an AI provider in the browser instead of pasting a key.
//
// Only providers whose terms allow it for third-party apps are offered
// (ChatGPT's plan sign-in for open-source local apps, OpenRouter's PKCE
// sign-in). The backend runs the flow on a loopback listener and saves an
// ordinary connection; mobile builds offer API keys only.

import { ref } from 'vue'
import { useI18n } from 'vue-i18n'

import { useLocalApi } from './useLocalApi'
import { isMobilePlatform } from './usePlatform'
import type { StudioConnection, StudioDocument } from '@/types'

export type SignInProvider = 'chatgpt' | 'openrouter'
export type ProviderPage = 'chatgpt_usage' | 'openrouter_keys'

const signingIn = ref<SignInProvider | null>(null)

/** The stable code at the start of a backend sign-in error. */
export function signInErrorCode(error: unknown): string {
  const text = String(error instanceof Error ? error.message : error)
  const code = text.split(':')[0]?.trim() ?? ''
  return /^[a-z_0-9]+$/.test(code) ? code : 'signin_failed'
}

export function useProviderSignIn() {
  const { invoke } = useLocalApi()
  const { t } = useI18n()

  const available = !isMobilePlatform

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

  return { available, signingIn, signIn, cancel, disconnect, openPage }
}
