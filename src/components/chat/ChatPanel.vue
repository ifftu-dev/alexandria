<script setup lang="ts">
import { computed, nextTick, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useRouter } from 'vue-router'
import { useChat } from '@/composables/useChat'
import { chatOffers } from '@/composables/useChatOffers'
import { snippetParts } from '@/utils/chatSnippet'
import ChatMessageItem from './ChatMessageItem.vue'
import ChatComposer from './ChatComposer.vue'
import ChatReviewSheet from './ChatReviewSheet.vue'
import ChatConnectionForm from './ChatConnectionForm.vue'
import ChatPracticeCard from './ChatPracticeCard.vue'
import ChatMemoryProposal from './ChatMemoryProposal.vue'
import ChatMemoryView from './ChatMemoryView.vue'
import ChatArtifactCard from './ChatArtifactCard.vue'
import ChatSavedView from './ChatSavedView.vue'
import { ConfirmDialog } from '@/components/ui'
import type { ChatMessage, ChatSearchHit, ChatSourceRef, StudioConnection, StudioDocument } from '@/types'

defineProps<{ variant: 'docked' | 'expanded' | 'sheet' }>()
const emit = defineEmits<{ close: [] }>()
const { t } = useI18n()
const router = useRouter()
const chat = useChat()
const scroller = ref<HTMLElement | null>(null)
const composer = ref<InstanceType<typeof ChatComposer> | null>(null)
const addingConnection = ref(false)
const renaming = ref<string | null>(null)
const renameText = ref('')
const deleting = ref<string | null>(null)
/** Which answer version is shown per user message, when there are several. */
const shownVersion = ref<Record<string, number>>({})

const messages = computed(() => chat.detail.value?.messages ?? [])
const practice = computed(() => chat.detail.value?.practice ?? [])
const latestPracticeId = computed(() => practice.value[practice.value.length - 1]?.id ?? null)
function proposalsFor(messageId: string) {
  return (chat.detail.value?.memory_proposals ?? []).filter((m) => m.source_message_id === messageId)
}
function artifactsFor(messageId: string) {
  return (chat.detail.value?.artifacts ?? []).filter((a) => a.message_id === messageId)
}
async function saveNote(question: ChatMessage, answer: ChatMessage) {
  const title = chat.thread.value?.title || question.content.split('\n')[0]?.slice(0, 80) || t('chat.artifacts.untitledNote')
  try {
    await chat.saveNote(answer.id, title)
  } catch {
    /* the composable reports the error */
  }
}
function practiceFor(messageId: string) {
  return practice.value.find((p) => p.message_id === messageId) ?? null
}
const modes = computed(() => [
  { value: 'ask' as const, label: t('chat.modes.ask'), hint: t('chat.modes.askHint') },
  { value: 'learn' as const, label: t('chat.modes.learn'), hint: t('chat.modes.learnHint') },
  { value: 'practice' as const, label: t('chat.modes.practice'), hint: t('chat.modes.practiceHint') },
])
const bindings = computed(() => chat.detail.value?.bindings ?? [])
const users = computed(() => messages.value.filter((m) => m.role === 'user'))
function answers(user: ChatMessage) {
  return messages.value.filter((m) => m.role === 'assistant' && m.reply_to === user.id)
}
function shown(user: ChatMessage) {
  const all = answers(user)
  if (!all.length) return null
  const pick = shownVersion.value[user.id]
  return all.find((m) => m.version === pick) ?? all[all.length - 1]!
}
function step(user: ChatMessage, delta: number) {
  const all = answers(user)
  const current = shown(user)
  const index = all.findIndex((m) => m.id === current?.id) + delta
  const next = all[Math.max(0, Math.min(all.length - 1, index))]
  if (next) shownVersion.value = { ...shownVersion.value, [user.id]: next.version }
}

