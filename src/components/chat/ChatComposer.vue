<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useChat } from '@/composables/useChat'
import { useAccountStatus } from '@/composables/useAccountStatus'
import { useProviderSignIn } from '@/composables/useProviderSignIn'
import type { ChatLocation, StudioConnection, StudioDocument } from '@/types'

const emit = defineEmits<{ addConnection: [] }>()
const { t } = useI18n()
const chat = useChat()
const { isMinor } = useAccountStatus()
const input = ref<HTMLTextAreaElement | null>(null)
const pickerOpen = ref(false)
const models = ref<string[] | null>(null)
const modelsFor = ref<string | null>(null)
const loadingModels = ref(false)
const modelsError = ref('')

const signIn = useProviderSignIn()
/** A connection the user asked to remove, awaiting confirmation. */
const removing = ref<string | null>(null)
/** What to tell the user after a removal, if anything. */
const removedNote = ref<'' | 'chatgpt' | 'openrouter'>('')
const usingPlan = computed(() => chat.selectedConnection.value?.value.auth === 'chatgpt_plan')

async function remove(c: StudioDocument<StudioConnection>) {
  if (removing.value !== c.id) {
    removing.value = c.id
    return
  }
  removing.value = null
  removedNote.value = ''
  try {
    const result = await signIn.disconnect(c.id)
    if (c.value.auth === 'chatgpt_plan' && !result.revoked) removedNote.value = 'chatgpt'
    else if (c.id.startsWith('openrouter-')) removedNote.value = 'openrouter'
    await chat.refreshConnections()
  } catch {
    chat.error.value = 'generic'
  }
}

const busy = computed(() => chat.preparing.value || chat.streaming.value)
const placeholder = computed(() =>
  chat.mode.value === 'practice'
    ? t('chat.composer.placeholderPractice')
    : chat.mode.value === 'learn'
      ? t('chat.composer.placeholderLearn')
      : t('chat.composer.placeholder'),
)
const canSend = computed(() => !!chat.draft.value.trim() && !chat.preparing.value && !!chat.selectedConnection.value)

function locationOf(value: string | undefined): ChatLocation {
  if (value === 'cloud') return 'remote'
  if (value === 'private_network') return 'private_network'
  return 'device'
}
const location = computed(() => locationOf(chat.selectedConnection.value?.value.location))
const destination = computed(() => {
  const c = chat.selectedConnection.value
  if (!c) return ''
  let host = c.value.endpoint
  try {
    host = new URL(c.value.endpoint).host
  } catch {
    /* shown as entered */
  }
  return `${t(`chat.location.${location.value}`)} · ${host}`
})

function onKeydown(event: KeyboardEvent) {
  if (event.key === 'Enter' && !event.shiftKey && !event.isComposing) {
    event.preventDefault()
    if (canSend.value && !chat.streaming.value) void chat.send()
  }
}

function autosize() {
  const el = input.value
  if (!el) return
  el.style.height = 'auto'
  el.style.height = `${Math.min(el.scrollHeight, 180)}px`
}
watch(() => chat.draft.value, () => nextTick(autosize))

async function showModels(connectionId: string) {
  modelsFor.value = connectionId
  models.value = null
  modelsError.value = ''
  loadingModels.value = true
  try {
    models.value = await chat.listModels(connectionId)
  } catch {
    modelsError.value = t('chat.composer.noModelsListed')
  } finally {
    loadingModels.value = false
  }
}

async function choose(connectionId: string, modelId: string | null) {
  pickerOpen.value = false
  await chat.selectModel(connectionId, modelId)
}

function closeOnOutside(event: MouseEvent) {
  if (!(event.target as HTMLElement).closest('[data-chat-picker]')) pickerOpen.value = false
}
watch(pickerOpen, (open) => {
  if (open && isMinor.value) void chat.refreshApprovals().catch(() => {})
  if (open) document.addEventListener('mousedown', closeOnOutside)
  else document.removeEventListener('mousedown', closeOnOutside)
})
onBeforeUnmount(() => document.removeEventListener('mousedown', closeOnOutside))

defineExpose({ focus: () => input.value?.focus() })
</script>

