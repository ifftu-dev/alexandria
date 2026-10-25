<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useChat } from '@/composables/useChat'
import { useAccountStatus } from '@/composables/useAccountStatus'
import { useProviderSignIn } from '@/composables/useProviderSignIn'
import { chatOffers } from '@/composables/useChatOffers'
import { CHAT_LOCATION_OF_CONNECTION, type ChatLocation, type ConnectionLocation } from '@/generated/chat-vocab'
import type { StudioConnection, StudioDocument } from '@/types'

/**
 * The message box and its one row of tools: attachments ("+"), the mode,
 * the model, and send. Everything else lives in the menus they open.
 */
defineProps<{ variant?: 'docked' | 'expanded' | 'sheet' }>()
const emit = defineEmits<{ addConnection: [] }>()
const { t } = useI18n()
const chat = useChat()
const { isMinor } = useAccountStatus()
const input = ref<HTMLTextAreaElement | null>(null)
type Menu = 'attach' | 'mode' | 'model'
const openMenu = ref<Menu | null>(null)
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

const modes = computed(() => [
  { value: 'ask' as const, label: t('chat.modes.ask'), hint: t('chat.modes.askHint') },
  { value: 'learn' as const, label: t('chat.modes.learn'), hint: t('chat.modes.learnHint') },
  { value: 'practice' as const, label: t('chat.modes.practice'), hint: t('chat.modes.practiceHint') },
])
const modeLabel = computed(() => modes.value.find((m) => m.value === chat.mode.value)?.label ?? '')

const bindings = computed(() => chat.detail.value?.bindings ?? [])
/** Offers from the page that are not attached yet. */
const offers = computed(() =>
  chatOffers.value.filter(
    (o) => !bindings.value.some((b) => b.course_id === o.course_id && b.element_id === o.element_id),
  ),
)

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