/** Offers from the page that are not attached yet. */
const offers = computed(() =>
  chatOffers.value.filter(
    (o) => !bindings.value.some((b) => b.course_id === o.course_id && b.element_id === o.element_id),
  ),
)
const title = computed(() => chat.thread.value?.title || t('chat.panel.untitled'))
const noConnection = computed(() => chat.connections.value.length === 0)
const errorText = computed(() => {
  const code = chat.error.value
  if (!code) return ''
  const known = [
    'assessment_active', 'no_connection', 'guardian_required', 'context_stale', 'too_many_runs',
    'source_unavailable', 'unreachable', 'timeout', 'auth', 'rate_limited', 'invalid',
    'assessment_item', 'memory', 'guardian_pending', 'guardian_declined', 'no_guardian',
    'under_13', 'adults_only',
  ]
  return t(`chat.errors.${known.includes(code) ? code : 'generic'}`)
})

watch(
  () => [messages.value.length, chat.liveText.value],
  async () => {
    await nextTick()
    const el = scroller.value
    if (!el) return
    // Follow the answer only when the reader is already near the bottom.
    if (el.scrollHeight - el.scrollTop - el.clientHeight < 160) el.scrollTop = el.scrollHeight
  },
  { deep: true },
)
watch(
  () => chat.thread.value?.id,
  async () => {
    shownVersion.value = {}
    await nextTick()
    if (scroller.value) scroller.value.scrollTop = scroller.value.scrollHeight
  },
)

function cite(source: ChatSourceRef) {
  if (source.kind === 'goal') void router.push('/goals')
  else void router.push({ path: `/learn/${source.course_id}`, query: { element: source.element_id } })
}

async function connectionSaved(saved: StudioDocument<StudioConnection>) {
  addingConnection.value = false
  await chat.refreshConnections()
  await chat.selectModel(saved.id, null)
  await nextTick()
  composer.value?.focus()
}

async function freshChat() {
  const text = chat.draft.value
  chat.cancelReview()
  await chat.newChat()
  chat.setDraft(text)
}

function startRename(id: string, current: string) {
  renaming.value = id
  renameText.value = current
}
async function finishRename() {
  const id = renaming.value
  renaming.value = null
  if (id && renameText.value.trim()) await chat.renameThread(id, renameText.value.trim())
}
async function confirmDelete() {
  const id = deleting.value
  deleting.value = null
  if (id) await chat.deleteThread(id)
}
const searchText = ref('')
const searchHits = ref<ChatSearchHit[] | null>(null)
let searchTimer: ReturnType<typeof setTimeout> | undefined
let searchSeq = 0
watch(searchText, (text) => {
  clearTimeout(searchTimer)
  if (!text.trim()) {
    searchHits.value = null
    return
  }
  const seq = ++searchSeq
  searchTimer = setTimeout(async () => {
    const hits = await chat.searchThreads(text)
    if (seq === searchSeq) searchHits.value = hits
  }, 200)
})
watch(() => chat.showHistory.value, (open) => {
  if (!open) searchText.value = ''
})
async function openHit(hit: ChatSearchHit) {
  await chat.openThread(hit.thread.id)
  if (!hit.message_id) return
  await nextTick()
  scroller.value?.querySelector(`[data-message-id="${CSS.escape(hit.message_id)}"]`)?.scrollIntoView({ block: 'center' })
}
function formatDate(value: string) {
  try {
    return new Date(value).toLocaleDateString(undefined, { month: 'short', day: 'numeric' })
  } catch {
    return ''
  }
}
</script>

