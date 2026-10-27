<script setup lang="ts">
import { computed, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { useProviderSignIn } from '@/composables/useProviderSignIn'
import { renderChatMarkdown } from '@/utils/chatMarkdown'
import type { ChatMessage, ChatSourceRef } from '@/types'

const props = defineProps<{
  message: ChatMessage
  /** Text streamed so far, while the answer is being written. */
  live?: string
  /** Position of this answer among the answers to the same question. */
  version?: { current: number; total: number }
  /** The newest answer keeps its actions visible; older ones show them on hover. */
  latest?: boolean
}>()
const emit = defineEmits<{
  retry: []
  saveNote: []
  cite: [source: ChatSourceRef]
  version: [delta: number]
}>()
const { t } = useI18n()
const copied = ref(false)

const isUser = computed(() => props.message.role === 'user')
const streaming = computed(() => props.message.status === 'streaming')
// A Practice answer ends with a question block that becomes a card; while it
// streams, show only the prose before it.
const text = computed(() => {
  const raw = streaming.value ? (props.live || props.message.content) : props.message.content
  // Practice questions and memory suggestions arrive as fenced blocks that
  // become cards; while streaming, hide them from the first fence on.
  const fences = ['```practice', '```memory'].map((f) => raw.indexOf(f)).filter((i) => i >= 0)
  return fences.length ? raw.slice(0, Math.min(...fences)).trimEnd() : raw
})

// Markdown is parsed once the answer is final. While it streams the raw text
// is shown, so a long answer is never re-parsed on every token.
const html = computed(() =>
  isUser.value || streaming.value
    ? ''
    : renderChatMarkdown(text.value, props.message.sources, {
        unverified: t('chat.message.unverified'),
        unverifiedHint: t('chat.message.unverifiedHint'),
        sourceHint: t('chat.message.sourceHint'),
      }),
)

const attribution = computed(() => {
  const m = props.message
  const parts = [m.model_id, m.location ? t(`chat.location.${m.location}`) : null].filter(Boolean)
  return parts.join(' · ')
})

const { openPage } = useProviderSignIn()
function openUsage() {
  void openPage('chatgpt_usage').catch(() => {})
}

const errorText = computed(() => {
  const code = props.message.error ?? ''
  if (!code || code === 'cancelled') return ''
  if (code.startsWith('rate_limited:')) {
    return t('chat.errors.rate_limited_after', { seconds: code.split(':')[1] })
  }
  if (code === 'unreachable' && props.message.location === 'device') return t('chat.errors.unreachable_device')
  const key = code.startsWith('http:') ? 'http' : code
  const known = [
    'assessment_active', 'unreachable', 'timeout', 'interrupted', 'auth', 'rate_limited',
    'model_unavailable', 'http', 'malformed', 'too_large', 'empty', 'provider', 'unsupported',
    'invalid', 'too_many_runs', 'usage_limit', 'plan_ineligible', 'signin_required',
  ]
  return t(`chat.errors.${known.includes(key) ? key : 'generic'}`)
})

function onClick(event: MouseEvent | KeyboardEvent) {
  const target = (event.target as HTMLElement).closest<HTMLElement>('[data-source]')
  if (!target) return
  if (event instanceof KeyboardEvent && event.key !== 'Enter' && event.key !== ' ') return
  const source = props.message.sources.find((s) => s.id === target.dataset.source)
  if (source) {
    event.preventDefault()
    emit('cite', source)
  }
}

async function copy() {
  try {
    await navigator.clipboard.writeText(props.message.content)
    copied.value = true
    setTimeout(() => (copied.value = false), 1500)
  } catch {
    /* clipboard unavailable */
  }
}
</script>

<template>
  <div v-if="isUser" class="flex justify-end">
    <div class="chat-you">
      {{ message.content }}
    </div>
  </div>
  <div v-else class="group text-foreground">
    <div
      v-if="message.status === 'interrupted' && text"
      class="border-s-[3px] border-warning ps-3"
    >
      <div class="chat-prose" @click="onClick" @keydown="onClick" v-html="html" />
    </div>
    <p v-else-if="streaming" class="whitespace-pre-wrap break-words leading-relaxed">
      {{ text }}<span class="chat-caret" aria-hidden="true" />
    </p>
    <div v-else-if="text" class="chat-prose" @click="onClick" @keydown="onClick" v-html="html" />

    <p v-if="streaming" class="mt-1 flex items-center gap-1.5 text-xs text-muted-foreground" role="status">
      <span class="chat-spin" aria-hidden="true" />{{ t('chat.message.writing') }}
    </p>

    <div
      v-if="message.status === 'interrupted' || message.status === 'failed'"
      class="mt-2 rounded-xl border px-3.5 py-2.5 text-[13px]"
      :class="message.status === 'interrupted' ? 'border-warning/40 bg-warning/10' : 'border-error/35 bg-error/5'"
      role="alert"
    >
      <p class="font-semibold">
        {{ message.error === 'usage_limit' ? t('chat.signin.limitTitle') : message.status === 'interrupted' ? t('chat.message.interruptedTitle') : t('chat.message.failedTitle') }}
      </p>
      <p class="mt-0.5 text-muted-foreground">
        {{ message.status === 'interrupted' && message.error === 'interrupted' ? t('chat.message.interruptedBody') : errorText }}
      </p>
      <button
        v-if="message.error === 'usage_limit'"
        type="button"
        class="mt-2 rounded-full bg-foreground px-3 py-1 text-xs font-medium text-background"
        @click="openUsage"
      >{{ t('chat.signin.manageUsage') }}</button>
    </div>
    <p v-else-if="message.error === 'practice_rejected'" class="mt-1 text-xs text-muted-foreground">
      {{ t('chat.practice.rejected') }}
    </p>
    <p v-else-if="message.status === 'cancelled'" class="mt-1 text-xs text-muted-foreground">
      {{ message.error === 'assessment_active' ? t('chat.errors.assessment_active') : t('chat.message.cancelled') }}
    </p>

    <div v-if="!streaming" class="chat-actions" :class="{ 'chat-actions--show': latest || message.status !== 'complete' }">
      <template v-if="version && version.total > 1">
        <button
          type="button"
          class="chat-icon-btn"
          :disabled="version.current <= 1"
          :aria-label="t('chat.message.previousVersion')"
          @click="emit('version', -1)"
        >‹</button>
        <span>{{ t('chat.message.version', version) }}</span>
        <button
          type="button"
          class="chat-icon-btn"
          :disabled="version.current >= version.total"
          :aria-label="t('chat.message.nextVersion')"
          @click="emit('version', 1)"
        >›</button>
      </template>
      <button v-if="message.content" type="button" class="chat-icon-btn" :title="copied ? t('chat.message.copied') : t('chat.message.copy')" :aria-label="t('chat.message.copy')" @click="copy">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8"><rect x="9" y="9" width="11" height="11" rx="2" /><path d="M5 15V6a2 2 0 0 1 2-2h9" /></svg>
      </button>
      <button
        v-if="message.status === 'complete' && message.content"
        type="button"
        class="chat-icon-btn"
        :title="t('chat.artifacts.saveNote')"
        :aria-label="t('chat.artifacts.saveNote')"
        @click="emit('saveNote')"
      >
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M6 3h12v18l-6-4-6 4z" /></svg>
      </button>
      <button type="button" class="chat-icon-btn" :title="t('chat.message.retryHint')" :aria-label="t('chat.message.retry')" @click="emit('retry')">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"><path d="M4 4v6h6M20 20v-6h-6" /><path d="M20 10a8 8 0 0 0-14-4L4 10M4 14a8 8 0 0 0 14 4l2-4" /></svg>
      </button>
      <span class="chat-attr">{{ attribution }}</span>
    </div>
  </div>
</template>

<style scoped>
.chat-you { max-width: 85%; white-space: pre-wrap; overflow-wrap: anywhere; border-radius: 1.125rem 1.125rem 0.375rem 1.125rem; background: color-mix(in srgb, var(--app-foreground) 8%, transparent); padding: 0.625rem 0.875rem; line-height: 1.5; color: var(--app-foreground); }
.chat-actions { display: flex; flex-wrap: wrap; align-items: center; gap: 0.125rem; margin-top: 0.375rem; font-size: 0.75rem; color: var(--app-muted-foreground); opacity: 0; transition: opacity 0.15s; }
.group:hover .chat-actions, .group:focus-within .chat-actions, .chat-actions--show { opacity: 1; }
@media (hover: none) { .chat-actions { opacity: 1; } }
.chat-attr { margin-inline-start: 0.375rem; }
.chat-prose :deep(p) { margin: 0 0 0.625rem; line-height: 1.6; }
.chat-prose :deep(p:last-child) { margin-bottom: 0; }
.chat-prose :deep(ul), .chat-prose :deep(ol) { margin: 0 0 0.5rem; padding-inline-start: 1.25rem; }
.chat-prose :deep(ul) { list-style: disc; }
.chat-prose :deep(ol) { list-style: decimal; }
.chat-prose :deep(code) { font-family: var(--font-mono); font-size: 0.8125rem; background: var(--app-muted); padding: 0.05rem 0.3rem; border-radius: 0.25rem; }
.chat-prose :deep(pre) { background: var(--app-muted); padding: 0.6rem 0.75rem; border-radius: var(--radius); overflow-x: auto; margin: 0 0 0.5rem; }
.chat-prose :deep(pre code) { background: transparent; padding: 0; }
.chat-prose :deep(h1), .chat-prose :deep(h2), .chat-prose :deep(h3) { font-weight: 600; margin: 0.75rem 0 0.35rem; font-size: 0.9375rem; }
.chat-prose :deep(blockquote) { border-inline-start: 3px solid var(--app-border); padding-inline-start: 0.75rem; color: var(--app-muted-foreground); }
.chat-prose :deep(.chat-link) { text-decoration: underline dotted; word-break: break-all; }
.chat-prose :deep(.chat-cite) {
  display: inline-flex; align-items: center; font-size: 0.6875rem; line-height: 1.3; padding: 0.05rem 0.4rem;
  border-radius: 999px; vertical-align: 1px; cursor: pointer; margin-inline: 0.1rem;
  background: color-mix(in srgb, var(--app-primary) 12%, transparent); color: var(--app-primary);
}
.chat-prose :deep(.chat-cite:focus-visible) { outline: 2px solid var(--app-ring); outline-offset: 1px; }
.chat-prose :deep(.chat-cite--unverified) {
  cursor: help;
  background: color-mix(in srgb, var(--app-warning) 16%, transparent);
  color: color-mix(in srgb, var(--app-warning) 70%, var(--app-foreground));
}
.chat-caret { display: inline-block; width: 7px; height: 14px; margin-inline-start: 2px; vertical-align: -2px; background: currentColor; animation: chat-blink 1s steps(2) infinite; }
.chat-spin { width: 10px; height: 10px; border-radius: 999px; border: 2px solid var(--app-border); border-top-color: var(--app-primary); animation: chat-spin 0.8s linear infinite; }
@keyframes chat-blink { 50% { opacity: 0; } }
@keyframes chat-spin { to { transform: rotate(360deg); } }
@media (prefers-reduced-motion: reduce) { .chat-caret, .chat-spin { animation: none; } }
.chat-icon-btn { display: inline-grid; place-items: center; min-width: 1.75rem; height: 1.75rem; border-radius: 0.5rem; color: var(--app-muted-foreground); }
.chat-icon-btn:hover:not(:disabled) { background: var(--app-muted); color: var(--app-foreground); }
.chat-icon-btn:disabled { opacity: 0.4; }
.chat-icon-btn svg { width: 0.9375rem; height: 0.9375rem; }
</style>
