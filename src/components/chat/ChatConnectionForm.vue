<script setup lang="ts">
import { computed, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { AppButton } from '@/components/ui'
import { useLocalApi } from '@/composables/useLocalApi'
import { useAccountStatus } from '@/composables/useAccountStatus'
import { chatErrorCode } from '@/composables/useChat'
import type { StudioConnection, StudioDocument } from '@/types'

const emit = defineEmits<{ saved: [connection: StudioDocument<StudioConnection>]; cancel: [] }>()
const { t } = useI18n()
const { invoke } = useLocalApi()
const { isMinor } = useAccountStatus()

type Where = 'local' | 'private_network' | 'cloud'
type Protocol = 'compatible' | 'anthropic'
const defaults: Record<Where, string> = {
  local: 'http://localhost:11434/v1',
  private_network: 'http://192.168.1.10:1234/v1',
  cloud: 'https://api.openai.com/v1',
}
const anthropicEndpoint = 'https://api.anthropic.com/v1'

const name = ref('')
const where = ref<Where>('local')
const protocol = ref<Protocol>('compatible')
const endpoint = ref(defaults.local)
const model = ref('')
const apiKey = ref('')
const contextTokens = ref('')
const saving = ref(false)
const message = ref('')

const options = computed(() => [
  { value: 'local' as const, label: t('chat.location.device'), hint: t('chat.connection.deviceHint'), dot: 'device' },
  { value: 'private_network' as const, label: t('chat.location.private_network'), hint: t('chat.connection.privateHint'), dot: 'private_network' },
  { value: 'cloud' as const, label: t('chat.location.remote'), hint: t('chat.connection.remoteHint'), dot: 'remote' },
])

function defaultEndpoint(w: Where, p: Protocol) {
  return w === 'cloud' && p === 'anthropic' ? anthropicEndpoint : defaults[w]
}

function choose(value: Where) {
  if (endpoint.value === defaultEndpoint(where.value, protocol.value)) endpoint.value = defaultEndpoint(value, protocol.value)
  where.value = value
}

function chooseProtocol(value: Protocol) {
  if (endpoint.value === defaultEndpoint(where.value, protocol.value)) endpoint.value = defaultEndpoint(where.value, value)
  protocol.value = value
}

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
          name: name.value.trim(),
          endpoint: endpoint.value.trim(),
          model: model.value.trim(),
          location: where.value,
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
  <form class="space-y-3 text-sm" @submit.prevent="save">
    <div>
      <h3 class="text-[15px] font-semibold">{{ t('chat.connection.title') }}</h3>
      <p class="mt-0.5 text-xs text-muted-foreground">{{ t('chat.connection.intro') }}</p>
    </div>
    <label class="block">
      <span class="label">{{ t('chat.connection.name') }}</span>
      <input v-model="name" class="input text-sm" required maxlength="200" :placeholder="t('chat.connection.namePlaceholder')">
    </label>
    <fieldset>
      <legend class="label">{{ t('chat.connection.where') }}</legend>
      <div class="grid gap-2 sm:grid-cols-3">
        <button
          v-for="option in options"
          :key="option.value"
          type="button"
          class="rounded-lg border p-2 text-start text-xs transition-colors"
          :class="where === option.value ? 'border-primary ring-1 ring-primary' : 'border-border hover:bg-muted/50'"
          :aria-pressed="where === option.value"
          @click="choose(option.value)"
        >
          <span class="flex items-center gap-1.5 text-[13px] font-medium text-foreground">
            <span class="chat-loc" :class="`chat-loc--${option.dot}`" aria-hidden="true" />{{ option.label }}
          </span>
          <span class="mt-0.5 block text-muted-foreground">{{ option.hint }}</span>
        </button>
      </div>
      <p v-if="where === 'cloud' && isMinor" class="mt-1.5 text-xs text-warning">{{ t('chat.connection.guardianNote') }}</p>
    </fieldset>
    <fieldset>
      <legend class="label">{{ t('chat.connection.protocol') }}</legend>
      <div class="flex gap-2" role="radiogroup" :aria-label="t('chat.connection.protocol')">
        <button
          v-for="option in (['compatible', 'anthropic'] as const)"
          :key="option"
          type="button"
          role="radio"
          class="rounded-full border px-3 py-1 text-xs transition-colors"
          :class="protocol === option ? 'border-primary bg-primary/10 font-medium text-foreground' : 'border-border text-muted-foreground hover:bg-muted/50'"
          :aria-checked="protocol === option"
          @click="chooseProtocol(option)"
        >{{ t(`chat.connection.protocols.${option}`) }}</button>
      </div>
    </fieldset>
    <label class="block">
      <span class="label">{{ t('chat.connection.endpoint') }}</span>
      <input v-model="endpoint" class="input text-sm" required inputmode="url" spellcheck="false">
    </label>
    <div class="grid gap-3 sm:grid-cols-2">
      <label class="block">
        <span class="label">{{ t('chat.connection.model') }}</span>
        <input v-model="model" class="input text-sm" required maxlength="200" spellcheck="false" :placeholder="t('chat.connection.modelPlaceholder')">
      </label>
      <label class="block">
        <span class="label">{{ where === 'cloud' ? t('chat.connection.apiKey') : t('chat.connection.apiKeyOptional') }}</span>
        <input v-model="apiKey" class="input text-sm" type="password" autocomplete="off" :required="where === 'cloud'">
      </label>
    </div>
    <label class="block">
      <span class="label">{{ t('chat.connection.contextTokens') }}</span>
      <input v-model="contextTokens" class="input text-sm" inputmode="numeric" pattern="[0-9]*">
      <span class="mt-1 block text-xs text-muted-foreground">{{ t('chat.connection.contextHint') }}</span>
    </label>
    <p v-if="message" class="text-xs text-error" role="alert">{{ message }}</p>
    <div class="flex justify-end gap-2">
      <AppButton variant="ghost" size="sm" type="button" @click="emit('cancel')">{{ t('chat.connection.cancel') }}</AppButton>
      <AppButton size="sm" type="submit" :loading="saving">{{ t('chat.connection.save') }}</AppButton>
    </div>
  </form>
</template>

<style scoped>
.chat-loc { width: 7px; height: 7px; flex: none; border-radius: 999px; }
.chat-loc--device { background: var(--app-success); }
.chat-loc--private_network { background: var(--app-warning); }
.chat-loc--remote { background: var(--app-primary); }
</style>
