<script setup lang="ts">
import { onBeforeUnmount, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useRoute } from 'vue-router'
import { useChat } from '@/composables/useChat'
import { useChatAvailability } from '@/composables/useChatAvailability'
import { useKeyboardShortcuts } from '@/composables/useKeyboardShortcuts'
import ChatPanel from './ChatPanel.vue'

/**
 * Mounts chat once in the app shell, outside the routed page, so the thread,
 * the draft, and a response that is still writing survive navigation.
 *
 * Desktop: a resizable panel docked beside the page, or an expanded
 * workspace. Narrow windows and phones: a button that opens a sheet. While
 * an assessment is in progress nothing here is mounted at all.
 */

const WIDTH_KEY = 'alexandria.chat.panelWidth'
const { t } = useI18n()
const route = useRoute()
const chat = useChat()
const { restricted } = useChatAvailability()
const { registerAction } = useKeyboardShortcuts()

const width = ref(390)
try {
  const stored = Number(localStorage.getItem(WIDTH_KEY))
  if (stored >= 320 && stored <= 720) width.value = stored
} catch {
  /* presentation preference only */
}

registerAction('toggle-chat', () => {
  if (!restricted.value) chat.toggle()
})

// One panel instance at a time: docked or expanded on wide windows, a sheet
// on narrow ones. Matches Tailwind's `md` breakpoint.
const wideQuery = window.matchMedia('(min-width: 768px)')
const wide = ref(wideQuery.matches)
const onWide = (event: MediaQueryListEvent) => (wide.value = event.matches)
wideQuery.addEventListener('change', onWide)
onBeforeUnmount(() => wideQuery.removeEventListener('change', onWide))

// Page changes can start an assessment elsewhere in the app; check again.
watch(
  () => route.fullPath,
  () => {
    if (chat.panelOpen.value) void chat.refreshStatus().catch(() => {})
  },
)

let dragStart = 0
let widthStart = 0
function startResize(event: PointerEvent) {
  dragStart = event.clientX
  widthStart = width.value
  const target = event.currentTarget as HTMLElement
  target.setPointerCapture(event.pointerId)
}
function resize(event: PointerEvent) {
  if (!(event.currentTarget as HTMLElement).hasPointerCapture(event.pointerId)) return
  const rtl = document.documentElement.dir === 'rtl'
  const delta = (dragStart - event.clientX) * (rtl ? -1 : 1)
  width.value = Math.max(320, Math.min(720, widthStart + delta))
}
function endResize() {
  try {
    localStorage.setItem(WIDTH_KEY, String(width.value))
  } catch {
    /* ignore */
  }
}
function resizeByKey(event: KeyboardEvent) {
  const step = event.key === 'ArrowLeft' ? 24 : event.key === 'ArrowRight' ? -24 : 0
  if (!step) return
  event.preventDefault()
  width.value = Math.max(320, Math.min(720, width.value + step))
  endResize()
}
</script>

<template>
  <template v-if="!restricted">
    <!-- Docked, desktop -->
    <aside
      v-if="wide && chat.panelOpen.value && !chat.expanded.value"
      class="relative flex h-full flex-none border-s border-border"
      :style="{ width: `${width}px` }"
    >
      <div
        class="absolute inset-y-0 -start-1 z-10 w-2 cursor-col-resize focus:outline-none focus-visible:bg-ring/30"
        role="separator"
        aria-orientation="vertical"
        :aria-valuenow="width"
        aria-valuemin="320"
        aria-valuemax="720"
        tabindex="0"
        @pointerdown="startResize"
        @pointermove="resize"
        @pointerup="endResize"
        @keydown="resizeByKey"
      />
      <ChatPanel class="w-full" variant="docked" @close="chat.setOpen(false)" />
    </aside>

    <!-- Expanded workspace, desktop -->
    <div
      v-if="wide && chat.panelOpen.value && chat.expanded.value"
      class="fixed inset-x-0 bottom-0 top-12 z-30 flex justify-center bg-background"
    >
      <ChatPanel class="w-full" variant="expanded" @close="chat.setOpen(false)" />
    </div>

    <!-- Narrow windows and phones -->
    <button
      v-if="!wide && !chat.panelOpen.value"
      type="button"
      class="fixed bottom-36 end-4 z-40 grid h-12 w-12 place-items-center rounded-full bg-primary text-primary-foreground shadow-lg"
      :aria-label="t('chat.panel.open')"
      @click="chat.setOpen(true)"
    >
      <svg class="h-5 w-5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"><path d="M4 5h16v11H8l-4 4z" /></svg>
    </button>
    <div v-if="!wide && chat.panelOpen.value" class="fixed inset-0 z-50 flex flex-col justify-end bg-black/40" @click.self="chat.setOpen(false)">
      <div class="flex h-[94%] flex-col overflow-hidden rounded-t-[1.75rem] bg-background shadow-2xl">
        <div class="mx-auto mt-2 h-1.5 w-10 flex-none rounded-full bg-border" aria-hidden="true" />
        <ChatPanel class="min-h-0 flex-1" variant="sheet" @close="chat.setOpen(false)" />
      </div>
    </div>
  </template>
</template>