<template>
  <section class="relative flex h-full min-h-0 flex-col bg-card" :aria-label="t('chat.panel.title')">
    <header class="flex flex-none items-center gap-1 border-b border-border py-2 pe-2 ps-3.5">
      <button
        v-if="chat.showHistory.value || chat.showMemory.value || chat.showSaved.value"
        type="button"
        class="chat-head-btn"
        :aria-label="t('chat.panel.back')"
        @click="chat.showHistory.value = false; chat.showMemory.value = false; chat.showSaved.value = false"
      >
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9"><path stroke-linecap="round" stroke-linejoin="round" d="M15 19l-7-7 7-7" /></svg>
      </button>
      <h2 class="min-w-0 flex-1 truncate text-[13.5px] font-semibold">
        {{ chat.showHistory.value ? t('chat.panel.history') : chat.showMemory.value ? t('chat.memory.title') : chat.showSaved.value ? t('chat.artifacts.title') : title }}
      </h2>
      <button type="button" class="chat-head-btn" :title="t('chat.memory.open')" :aria-label="t('chat.memory.open')" :aria-pressed="chat.showMemory.value" @click="chat.showMemory.value = !chat.showMemory.value; chat.showHistory.value = false; chat.showSaved.value = false">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M12 3a6 6 0 0 0-6 6c0 2.2 1.2 3.7 2.4 4.8.8.7 1.1 1.4 1.1 2.2V17h5v-1c0-.8.3-1.5 1.1-2.2C16.8 12.7 18 11.2 18 9a6 6 0 0 0-6-6z" /><path d="M10 20h4" /></svg>
      </button>
      <button type="button" class="chat-head-btn" :title="t('chat.artifacts.open')" :aria-label="t('chat.artifacts.open')" :aria-pressed="chat.showSaved.value" @click="chat.showSaved.value = !chat.showSaved.value; chat.showHistory.value = false; chat.showMemory.value = false">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M6 3h12v18l-6-4-6 4z" /></svg>
      </button>
      <button type="button" class="chat-head-btn" :title="t('chat.panel.history')" :aria-label="t('chat.panel.history')" :aria-pressed="chat.showHistory.value" @click="chat.showHistory.value = !chat.showHistory.value; chat.showMemory.value = false; chat.showSaved.value = false">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8"><path d="M3 12a9 9 0 1 0 3-6.7L3 8" /><path d="M3 3v5h5M12 7v5l3 2" /></svg>
      </button>
      <button type="button" class="chat-head-btn" :title="t('chat.panel.newChat')" :aria-label="t('chat.panel.newChat')" @click="chat.newChat()">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"><path d="M12 5v14M5 12h14" /></svg>
      </button>
      <button
        v-if="variant !== 'sheet'"
        type="button"
        class="chat-head-btn"
        :title="variant === 'expanded' ? t('chat.panel.collapse') : t('chat.panel.expand')"
        :aria-label="variant === 'expanded' ? t('chat.panel.collapse') : t('chat.panel.expand')"
        @click="chat.expanded.value = !chat.expanded.value"
      >
        <svg v-if="variant === 'expanded'" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"><path d="M4 14h6v6M20 10h-6V4M14 10l7-7M3 21l7-7" /></svg>
        <svg v-else viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"><path d="M14 4h6v6M10 20H4v-6M20 4l-7 7M4 20l7-7" /></svg>
      </button>
      <button type="button" class="chat-head-btn" :title="t('chat.panel.closeHint')" :aria-label="t('chat.panel.close')" @click="emit('close')">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"><path d="M6 6l12 12M18 6L6 18" /></svg>
      </button>
    </header>

    <ChatSavedView v-if="chat.showSaved.value" />
    <ChatMemoryView v-else-if="chat.showMemory.value" />

    <!-- Thread history -->
    <div v-else-if="chat.showHistory.value" class="min-h-0 flex-1 overflow-y-auto p-2">
      <input
        v-if="chat.threads.value.length"
        v-model="searchText"
        type="search"
        class="input mb-2 py-1.5 text-sm"
        maxlength="200"
        :placeholder="t('chat.panel.search')"
        :aria-label="t('chat.panel.search')"
      >
      <template v-if="searchHits">
        <p v-if="!searchHits.length" class="px-2 py-6 text-center text-sm text-muted-foreground">{{ t('chat.panel.noResults') }}</p>
        <ul class="space-y-0.5">
          <li v-for="hit in searchHits" :key="hit.thread.id">
            <button type="button" class="w-full rounded-lg px-2.5 py-2 text-start hover:bg-muted/60" @click="openHit(hit)">
              <span class="block truncate text-sm font-medium">{{ hit.thread.title || t('chat.panel.untitled') }}</span>
              <span v-if="hit.snippet" class="line-clamp-2 block text-xs text-muted-foreground">
                <template v-for="(part, i) in snippetParts(hit.snippet)" :key="i"><mark v-if="part.hit" class="rounded-sm bg-warning/30 text-foreground">{{ part.text }}</mark><template v-else>{{ part.text }}</template></template>
              </span>
              <span class="block text-[11px] text-muted-foreground">{{ formatDate(hit.thread.updated_at) }}</span>
            </button>
          </li>
        </ul>
      </template>
      <p v-else-if="!chat.threads.value.length" class="px-2 py-6 text-center text-sm text-muted-foreground">{{ t('chat.panel.noThreads') }}</p>
      <ul v-if="!searchHits" class="space-y-0.5">
        <li v-for="item in chat.threads.value" :key="item.id" class="group flex items-center gap-1 rounded-lg hover:bg-muted/60">
          <input
            v-if="renaming === item.id"
            v-model="renameText"
            class="input mx-1 my-1 py-1 text-sm"
            maxlength="120"
            :aria-label="t('chat.panel.rename')"
            @keydown.enter.prevent="finishRename"
            @keydown.esc="renaming = null"
            @blur="finishRename"
          >
          <button v-else type="button" class="min-w-0 flex-1 px-2.5 py-2 text-start" @click="chat.openThread(item.id)">
            <span class="block truncate text-sm" :class="chat.thread.value?.id === item.id ? 'font-semibold' : ''">{{ item.title || t('chat.panel.untitled') }}</span>
            <span class="block text-[11px] text-muted-foreground">{{ formatDate(item.updated_at) }}</span>
          </button>
          <button type="button" class="chat-head-btn opacity-0 group-hover:opacity-100 focus:opacity-100" :aria-label="t('chat.panel.rename')" @click="startRename(item.id, item.title)">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8"><path d="M4 20h4L19 9l-4-4L4 16v4z" /></svg>
          </button>
          <button type="button" class="chat-head-btn opacity-0 group-hover:opacity-100 focus:opacity-100" :aria-label="t('chat.panel.delete')" @click="deleting = item.id">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8"><path d="M4 7h16M10 11v6M14 11v6M6 7l1 13h10l1-13M9 7V4h6v3" /></svg>
          </button>
        </li>
      </ul>
    </div>

    <template v-else>
      <div class="flex-none px-3 pt-2">
        <div class="tabs" role="tablist" :aria-label="t('chat.panel.title')">
          <button
            v-for="m in modes"
            :key="m.value"
            type="button"
            class="tab !py-1 text-xs"
            :class="{ 'tab-active': chat.mode.value === m.value }"
            role="tab"
            :aria-selected="chat.mode.value === m.value"
            :title="m.hint"
            :disabled="chat.streaming.value"
            @click="chat.setMode(m.value)"
          >{{ m.label }}</button>
        </div>
        <p v-if="chat.mode.value === 'practice'" class="mt-1.5 text-[11px] text-muted-foreground">{{ t('chat.practice.banner') }}</p>
      </div>

      <div ref="scroller" class="min-h-0 flex-1 space-y-4 overflow-y-auto px-3.5 py-3" aria-live="off">
        <div v-if="addingConnection || (noConnection && !messages.length)" class="rounded-xl border border-border bg-background p-3">
          <ChatConnectionForm @saved="connectionSaved" @cancel="addingConnection = false" />
        </div>
        <div v-else-if="!messages.length" class="px-2 py-8 text-center">
          <p class="text-sm font-semibold">{{ t('chat.panel.emptyTitle') }}</p>
          <p class="mx-auto mt-1 max-w-xs text-xs text-muted-foreground">{{ t('chat.panel.emptyBody') }}</p>
          <button
            v-if="chat.mode.value === 'practice'"
            type="button"
            class="mt-3 rounded-full border border-border px-3 py-1 text-xs font-medium hover:bg-muted"
            :disabled="chat.preparing.value"
            @click="chat.ask(t('chat.practice.startPrompt'))"
          >{{ t('chat.practice.start') }}</button>
        </div>
        <template v-for="user in users" :key="user.id">
          <ChatMessageItem :message="user" :data-message-id="user.id" />
          <ChatMessageItem
            v-if="shown(user)"
            :data-message-id="shown(user)!.id"
            :message="shown(user)!"
            :live="chat.liveText.value[shown(user)!.id]"
            :version="{ current: answers(user).findIndex((m) => m.id === shown(user)!.id) + 1, total: answers(user).length }"
            @retry="chat.retry(user.id)"
            @save-note="saveNote(user, shown(user)!)"
            @cite="cite"
            @version="(delta) => step(user, delta)"
          />
          <template v-if="shown(user)">
            <ChatMemoryProposal v-for="m in proposalsFor(shown(user)!.id)" :key="m.id" :item="m" />
            <ChatArtifactCard v-for="a in artifactsFor(shown(user)!.id)" :key="a.id" :item="a" />
          </template>
          <ChatPracticeCard
            v-if="shown(user) && practiceFor(shown(user)!.id)"
            :item="practiceFor(shown(user)!.id)!"
            :latest="practiceFor(shown(user)!.id)!.id === latestPracticeId"
          />
        </template>
      </div>

      <div v-if="errorText" class="mx-3 mb-1 flex items-start gap-2 rounded-lg border border-warning/40 bg-warning/10 px-3 py-2 text-xs" role="alert">
        <div class="flex-1">
          <p>{{ errorText }}</p>
          <button
            v-if="chat.error.value === 'guardian_required'"
            type="button"
            class="mt-1.5 rounded-full border border-border bg-background px-2.5 py-0.5 font-medium hover:bg-muted"
            @click="chat.requestGuardianApproval()"
          >{{ t('chat.guardian.ask') }}</button>
        </div>
        <button type="button" class="text-muted-foreground hover:text-foreground" :aria-label="t('chat.panel.close')" @click="chat.error.value = null">×</button>
      </div>

      <div v-if="bindings.length || offers.length" class="flex flex-none flex-wrap gap-1.5 border-t border-border px-3 pt-2" :aria-label="t('chat.context.label')">
        <span
          v-for="b in bindings"
          :key="b.id"
          class="inline-flex max-w-full items-center gap-1.5 rounded-full border border-border bg-background py-0.5 pe-1 ps-2 text-[11.5px]"
          :title="t('chat.context.removeHint')"
        >
          <svg class="h-3 w-3 flex-none" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8"><path d="M6 3h9l4 4v14H6z" /><path d="M14 3v5h5" /></svg>
          <span class="truncate">{{ b.kind === 'goal' ? t('chat.context.goal', { title: b.label }) : t('chat.context.lesson', { title: b.label }) }}</span>
          <button type="button" class="grid h-4 w-4 place-items-center rounded-full text-muted-foreground hover:bg-muted hover:text-foreground" :aria-label="t('chat.context.remove')" @click="chat.detach(b.id)">×</button>
        </span>
        <button
          v-for="o in offers"
          :key="`${o.course_id}:${o.element_id}`"
          type="button"
          class="inline-flex max-w-full items-center gap-1 rounded-full border border-dashed border-border px-2 py-0.5 text-[11.5px] text-muted-foreground hover:border-muted-foreground hover:text-foreground"
          @click="chat.attach(o)"
        >
          <span aria-hidden="true">+</span><span class="truncate">{{ o.kind === 'goal' ? t('chat.context.attachGoal') : t('chat.context.attachLesson') }} · {{ o.label }}</span>
        </button>
      </div>

      <ChatComposer ref="composer" class="flex-none" @add-connection="addingConnection = true" />

      <ChatReviewSheet
        v-if="chat.prepared.value"
        :prepared="chat.prepared.value"
        :continuing="messages.length > 0"
        :busy="chat.preparing.value"
        @send="chat.confirmReview()"
        @cancel="chat.cancelReview()"
        @fresh="freshChat"
        @toggle-binding="(id) => chat.revise({ binding: id })"
        @toggle-history="(include) => chat.revise({ history: include })"
        @toggle-memory="(id) => chat.revise({ memory: id })"
      />
    </template>
    <ConfirmDialog
      :open="deleting !== null"
      :title="t('chat.panel.delete')"
      :message="t('chat.panel.deleteConfirm')"
      :confirm-label="t('chat.panel.delete')"
      confirm-variant="danger"
      @confirm="confirmDelete"
      @cancel="deleting = null"
    />
  </section>
</template>

<style scoped>
.chat-head-btn { display: grid; place-items: center; width: 1.75rem; height: 1.75rem; flex: none; border-radius: 0.5rem; color: var(--app-muted-foreground); }
.chat-head-btn:hover { background: var(--app-muted); color: var(--app-foreground); }
.chat-head-btn svg { width: 1rem; height: 1rem; }
</style>