/** The composer lists only connections chat can use, so every one has a chat location. */
function locationOf(value: ConnectionLocation | undefined): ChatLocation {
  return (value && CHAT_LOCATION_OF_CONNECTION[value]) ?? 'device'
}
const location = computed(() => locationOf(chat.selectedConnection.value?.value.location))
function hostOf(endpoint: string) {
  try {
    return new URL(endpoint).host
  } catch {
    return endpoint
  }
}
const destination = computed(() => {
  const c = chat.selectedConnection.value
  return c ? `${t(`chat.location.${location.value}`)} · ${hostOf(c.value.endpoint)}` : ''
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
  el.style.height = `${Math.min(el.scrollHeight, 200)}px`
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
  openMenu.value = null
  await chat.selectModel(connectionId, modelId)
}

function toggle(menu: Menu) {
  openMenu.value = openMenu.value === menu ? null : menu
}

function closeOnOutside(event: MouseEvent) {
  if (!(event.target as HTMLElement).closest('[data-chat-menu]')) openMenu.value = null
}
function closeOnEscape(event: KeyboardEvent) {
  if (event.key === 'Escape') openMenu.value = null
}
watch(openMenu, (open) => {
  if (open === 'model' && isMinor.value) void chat.refreshApprovals().catch(() => {})
  if (open) {
    document.addEventListener('mousedown', closeOnOutside)
    document.addEventListener('keydown', closeOnEscape)
  } else {
    document.removeEventListener('mousedown', closeOnOutside)
    document.removeEventListener('keydown', closeOnEscape)
    removing.value = null
  }
})
onBeforeUnmount(() => {
  document.removeEventListener('mousedown', closeOnOutside)
  document.removeEventListener('keydown', closeOnEscape)
})

defineExpose({ focus: () => input.value?.focus() })
</script>

<template>
  <div class="composer" :class="{ 'composer--sheet': variant === 'sheet' }">
    <div class="composer-box">
      <textarea
        ref="input"
        :value="chat.draft.value"
        rows="1"
        class="composer-input"
        :placeholder="placeholder"
        :aria-label="placeholder"
        @input="chat.setDraft(($event.target as HTMLTextAreaElement).value)"
        @keydown="onKeydown"
      />
      <div class="composer-tools relative" data-chat-menu>
        <!-- Attachments: lessons and goals the page offers, and what is attached -->
        <div>
          <button
            type="button"
            class="composer-plus"
            :class="{ 'composer-plus--on': bindings.length }"
            :aria-expanded="openMenu === 'attach'"
            :aria-label="t('chat.composer.attach')"
            :title="t('chat.composer.attach')"
            @click="toggle('attach')"
          >
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M12 5v14M5 12h14" /></svg>
            <span v-if="bindings.length" class="composer-badge">{{ bindings.length }}</span>
          </button>
          <div v-if="openMenu === 'attach'" class="composer-menu" role="menu">
            <p class="composer-menu-head">{{ t('chat.context.label') }}</p>
            <p v-if="!bindings.length && !offers.length" class="composer-menu-empty">{{ t('chat.composer.nothingToAttach') }}</p>
            <div v-for="b in bindings" :key="b.id" class="composer-item">
              <svg class="composer-item-ic" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8"><path d="M6 3h9l4 4v14H6z" /><path d="M14 3v5h5" /></svg>
              <span class="min-w-0 flex-1 truncate">{{ b.kind === 'goal' ? t('chat.context.goal', { title: b.label }) : t('chat.context.lesson', { title: b.label }) }}</span>
              <button type="button" class="composer-item-x" :title="t('chat.context.removeHint')" :aria-label="t('chat.context.remove')" @click="chat.detach(b.id)">×</button>
            </div>
            <button
              v-for="o in offers"
              :key="`${o.course_id}:${o.element_id}`"
              type="button"
              role="menuitem"
              class="composer-item composer-item--action"
              @click="chat.attach(o); openMenu = null"
            >
              <span class="composer-item-ic text-primary" aria-hidden="true">+</span>
              <span class="min-w-0 flex-1 truncate">{{ o.kind === 'goal' ? t('chat.context.attachGoal') : t('chat.context.attachLesson') }} · {{ o.label }}</span>
            </button>
          </div>
        </div>

        <!-- Mode -->
        <div>
          <button
            type="button"
            class="composer-pill"
            :aria-expanded="openMenu === 'mode'"
            aria-haspopup="menu"
            :aria-label="t('chat.composer.mode', { mode: modeLabel })"
            :disabled="chat.streaming.value"
            @click="toggle('mode')"
          >
            <b>{{ modeLabel }}</b>
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4"><path d="M6 9l6 6 6-6" /></svg>
          </button>
          <div v-if="openMenu === 'mode'" class="composer-menu" role="menu">
            <button
              v-for="m in modes"
              :key="m.value"
              type="button"
              role="menuitemradio"
              class="composer-item composer-item--action composer-item--two"
              :aria-checked="chat.mode.value === m.value"
              @click="chat.setMode(m.value); openMenu = null"
            >
              <span class="composer-check" aria-hidden="true">{{ chat.mode.value === m.value ? '✓' : '' }}</span>
              <span class="min-w-0 flex-1"><b>{{ m.label }}</b><small>{{ m.hint }}</small></span>
            </button>
          </div>
        </div>

        <!-- Model -->
        <div class="min-w-0">
          <button
            type="button"
            class="composer-pill min-w-0"
            :aria-expanded="openMenu === 'model'"
            aria-haspopup="listbox"
            :title="destination ? t('chat.composer.sendsTo', { destination }) : undefined"
            @click="toggle('model')"
          >
            <span v-if="chat.selectedConnection.value" class="chat-loc" :class="`chat-loc--${location}`" aria-hidden="true" />
            <b class="truncate">{{ chat.selectedModel.value ?? t('chat.composer.chooseModel') }}</b>
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4"><path d="M6 9l6 6 6-6" /></svg>
          </button>
          <div v-if="openMenu === 'model'" class="composer-menu composer-menu--wide" role="listbox">
            <p v-if="destination" class="composer-menu-head">{{ t('chat.composer.sendsTo', { destination }) }}</p>
            <template v-for="c in chat.connections.value" :key="c.id">
              <button
                type="button"
                role="option"
                class="composer-item composer-item--action composer-item--two"
                :aria-selected="chat.selectedConnectionId.value === c.id"
                @click="choose(c.id, null)"
              >
                <span class="chat-loc mt-1.5" :class="`chat-loc--${locationOf(c.value.location)}`" aria-hidden="true" />
                <span class="min-w-0 flex-1">
                  <b class="truncate">{{ c.value.name }}</b>
                  <small class="truncate">{{ c.value.model }} · {{ t(`chat.location.${locationOf(c.value.location)}`) }}</small>
                  <small
                    v-if="isMinor && c.value.location === 'cloud'"
                    :class="chat.approvalFor(c.id, c.value.endpoint)?.status === 'approved' ? 'text-success' : 'text-warning'"
                  >{{ t(`chat.guardian.status.${chat.approvalFor(c.id, c.value.endpoint)?.status ?? 'none'}`) }}</small>
                </span>
              </button>
              <div v-if="chat.selectedConnectionId.value === c.id" class="composer-sub">
                <button v-if="modelsFor !== c.id" type="button" class="composer-sublink" @click="showModels(c.id)">{{ t('chat.composer.listModels') }}</button>
                <p v-else-if="loadingModels" class="composer-subtext">{{ t('chat.composer.loadingModels') }}</p>
                <p v-else-if="modelsError" class="composer-subtext">{{ modelsError }}</p>
                <button
                  v-for="m in models ?? []"
                  :key="m"
                  type="button"
                  role="option"
                  class="composer-subitem"
                  :aria-selected="chat.selectedModel.value === m"
                  @click="choose(c.id, m)"
                >{{ m }}</button>
                <button type="button" class="composer-sublink !text-muted-foreground hover:!text-error" @click="remove(c)">
                  {{ removing === c.id ? t('chat.signin.removeConfirm') : t('chat.signin.remove') }}
                </button>
              </div>
            </template>
            <button
              type="button"
              class="composer-item composer-item--action composer-add"
              @click="openMenu = null; emit('addConnection')"
            >+ {{ t('chat.composer.addConnection') }}</button>
          </div>
        </div>

        <span class="flex-1" />
        <button
          v-if="chat.streaming.value"
          type="button"
          class="composer-send composer-send--stop"
          :aria-label="t('chat.composer.stop')"
          :title="t('chat.composer.stop')"
          @click="chat.stop()"
        >
          <svg viewBox="0 0 24 24" fill="currentColor"><rect x="7" y="7" width="10" height="10" rx="2" /></svg>
        </button>
        <button
          v-else
          type="button"
          class="composer-send"
          :disabled="!canSend || busy"
          :aria-label="chat.preparing.value ? t('chat.composer.preparing') : t('chat.composer.send')"
          :title="t('chat.composer.send')"
          @click="chat.send()"
        >
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.3" stroke-linecap="round" stroke-linejoin="round"><path d="M12 19V5M5 12l7-7 7 7" /></svg>
        </button>
      </div>
    </div>
    <p v-if="usingPlan" class="composer-meta">
      {{ t('chat.signin.usingPlan') }}
      <span aria-hidden="true">·</span>
      <button type="button" class="text-primary hover:underline" @click="signIn.openPage('chatgpt_usage')">{{ t('chat.signin.manageUsage') }}</button>
    </p>
    <p v-if="removedNote" class="composer-meta" role="status">
      {{ removedNote === 'chatgpt' ? t('chat.signin.removedChatgpt') : t('chat.signin.removedOpenrouter') }}
      <button
        v-if="removedNote === 'openrouter'"
        type="button"
        class="text-primary hover:underline"
        @click="signIn.openPage('openrouter_keys')"
      >{{ t('chat.signin.openKeys') }}</button>
    </p>
  </div>
</template>

<style scoped>
.composer { padding: 0.5rem 0.75rem 0.75rem; }
.composer--sheet { padding-bottom: calc(0.75rem + env(safe-area-inset-bottom, 0px)); }
.composer-box { border: 1px solid var(--app-border); background: var(--app-card); border-radius: 1.25rem; padding: 0.75rem 0.625rem 0.5rem 0.875rem; transition: border-color 0.15s, box-shadow 0.15s; }
.composer-box:focus-within { border-color: color-mix(in srgb, var(--app-primary) 60%, var(--app-border)); box-shadow: 0 0 0 3px color-mix(in srgb, var(--app-primary) 14%, transparent); }
.composer-input { display: block; width: 100%; max-height: 12.5rem; resize: none; background: transparent; font-size: 0.875rem; line-height: 1.55; color: var(--app-foreground); outline: none; }
.composer--sheet .composer-input { font-size: 0.9375rem; }
.composer-input::placeholder { color: var(--app-muted-foreground); }
.composer-tools { display: flex; align-items: center; gap: 0.375rem; margin-top: 0.625rem; }
.composer-plus { position: relative; display: grid; place-items: center; width: 1.875rem; height: 1.875rem; flex: none; border-radius: 999px; border: 1px solid var(--app-border); color: var(--app-muted-foreground); }
.composer--sheet .composer-plus { width: 2.25rem; height: 2.25rem; }
.composer-plus:hover, .composer-plus--on { color: var(--app-foreground); border-color: color-mix(in srgb, var(--app-primary) 50%, var(--app-border)); }
.composer-plus svg { width: 0.9375rem; height: 0.9375rem; }
.composer-badge { position: absolute; top: -0.3rem; inset-inline-end: -0.3rem; min-width: 1rem; height: 1rem; padding: 0 0.25rem; border-radius: 999px; background: var(--app-primary); color: var(--app-primary-foreground); font-size: 0.625rem; font-weight: 600; line-height: 1rem; text-align: center; }
.composer-pill { display: inline-flex; align-items: center; gap: 0.375rem; max-width: 13rem; height: 1.875rem; padding: 0 0.625rem; border-radius: 999px; background: color-mix(in srgb, var(--app-foreground) 7%, transparent); font-size: 0.78125rem; color: var(--app-muted-foreground); white-space: nowrap; }
.composer--sheet .composer-pill { height: 2.25rem; font-size: 0.84375rem; }
.composer-pill:hover:not(:disabled) { color: var(--app-foreground); }
.composer-pill:disabled { opacity: 0.55; }
.composer-pill b { font-weight: 500; color: var(--app-foreground); }
.composer-pill svg { width: 0.75rem; height: 0.75rem; flex: none; }
.composer-send { display: grid; place-items: center; width: 2.125rem; height: 2.125rem; flex: none; border-radius: 999px; background: var(--app-primary); color: var(--app-primary-foreground); }
.composer--sheet .composer-send { width: 2.5rem; height: 2.5rem; }
.composer-send:disabled { opacity: 0.35; }
.composer-send svg { width: 1.0625rem; height: 1.0625rem; }
.composer-send--stop { background: var(--app-foreground); color: var(--app-background); }
.composer-meta { display: flex; flex-wrap: wrap; align-items: center; gap: 0.375rem; margin: 0.5rem 0.375rem 0; font-size: 0.75rem; color: var(--app-muted-foreground); }
.composer-menu { position: absolute; bottom: calc(100% + 0.5rem); inset-inline-start: 0; z-index: 30; width: min(16.5rem, 100%); max-height: 20rem; overflow-y: auto; padding: 0.375rem; border-radius: 0.875rem; border: 1px solid var(--app-border); background: var(--app-card); box-shadow: 0 12px 32px rgb(0 0 0 / 0.22); font-size: 0.8125rem; }
.composer-menu--wide { width: min(19rem, 100%); }
.composer-menu-head { margin: 0; padding: 0.375rem 0.5rem 0.25rem; font-size: 0.71875rem; color: var(--app-muted-foreground); }
.composer-menu-empty { margin: 0; padding: 0.5rem; color: var(--app-muted-foreground); }
.composer-item { display: flex; width: 100%; align-items: center; gap: 0.5rem; padding: 0.5rem; border-radius: 0.5rem; text-align: start; color: var(--app-foreground); }
.composer-item--action:hover { background: color-mix(in srgb, var(--app-foreground) 7%, transparent); }
.composer-item--two { align-items: flex-start; }
.composer-item b { display: block; font-weight: 500; }
.composer-item small { display: block; font-size: 0.75rem; color: var(--app-muted-foreground); }
.composer-item-ic { width: 0.875rem; height: 0.875rem; flex: none; display: grid; place-items: center; font-weight: 600; }
.composer-item-x { display: grid; place-items: center; width: 1.5rem; height: 1.5rem; border-radius: 999px; color: var(--app-muted-foreground); }
.composer-item-x:hover { background: var(--app-muted); color: var(--app-foreground); }
.composer-check { width: 0.875rem; flex: none; color: var(--app-primary); font-weight: 700; }
.composer-sub { margin: 0 0 0.25rem 1.375rem; padding-inline-start: 0.5rem; border-inline-start: 1px solid var(--app-border); }
.composer-sublink { display: block; padding: 0.3125rem 0.5rem; font-size: 0.75rem; color: var(--app-primary); }
.composer-sublink:hover { text-decoration: underline; }
.composer-subtext { margin: 0; padding: 0.3125rem 0.5rem; font-size: 0.75rem; color: var(--app-muted-foreground); }
.composer-subitem { display: block; width: 100%; padding: 0.3125rem 0.5rem; border-radius: 0.375rem; text-align: start; font-size: 0.78125rem; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.composer-subitem:hover, .composer-subitem[aria-selected="true"] { background: var(--app-muted); }
.composer-add { margin-top: 0.25rem; border-top: 1px solid var(--app-border); border-radius: 0 0 0.5rem 0.5rem; color: var(--app-primary); font-weight: 500; }
.chat-loc { display: inline-block; width: 7px; height: 7px; flex: none; border-radius: 999px; }
.chat-loc--device { background: var(--app-success); }
.chat-loc--private_network { background: var(--app-warning); }
.chat-loc--remote { background: var(--app-primary); }
</style>
