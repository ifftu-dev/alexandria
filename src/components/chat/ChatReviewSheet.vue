<script setup lang="ts">
import { computed, nextTick, onMounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { AppButton } from '@/components/ui'
import type { ChatPreparedMessage } from '@/types'

const props = defineProps<{
  prepared: ChatPreparedMessage
  /** True when the thread already has messages, so the scope changed. */
  continuing: boolean
  busy: boolean
}>()
const emit = defineEmits<{
  send: []
  cancel: []
  fresh: []
  toggleBinding: [bindingId: string]
  toggleHistory: [include: boolean]
  toggleMemory: [memoryId: string]
}>()
const { t } = useI18n()
const sheet = ref<HTMLElement | null>(null)

const destination = computed(() => props.prepared.destination)
const required = computed(() => props.prepared.items.filter((i) => i.kind === 'instructions' || i.kind === 'message'))
const history = computed(() => props.prepared.items.find((i) => i.kind === 'history') ?? null)
const lessons = computed(() => props.prepared.items.filter((i) => i.kind === 'lesson' || i.kind === 'goal'))
const memory = computed(() => props.prepared.items.filter((i) => i.kind === 'memory'))
const memoryTokens = computed(() => memory.value.reduce((sum, i) => sum + (i.included ? i.tokens : 0), 0))
const requiredTokens = computed(() => required.value.reduce((sum, i) => sum + i.tokens, 0))
const attachedTokens = computed(() => lessons.value.reduce((sum, i) => sum + (i.included ? i.tokens : 0), 0))
const meter = computed(() =>
  Math.min(100, Math.round((props.prepared.estimated_input_tokens / Math.max(1, props.prepared.budget_tokens)) * 100)),
)
const n = (value: number) => value.toLocaleString()

onMounted(async () => {
  await nextTick()
  sheet.value?.focus()
})
</script>

<template>
  <div class="absolute inset-0 z-20 flex items-end bg-foreground/20" @keydown.esc="emit('cancel')">
    <div
      ref="sheet"
      class="max-h-[94%] w-full overflow-y-auto rounded-t-2xl border-t border-border bg-card p-4 shadow-lg outline-none"
      role="dialog"
      aria-modal="true"
      aria-labelledby="chat-review-title"
      tabindex="-1"
    >
      <h2 id="chat-review-title" class="text-[15px] font-semibold">
        {{ prepared.fits ? (continuing ? t('chat.review.changedTitle') : t('chat.review.title')) : t('chat.review.tooLargeTitle') }}
      </h2>
      <p class="mb-3 mt-0.5 text-xs text-muted-foreground">
        {{
          prepared.fits
            ? (continuing ? t('chat.review.changedBody') : t('chat.review.firstSend'))
            : t('chat.review.tooLargeBody', { tokens: n(prepared.estimated_input_tokens), budget: n(prepared.budget_tokens) })
        }}
      </p>

      <div
        class="mb-2.5 flex items-start gap-2.5 rounded-lg border p-2.5 text-[12.5px]"
        :class="destination.location === 'remote'
          ? 'border-primary/30 bg-primary/[0.07]'
          : destination.location === 'private_network' ? 'border-warning/45 bg-warning/[0.09]' : 'border-success/40 bg-success/[0.08]'"
      >
        <span class="chat-loc mt-1.5" :class="`chat-loc--${destination.location}`" aria-hidden="true" />
        <div class="min-w-0">
          <p class="text-[13px] font-semibold">{{ t(`chat.location.${destination.location}`) }} · {{ destination.host }}</p>
          <p class="text-muted-foreground">
            {{ destination.connection_name }} · {{ destination.model_id }} ·
            {{ destination.location === 'device' ? t('chat.review.deviceNote') : t('chat.review.provider') }}
          </p>
        </div>
      </div>

      <section class="chat-rv">
        <header>{{ t('chat.review.required') }}<span>~{{ n(requiredTokens) }}</span></header>
        <div v-for="item in required" :key="item.kind" class="chat-rv-row">
          <div class="min-w-0 flex-1">
            <p>{{ item.kind === 'instructions' ? t('chat.review.instructions') : t('chat.review.yourMessage') }}</p>
            <p class="chat-rv-detail">{{ item.kind === 'instructions' ? item.label : `"${item.label}"` }}</p>
          </div>
          <span class="chat-switch" aria-hidden="true" data-on="true" data-locked="true" />
        </div>
      </section>

      <section v-if="history" class="chat-rv">
        <header>{{ t('chat.review.conversation') }}<span>~{{ n(history.tokens) }}</span></header>
        <div class="chat-rv-row" :class="{ 'chat-rv-row--off': !history.included }">
          <div class="min-w-0 flex-1">
            <p>{{ t('chat.review.turns', Number(history.label)) }}</p>
            <p v-if="prepared.omitted_turns" class="chat-rv-detail">{{ t('chat.review.omitted', { count: prepared.omitted_turns }) }}</p>
          </div>
          <button
            type="button"
            role="switch"
            class="chat-switch"
            :data-on="history.included"
            :aria-checked="history.included"
            :aria-label="t('chat.review.conversation')"
            :disabled="busy"
            @click="emit('toggleHistory', !history.included)"
          />
        </div>
      </section>

      <section v-if="lessons.length" class="chat-rv">
        <header>{{ t('chat.review.attached') }}<span>~{{ n(attachedTokens) }}</span></header>
        <div v-for="item in lessons" :key="item.binding_id ?? item.label" class="chat-rv-row" :class="{ 'chat-rv-row--off': !item.included }">
          <div class="min-w-0 flex-1">
            <p>{{ item.kind === 'goal' ? t('chat.context.goal', { title: item.label }) : t('chat.context.lesson', { title: item.label }) }}</p>
            <p v-if="item.truncated" class="chat-rv-detail">{{ t('chat.review.partly') }}</p>
          </div>
          <span v-if="item.included" class="text-xs text-muted-foreground">~{{ n(item.tokens) }}</span>
          <button
            type="button"
            role="switch"
            class="chat-switch"
            :data-on="item.included"
            :aria-checked="item.included"
            :aria-label="item.label"
            :disabled="busy"
            @click="item.binding_id && emit('toggleBinding', item.binding_id)"
          />
        </div>
      </section>

      <section v-if="memory.length" class="chat-rv">
        <header>{{ t('chat.memory.reviewTitle') }}<span>~{{ n(memoryTokens) }}</span></header>
        <div v-for="item in memory" :key="item.memory_id ?? item.label" class="chat-rv-row" :class="{ 'chat-rv-row--off': !item.included }">
          <div class="min-w-0 flex-1">
            <p class="break-words">{{ item.label }}</p>
          </div>
          <button
            type="button"
            role="switch"
            class="chat-switch"
            :data-on="item.included"
            :aria-checked="item.included"
            :aria-label="item.label"
            :disabled="busy"
            @click="item.memory_id && emit('toggleMemory', item.memory_id)"
          />
        </div>
      </section>

      <section class="chat-rv">
        <header>{{ t('chat.review.never') }}</header>
        <div class="chat-rv-row text-muted-foreground">
          <div class="min-w-0 flex-1">
            <p>{{ t('chat.review.neverItem') }}</p>
            <p class="chat-rv-detail">{{ t('chat.review.neverDetail') }}</p>
          </div>
        </div>
      </section>

      <p class="mb-1 mt-1 text-xs text-muted-foreground">
        {{ t('chat.review.estimate', { tokens: n(prepared.estimated_input_tokens), budget: n(prepared.budget_tokens), reserve: n(prepared.output_reserve_tokens) }) }}
      </p>
      <div class="h-1.5 overflow-hidden rounded-full bg-muted">
        <div class="h-full rounded-full" :class="prepared.fits ? 'bg-primary' : 'bg-error'" :style="{ width: `${meter}%` }" />
      </div>

      <div class="mt-3 flex items-center justify-end gap-2">
        <p v-if="prepared.fits" class="flex-1 text-xs text-muted-foreground">{{ t('chat.review.reuse') }}</p>
        <AppButton variant="ghost" size="sm" type="button" @click="emit('cancel')">{{ t('chat.review.cancel') }}</AppButton>
        <AppButton v-if="prepared.fits" size="sm" type="button" :loading="busy" @click="emit('send')">{{ t('chat.review.send') }}</AppButton>
      </div>
      <div v-if="continuing" class="mt-2 text-end text-xs">
        <button type="button" class="font-medium text-primary hover:underline" @click="emit('fresh')">{{ t('chat.review.freshChat') }}</button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.chat-rv { border: 1px solid var(--app-border); border-radius: var(--radius); margin-bottom: 0.625rem; overflow: hidden; }
.chat-rv header { display: flex; justify-content: space-between; gap: 0.5rem; padding: 0.4rem 0.625rem; font-size: 0.75rem; font-weight: 600; background: var(--app-background); }
.chat-rv header span { font-weight: 400; color: var(--app-muted-foreground); }
.chat-rv-row { display: flex; align-items: center; gap: 0.5rem; padding: 0.375rem 0.625rem; border-top: 1px solid var(--app-border); font-size: 0.78rem; }
.chat-rv-detail { font-size: 0.6875rem; color: var(--app-muted-foreground); }
.chat-rv-row--off { color: var(--app-muted-foreground); }
.chat-rv-row--off p:first-child { text-decoration: line-through; }
.chat-switch { position: relative; width: 1.75rem; height: 1rem; flex: none; border-radius: 999px; background: var(--app-border); }
.chat-switch[data-on='true'] { background: var(--app-primary); }
.chat-switch[data-locked='true'] { opacity: 0.5; }
.chat-switch::after { content: ''; position: absolute; top: 2px; inset-inline-start: 2px; width: 12px; height: 12px; border-radius: 999px; background: #fff; transition: inset-inline-start 150ms; }
.chat-switch[data-on='true']::after { inset-inline-start: 14px; }
.chat-switch:focus-visible { outline: 2px solid var(--app-ring); outline-offset: 2px; }
.chat-loc { width: 7px; height: 7px; flex: none; border-radius: 999px; }
.chat-loc--device { background: var(--app-success); }
.chat-loc--private_network { background: var(--app-warning); }
.chat-loc--remote { background: var(--app-primary); }
</style>