<template>
  <div class="px-3 pb-3 pt-2">
    <div class="rounded-xl border border-input bg-background px-2.5 pb-1.5 pt-2 focus-within:border-ring focus-within:ring-2 focus-within:ring-ring/20">
      <textarea
        ref="input"
        :value="chat.draft.value"
        rows="2"
        class="block max-h-44 w-full resize-none bg-transparent text-sm leading-relaxed text-foreground outline-none placeholder:text-muted-foreground"
        :placeholder="placeholder"
        :aria-label="placeholder"
        @input="chat.setDraft(($event.target as HTMLTextAreaElement).value)"
        @keydown="onKeydown"
      />
      <div class="flex items-center gap-1.5">
        <div class="relative min-w-0" data-chat-picker>
          <button
            type="button"
            class="flex max-w-[15rem] items-center gap-1.5 rounded-full px-2 py-0.5 text-xs text-muted-foreground hover:bg-muted hover:text-foreground"
            :aria-expanded="pickerOpen"
            aria-haspopup="listbox"
            @click="pickerOpen = !pickerOpen"
          >
            <span v-if="chat.selectedConnection.value" class="chat-loc" :class="`chat-loc--${location}`" aria-hidden="true" />
            <span class="truncate font-medium text-foreground">{{ chat.selectedModel.value ?? t('chat.composer.chooseModel') }}</span>
            <svg class="h-3 w-3 flex-none" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M6 9l6 6 6-6" /></svg>
          </button>
          <div
            v-if="pickerOpen"
            class="absolute bottom-full start-0 z-30 mb-1 max-h-72 w-72 overflow-y-auto rounded-lg border border-border bg-card p-1 shadow-lg"
            role="listbox"
          >
            <template v-for="c in chat.connections.value" :key="c.id">
              <button
                type="button"
                role="option"
                class="flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-start text-xs hover:bg-muted"
                :aria-selected="chat.selectedConnectionId.value === c.id"
                @click="choose(c.id, null)"
              >
                <span class="chat-loc" :class="`chat-loc--${locationOf(c.value.location)}`" aria-hidden="true" />
                <span class="min-w-0 flex-1">
                  <span class="block truncate font-medium text-foreground">{{ c.value.name }}</span>
                  <span class="block truncate text-muted-foreground">{{ c.value.model }} · {{ t(`chat.location.${locationOf(c.value.location)}`) }}</span>
                  <span
                    v-if="isMinor && c.value.location === 'cloud'"
                    class="block truncate text-[11px]"
                    :class="chat.approvalFor(c.id, c.value.endpoint)?.status === 'approved' ? 'text-success' : 'text-warning'"
                  >{{ t(`chat.guardian.status.${chat.approvalFor(c.id, c.value.endpoint)?.status ?? 'none'}`) }}</span>
                </span>
              </button>
              <div v-if="chat.selectedConnectionId.value === c.id" class="ms-5 border-s border-border ps-2">
                <button
                  v-if="modelsFor !== c.id"
                  type="button"
                  class="px-2 py-1 text-[11px] text-primary hover:underline"
                  @click="showModels(c.id)"
                >{{ t('chat.composer.listModels') }}</button>
                <p v-else-if="loadingModels" class="px-2 py-1 text-[11px] text-muted-foreground">{{ t('chat.composer.loadingModels') }}</p>
                <p v-else-if="modelsError" class="px-2 py-1 text-[11px] text-muted-foreground">{{ modelsError }}</p>
                <button
                  v-for="m in models ?? []"
                  :key="m"
                  type="button"
                  role="option"
                  class="block w-full truncate rounded px-2 py-1 text-start text-[11px] hover:bg-muted"
                  :aria-selected="chat.selectedModel.value === m"
                  @click="choose(c.id, m)"
                >{{ m }}</button>
                <button
                  type="button"
                  class="px-2 py-1 text-[11px] text-muted-foreground hover:text-error"
                  @click="remove(c)"
                >{{ removing === c.id ? t('chat.signin.removeConfirm') : t('chat.signin.remove') }}</button>
              </div>
            </template>
            <button
              type="button"
              class="mt-1 w-full rounded-md border-t border-border px-2 py-1.5 text-start text-xs font-medium text-primary hover:bg-muted"
              @click="pickerOpen = false; emit('addConnection')"
            >+ {{ t('chat.composer.addConnection') }}</button>
          </div>
        </div>
        <span class="flex-1" />
        <button
          v-if="chat.streaming.value"
          type="button"
          class="grid h-8 w-8 place-items-center rounded-full bg-foreground text-background"
          :aria-label="t('chat.composer.stop')"
          :title="t('chat.composer.stop')"
          @click="chat.stop()"
        >
          <svg class="h-3.5 w-3.5" viewBox="0 0 24 24" fill="currentColor"><rect x="6" y="6" width="12" height="12" rx="2" /></svg>
        </button>
        <button
          v-else
          type="button"
          class="grid h-8 w-8 place-items-center rounded-full bg-primary text-primary-foreground disabled:opacity-40"
          :disabled="!canSend || busy"
          :aria-label="chat.preparing.value ? t('chat.composer.preparing') : t('chat.composer.send')"
          :title="t('chat.composer.send')"
          @click="chat.send()"
        >
          <svg class="h-4 w-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 19V5M5 12l7-7 7 7" /></svg>
        </button>
      </div>
    </div>
    <p v-if="usingPlan" class="mt-1.5 flex items-center gap-1.5 text-[11px] text-muted-foreground">
      <span class="font-medium text-foreground">{{ t('chat.signin.usingPlan') }}</span>
      <span aria-hidden="true">·</span>
      <button type="button" class="text-primary hover:underline" @click="signIn.openPage('chatgpt_usage')">{{ t('chat.signin.manageUsage') }}</button>
    </p>
    <p v-else-if="destination" class="mt-1.5 truncate text-[11px] text-muted-foreground">{{ t('chat.composer.sendsTo', { destination }) }}</p>
    <p v-if="removedNote" class="mt-1 text-[11px] text-muted-foreground" role="status">
      {{ removedNote === 'chatgpt' ? t('chat.signin.removedChatgpt') : t('chat.signin.removedOpenrouter') }}
      <button
        v-if="removedNote === 'openrouter'"
        type="button"
        class="ms-1 text-primary hover:underline"
        @click="signIn.openPage('openrouter_keys')"
      >{{ t('chat.signin.openKeys') }}</button>
    </p>
  </div>
</template>

<style scoped>
.chat-loc { display: inline-block; width: 7px; height: 7px; flex: none; border-radius: 999px; }
.chat-loc--device { background: var(--app-success); }
.chat-loc--private_network { background: var(--app-warning); }
.chat-loc--remote { background: var(--app-primary); }
</style>
