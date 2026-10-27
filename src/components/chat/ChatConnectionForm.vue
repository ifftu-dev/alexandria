<script setup lang="ts">
import { computed, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { AppButton } from '@/components/ui'
import { useLocalApi } from '@/composables/useLocalApi'
import { useAccountStatus } from '@/composables/useAccountStatus'
import { chatErrorCode } from '@/composables/useChat'
import type { StudioConnection, StudioDocument } from '@/types'
import { restrictsAppsForMinors } from '@/utils/providerTerms'
import { signInErrorCode, useProviderSignIn, type SignInProvider } from '@/composables/useProviderSignIn'

/**
 * Connecting chat to a model, one decision per screen: sign in with an
 * account, or set one up with an API key or a model the user runs. Each
 * choice opens its own step; rarely needed fields sit under "Advanced".
 */
const props = defineProps<{ canCancel?: boolean }>()
const emit = defineEmits<{ saved: [connection: StudioDocument<StudioConnection>]; cancel: [] }>()
const { t } = useI18n()
const { invoke } = useLocalApi()
const { isMinor } = useAccountStatus()

type Step = 'choose' | 'key' | 'local' | 'code' | 'welcome'
const step = ref<Step>('choose')

type Where = 'local' | 'private_network'
type Protocol = 'compatible' | 'anthropic'
const localDefaults: Record<Where, string> = {
  local: 'http://localhost:11434/v1',
  private_network: 'http://192.168.1.10:1234/v1',
}
const cloudDefaults: Record<Protocol, string> = {
  compatible: 'https://api.openai.com/v1',
  anthropic: 'https://api.anthropic.com/v1',
}

const name = ref('')
const where = ref<Where>('local')
const protocol = ref<Protocol>('anthropic')
const endpoint = ref(cloudDefaults.anthropic)
const model = ref('')
const apiKey = ref('')
const contextTokens = ref('')
const saving = ref(false)
const message = ref('')

const signIn = useProviderSignIn()
const signInError = ref('')
/** A ChatGPT connection just made: the welcome OpenAI asks apps to show. */
const planWelcome = ref<StudioDocument<StudioConnection> | null>(null)
const signInMessages: Record<string, string> = {
  signin_timeout: 'chat.signin.errors.timeout',
  signin_declined: 'chat.signin.errors.declined',
  plan_not_granted: 'chat.signin.errors.planNotGranted',
  different_account: 'chat.signin.errors.differentAccount',
  signin_busy: 'chat.signin.errors.busy',
  unreachable: 'chat.signin.errors.unreachable',
  under_13: 'chat.errors.under_13',
  adults_only: 'chat.errors.adults_only',
  model_unavailable: 'chat.signin.errors.noModels',
  code_rejected: 'chat.signin.errors.codeRejected',
  signin_expired: 'chat.signin.errors.expired',
}

/** Account sign-in is offered: in the browser on desktop, by pasted code on phones (adults only). */
const offersSignIn = computed(() => signIn.available || (signIn.codeAvailable && !isMinor.value))

const pastedCode = ref('')
const connecting = ref(false)

function showSignInError(e: unknown) {
  const code = signInErrorCode(e)
  if (code === 'signin_cancelled') return
  const known = signInMessages[code]
  signInError.value = known ? t(known) : t('chat.signin.errors.failedCode', { code })
}

function go(next: Step) {
  message.value = ''
  signInError.value = ''
  if (next === 'key') {
    protocol.value = 'anthropic'
    endpoint.value = cloudDefaults.anthropic
  } else if (next === 'local') {
    where.value = 'local'
    protocol.value = 'compatible'
    endpoint.value = localDefaults.local
  }
  step.value = next
}

async function startCodeSignIn() {
  signInError.value = ''
  pastedCode.value = ''
  try {
    await signIn.startCode('openrouter')
    step.value = 'code'
  } catch (e) {
    showSignInError(e)
  }
}

async function finishCodeSignIn() {
  if (!pastedCode.value.trim()) return
  signInError.value = ''
  connecting.value = true
  try {
    const saved = await signIn.finishCode(pastedCode.value)
    pastedCode.value = ''
    emit('saved', saved)
  } catch (e) {
    showSignInError(e)
    if (signInErrorCode(e) === 'signin_expired') step.value = 'choose'
  } finally {
    connecting.value = false
  }
}

async function startSignIn(provider: SignInProvider) {
  signInError.value = ''
  try {
    const saved = await signIn.signIn(provider)
    if (provider === 'chatgpt' && saved.revision === 1) {
      planWelcome.value = saved
      step.value = 'welcome'
    } else emit('saved', saved)
  } catch (e) {
    showSignInError(e)
  }
}

function chooseWhere(value: Where) {
  if (endpoint.value === localDefaults[where.value]) endpoint.value = localDefaults[value]
  where.value = value
}

function chooseProtocol(value: Protocol) {
  if (endpoint.value === cloudDefaults[protocol.value]) endpoint.value = cloudDefaults[value]
  protocol.value = value
}

/** A readable default name from the endpoint, used when the field is left empty. */
function defaultName(): string {
  let host = ''
  try {
    host = new URL(endpoint.value.trim()).host
  } catch {
    /* falls back below */
  }
  const known: Record<string, string> = {
    'api.anthropic.com': 'Anthropic',
    'api.openai.com': 'OpenAI',
    'generativelanguage.googleapis.com': 'Gemini',
    'openrouter.ai': 'OpenRouter',
    'api.mistral.ai': 'Mistral',
    'api.groq.com': 'Groq',
    'api.x.ai': 'xAI',
  }
  if (known[host]) return known[host]!
  if (host.endsWith(':11434')) return 'Ollama'
  if (host.endsWith(':1234')) return 'LM Studio'
  return host || t('chat.connect.unnamed')
}

const isCloud = computed(() => step.value === 'key')

async function save() {
  saving.value = true
  message.value = ''
  const id = crypto.randomUUID()
  const window = Number.parseInt(contextTokens.value, 10)
  try {
    const saved = await invoke<StudioDocument<StudioConnection>>('studio_save_connection', {
      document: {
        id,
        revision: 0,
        value: {
          id,
          name: name.value.trim() || defaultName(),
          endpoint: endpoint.value.trim(),
          model: model.value.trim(),
          location: isCloud.value ? 'cloud' : where.value,
          capability: 'text',
          enabled: true,
          has_key: false,
          context_tokens: Number.isFinite(window) && window > 0 ? window : null,
          ...(protocol.value === 'anthropic' ? { protocol: 'anthropic' as const } : {}),
        },
      },
      apiKey: apiKey.value || null,
    })
    apiKey.value = ''
    emit('saved', saved)
  } catch (e) {
    // Validation messages from the backend are written for people.
    const text = String(e)
    message.value = chatErrorCode(e) === 'invalid_input' ? text.slice(text.indexOf(':') + 1).trim() : t('chat.errors.generic')
  } finally {
    saving.value = false
  }
}
</script>

<template>
  <!-- ChatGPT plan welcome, shown once (OpenAI's UI guidelines) -->
  <div v-if="step === 'welcome' && planWelcome" class="connect" role="dialog" aria-labelledby="chat-plan-welcome">
    <div class="connect-hero">
      <h3 id="chat-plan-welcome">{{ t('chat.signin.welcomeTitle') }}</h3>
      <p>{{ t('chat.signin.welcomeBody') }}</p>
    </div>
    <div class="grid gap-2">
      <button type="button" class="connect-btn connect-btn--primary" @click="emit('saved', planWelcome!)">{{ t('chat.signin.gotIt') }}</button>
      <button type="button" class="connect-link" @click="signIn.openPage('chatgpt_usage')">{{ t('chat.signin.manageUsage') }}</button>
    </div>
  </div>

  <!-- Step 1: choose how -->
  <div v-else-if="step === 'choose'" class="connect">
    <div class="connect-hero">
      <h3>{{ t('chat.connect.title') }}</h3>
      <p>{{ t('chat.connect.intro') }}</p>
    </div>

    <div v-if="offersSignIn" class="grid gap-2.5">
      <template v-if="signIn.signingIn.value">
        <p class="connect-status" role="status"><span class="connect-spin" aria-hidden="true" />{{ t('chat.signin.waiting') }}</p>
        <button type="button" class="connect-link" @click="signIn.cancel()">{{ t('chat.signin.cancel') }}</button>
      </template>
      <template v-else>
        <template v-if="signIn.available">
          <button type="button" class="connect-btn bg-black text-white hover:bg-neutral-800 dark:bg-white dark:text-black dark:hover:bg-neutral-200" @click="startSignIn('chatgpt')">{{ t('chat.signin.chatgpt') }}</button>
          <p class="connect-hint">{{ t('chat.signin.chatgptHint') }}</p>
          <p v-if="isMinor" class="connect-hint text-warning">{{ t('chat.connection.guardianNote') }}</p>
        </template>
        <template v-if="!isMinor">
          <button type="button" class="connect-btn connect-btn--outline" @click="signIn.available ? startSignIn('openrouter') : startCodeSignIn()">{{ t('chat.signin.openrouter') }}</button>
          <p class="connect-hint">{{ t('chat.signin.openrouterHint') }}</p>
        </template>
      </template>
      <p v-if="signInError" class="text-center text-[13px] text-error" role="alert">{{ signInError }}</p>
    </div>

    <p v-if="offersSignIn" class="connect-sep">{{ t('chat.connect.or') }}</p>

    <div>
      <button type="button" class="connect-opt" @click="go('key')">
        <span class="connect-opt-ic"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><circle cx="8" cy="15" r="4" /><path d="M11 12l9-9M17 6l3 3" /></svg></span>
        <span class="min-w-0 flex-1"><b>{{ t('chat.connect.keyTitle') }}</b><small>{{ t('chat.connect.keyHint') }}</small></span>
        <svg class="connect-go" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M9 6l6 6-6 6" /></svg>
      </button>
      <button type="button" class="connect-opt" @click="go('local')">
        <span class="connect-opt-ic"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="4" width="18" height="12" rx="2" /><path d="M8 20h8M12 16v4" /></svg></span>
        <span class="min-w-0 flex-1"><b>{{ t('chat.connect.localTitle') }}</b><small>{{ t('chat.connect.localHint') }}</small></span>
        <svg class="connect-go" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M9 6l6 6-6 6" /></svg>
      </button>
    </div>

    <p class="connect-foot">
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect x="5" y="11" width="14" height="9" rx="2" /><path d="M8 11V8a4 4 0 0 1 8 0v3" /></svg>
      {{ t('chat.connect.private') }}
    </p>
    <button v-if="props.canCancel" type="button" class="connect-link" @click="emit('cancel')">{{ t('chat.connection.cancel') }}</button>
  </div>

  <!-- Phones: paste OpenRouter's code -->
  <div v-else-if="step === 'code'" class="connect">
    <button type="button" class="connect-back" @click="go('choose')">
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M15 18l-6-6 6-6" /></svg>{{ t('chat.connect.back') }}
    </button>
    <div class="connect-steps" aria-hidden="true"><i /><i /><span>{{ t('chat.connect.step', { n: 2, total: 2 }) }}</span></div>
    <div class="connect-hero !pt-0">
      <h3>{{ t('chat.connect.codeTitle') }}</h3>
      <p>{{ t('chat.connect.codeSub') }}</p>
    </div>
    <ol class="connect-list">
      <li>{{ t('chat.connect.codeStep1') }}</li>
      <li>{{ t('chat.connect.codeStep2') }}</li>
    </ol>
    <label class="block">
      <span class="label">{{ t('chat.signin.codeLabel') }}</span>
      <input v-model="pastedCode" class="input connect-code" autocomplete="one-time-code" autocapitalize="off" spellcheck="false" maxlength="512" @keydown.enter.prevent="finishCodeSignIn">
    </label>
    <AppButton type="button" class="w-full" :loading="connecting" :disabled="!pastedCode.trim()" @click="finishCodeSignIn">{{ t('chat.signin.connect') }}</AppButton>
    <p v-if="signInError" class="text-center text-[13px] text-error" role="alert">{{ signInError }}</p>
    <p class="connect-hint">
      {{ t('chat.connect.codeHint') }}
      <button type="button" class="text-primary hover:underline" @click="startCodeSignIn">{{ t('chat.connect.reopen') }}</button>
    </p>
  </div>

  <!-- API key, or a model the user runs -->
  <form v-else class="connect" @submit.prevent="save">
    <button type="button" class="connect-back" @click="go('choose')">
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M15 18l-6-6 6-6" /></svg>{{ t('chat.connect.back') }}
    </button>
    <div class="connect-hero !pt-0">
      <h3>{{ isCloud ? t('chat.connect.keyTitle') : t('chat.connect.localTitle') }}</h3>
      <p>{{ isCloud ? t('chat.connect.keySub') : t('chat.connect.localSub') }}</p>
    </div>

    <div v-if="isCloud" class="connect-seg" role="radiogroup" :aria-label="t('chat.connection.protocol')">
      <button
        v-for="option in (['anthropic', 'compatible'] as const)"
        :key="option"
        type="button"
        role="radio"
        :aria-checked="protocol === option"
        :class="{ on: protocol === option }"
        @click="chooseProtocol(option)"
      >{{ t(`chat.connect.providers.${option}`) }}</button>
    </div>
    <div v-else class="connect-seg" role="radiogroup" :aria-label="t('chat.connection.where')">
      <button
        v-for="option in (['local', 'private_network'] as const)"
        :key="option"
        type="button"
        role="radio"
        :aria-checked="where === option"
        :class="{ on: where === option }"
        @click="chooseWhere(option)"
      >{{ option === 'local' ? t('chat.location.device') : t('chat.location.private_network') }}</button>
    </div>
    <p v-if="isCloud && isMinor" class="connect-hint text-start text-warning">{{ t('chat.connection.guardianNote') }}</p>

    <label v-if="isCloud" class="block">
      <span class="label">{{ t('chat.connection.apiKey') }}</span>
      <input v-model="apiKey" class="input" type="password" autocomplete="off" required>
    </label>
    <label v-else class="block">
      <span class="label">{{ t('chat.connection.endpoint') }}</span>
      <input v-model="endpoint" class="input" required inputmode="url" spellcheck="false">
    </label>
    <label class="block">
      <span class="label">{{ t('chat.connection.model') }}</span>
      <input v-model="model" class="input" required maxlength="200" spellcheck="false" :placeholder="protocol === 'anthropic' ? t('chat.connection.modelPlaceholderAnthropic') : t('chat.connection.modelPlaceholder')">
    </label>

    <details class="connect-adv">
      <summary>{{ t('chat.connect.advanced') }}</summary>
      <div class="grid gap-3 pt-3">
        <label class="block">
          <span class="label">{{ t('chat.connection.name') }}</span>
          <input v-model="name" class="input" maxlength="200" :placeholder="defaultName()">
        </label>
        <label v-if="isCloud" class="block">
          <span class="label">{{ t('chat.connection.endpoint') }}</span>
          <input v-model="endpoint" class="input" required inputmode="url" spellcheck="false">
        </label>
        <template v-else>
          <label class="block">
            <span class="label">{{ t('chat.connection.apiKeyOptional') }}</span>
            <input v-model="apiKey" class="input" type="password" autocomplete="off">
          </label>
          <div class="connect-seg" role="radiogroup" :aria-label="t('chat.connection.protocol')">
            <button
              v-for="option in (['compatible', 'anthropic'] as const)"
              :key="option"
              type="button"
              role="radio"
              :aria-checked="protocol === option"
              :class="{ on: protocol === option }"
              @click="protocol = option"
            >{{ t(`chat.connection.protocols.${option}`) }}</button>
          </div>
        </template>
        <label class="block">
          <span class="label">{{ t('chat.connection.contextTokens') }}</span>
          <input v-model="contextTokens" class="input" inputmode="numeric" pattern="[0-9]*">
          <span class="mt-1 block text-xs text-muted-foreground">{{ t('chat.connection.contextHint') }}</span>
        </label>
      </div>
    </details>
    <p v-if="restrictsAppsForMinors(endpoint)" class="text-xs text-warning">{{ t('common.aiTerms.geminiNotice') }}</p>

    <p v-if="message" class="text-[13px] text-error" role="alert">{{ message }}</p>
    <AppButton type="submit" class="w-full" :loading="saving">{{ t('chat.connection.save') }}</AppButton>
  </form>
</template>

<style scoped>
.connect { display: grid; gap: 1rem; font-size: 0.875rem; }
.connect-hero { padding-top: 1rem; }
.connect-hero h3 { font-size: 1.1875rem; line-height: 1.3; font-weight: 600; letter-spacing: -0.01em; margin: 0 0 0.375rem; }
.connect-hero p { margin: 0; color: var(--app-muted-foreground); line-height: 1.5; }
.connect-btn { display: flex; width: 100%; align-items: center; justify-content: center; gap: 0.5rem; min-height: 2.875rem; border-radius: 999px; padding: 0 1rem; font-size: 0.90625rem; font-weight: 600; }
.connect-btn--outline { border: 1px solid var(--app-border); color: var(--app-foreground); }
.connect-btn--outline:hover { background: var(--app-muted); }
.connect-btn--primary { background: var(--app-primary); color: var(--app-primary-foreground); }
.connect-btn--primary:hover { background: var(--app-primary-hover); }
.connect-hint { margin: -0.25rem 0 0; text-align: center; font-size: 0.78125rem; color: var(--app-muted-foreground); }
.connect-link { justify-self: center; font-size: 0.8125rem; color: var(--app-muted-foreground); }
.connect-link:hover { color: var(--app-foreground); text-decoration: underline; }
.connect-status { display: flex; align-items: center; justify-content: center; gap: 0.5rem; min-height: 2.875rem; margin: 0; border-radius: 999px; background: var(--app-muted); font-size: 0.84375rem; }
.connect-spin { width: 0.75rem; height: 0.75rem; border-radius: 999px; border: 2px solid var(--app-border); border-top-color: var(--app-primary); animation: connect-spin 0.8s linear infinite; }
@keyframes connect-spin { to { transform: rotate(360deg); } }
@media (prefers-reduced-motion: reduce) { .connect-spin { animation: none; } }
.connect-sep { display: flex; align-items: center; gap: 0.625rem; margin: 0.25rem 0 -0.25rem; font-size: 0.78125rem; color: var(--app-muted-foreground); }
.connect-sep::before, .connect-sep::after { content: ""; height: 1px; flex: 1; background: var(--app-border); opacity: 0.6; }
.connect-opt { display: flex; width: 100%; align-items: center; gap: 0.75rem; padding: 0.75rem 0.25rem; text-align: start; border-bottom: 1px solid color-mix(in srgb, var(--app-border) 60%, transparent); }
.connect-opt:last-child { border-bottom: 0; }
.connect-opt:hover b { color: var(--app-primary); }
.connect-opt b { display: block; font-weight: 500; }
.connect-opt small { display: block; font-size: 0.78125rem; color: var(--app-muted-foreground); }
.connect-opt-ic { display: grid; place-items: center; width: 2.125rem; height: 2.125rem; flex: none; border-radius: 0.625rem; background: var(--app-muted); color: var(--app-muted-foreground); }
.connect-opt-ic svg { width: 1.0625rem; height: 1.0625rem; }
.connect-go { width: 1rem; height: 1rem; flex: none; color: var(--app-muted-foreground); }
.connect-foot { display: flex; align-items: center; gap: 0.375rem; margin: 0; font-size: 0.75rem; color: var(--app-muted-foreground); }
.connect-foot svg { width: 0.8125rem; height: 0.8125rem; flex: none; }
.connect-back { display: inline-flex; align-items: center; gap: 0.25rem; justify-self: start; margin-inline-start: -0.375rem; padding: 0.25rem 0.375rem; border-radius: 0.5rem; font-size: 0.8125rem; color: var(--app-muted-foreground); }
.connect-back:hover { background: var(--app-muted); color: var(--app-foreground); }
.connect-back svg { width: 1rem; height: 1rem; }
.connect-steps { display: flex; align-items: center; gap: 0.5rem; font-size: 0.78125rem; color: var(--app-muted-foreground); }
.connect-steps i { flex: 1; height: 3px; border-radius: 999px; background: var(--app-primary); }
.connect-list { margin: 0; padding-inline-start: 1.25rem; display: grid; gap: 0.375rem; color: var(--app-muted-foreground); list-style: decimal; }
.connect-code { font-family: var(--font-mono); height: 3.25rem; }
.connect-seg { display: flex; padding: 0.1875rem; border-radius: 0.75rem; background: var(--app-muted); }
.connect-seg button { flex: 1; min-height: 2.25rem; border-radius: 0.5625rem; font-size: 0.8125rem; color: var(--app-muted-foreground); }
.connect-seg button.on { background: var(--app-card); color: var(--app-foreground); font-weight: 500; box-shadow: 0 1px 2px rgb(0 0 0 / 0.12); }
.connect-adv summary { cursor: pointer; font-size: 0.8125rem; color: var(--app-muted-foreground); list-style: none; display: inline-flex; align-items: center; gap: 0.25rem; }
.connect-adv summary::before { content: "›"; display: inline-block; transition: transform 0.15s; }
.connect-adv[open] summary::before { transform: rotate(90deg); }
.connect-adv summary::-webkit-details-marker { display: none; }
</style>
