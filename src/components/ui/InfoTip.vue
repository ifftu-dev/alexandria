<script setup lang="ts">
import { ref, onMounted, onUnmounted, useId, nextTick } from 'vue'

const props = withDefaults(defineProps<{
  /** Tooltip body. Use the default slot instead for rich content. */
  text?: string
  /** Accessible name for the trigger button. */
  label?: string
  /** Popover side relative to the icon. */
  placement?: 'top' | 'bottom'
}>(), {
  text: '',
  label: '',
  placement: 'top',
})

// Hover-to-reveal (mouseenter/mouseleave) plus keyboard focus/blur, so the tip
// is reachable without a click and stays accessible to keyboard users. The
// popover is teleported to <body> and fixed-positioned from the trigger's
// rect, so it never gets clipped by an ancestor's `overflow` (e.g. a
// scrollable sidebar).
const popId = useId()
const open = ref(false)
const triggerRef = ref<HTMLButtonElement | null>(null)
const pos = ref({ top: 0, left: 0 })

// Keep in sync with `.infotip-pop { max-width }` below.
const MAX_WIDTH = 256
const MARGIN = 8
const GAP = 6

function place() {
  const el = triggerRef.value
  if (!el) return
  const r = el.getBoundingClientRect()
  const half = MAX_WIDTH / 2
  const cx = r.left + r.width / 2
  const left = Math.min(
    Math.max(cx, MARGIN + half),
    window.innerWidth - MARGIN - half,
  )
  const top = props.placement === 'top' ? r.top - GAP : r.bottom + GAP
  pos.value = { top, left }
}

function show() {
  place()
  open.value = true
  void nextTick(place)
}

function hide() {
  open.value = false
}

function onKey(e: KeyboardEvent) {
  if (e.key === 'Escape' && open.value) open.value = false
}

onMounted(() => {
  document.addEventListener('keydown', onKey)
})
onUnmounted(() => {
  document.removeEventListener('keydown', onKey)
})
</script>

<template>
  <span class="infotip">
    <button
      ref="triggerRef"
      type="button"
      class="infotip-trigger"
      :class="{ 'infotip-trigger--active': open }"
      :aria-label="label || $t('common.infoTip.label')"
      :aria-expanded="open"
      :aria-describedby="popId"
      @mouseenter="show"
      @mouseleave="hide"
      @focus="show"
      @blur="hide"
    >
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true">
        <circle cx="12" cy="12" r="9" />
        <path stroke-linecap="round" stroke-linejoin="round" d="M12 11v5" />
        <path stroke-linecap="round" d="M12 8h.01" />
      </svg>
    </button>

    <Teleport to="body">
      <Transition
        enter-active-class="transition duration-100 ease-out"
        enter-from-class="opacity-0 scale-95"
        enter-to-class="opacity-100 scale-100"
        leave-active-class="transition duration-75 ease-in"
        leave-from-class="opacity-100 scale-100"
        leave-to-class="opacity-0 scale-95"
      >
        <span
          v-show="open"
          :id="popId"
          role="tooltip"
          class="infotip-pop"
          :class="placement === 'top' ? 'infotip-pop--top' : 'infotip-pop--bottom'"
          :style="{ top: `${pos.top}px`, left: `${pos.left}px` }"
        >
          <slot>{{ text }}</slot>
        </span>
      </Transition>
    </Teleport>
  </span>
</template>

<style scoped>
.infotip {
  display: inline-flex;
  vertical-align: middle;
  line-height: 0;
}

.infotip-trigger {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 1rem;
  height: 1rem;
  color: var(--app-muted-foreground);
  border-radius: 9999px;
  cursor: pointer;
  transition: color 0.15s;
}

.infotip-trigger svg {
  width: 0.875rem;
  height: 0.875rem;
}

.infotip-trigger:hover,
.infotip-trigger--active {
  color: var(--app-foreground);
}
</style>

<style>
/* Unscoped: the popover is teleported to <body>, so scoped styles wouldn't
   apply. Positioned fixed from the trigger rect; translate centers it and
   flips above/below per placement. */
.infotip-pop {
  position: fixed;
  z-index: 200;
  width: max-content;
  max-width: 16rem;
  padding: 0.5rem 0.625rem;
  border-radius: 0.5rem;
  border: 1px solid var(--app-border);
  background: var(--app-card);
  color: var(--app-foreground);
  font-size: 0.75rem;
  font-weight: 400;
  line-height: 1.4;
  text-align: left;
  white-space: normal;
  box-shadow: 0 8px 24px -8px rgb(0 0 0 / 0.28);
}

.infotip-pop--bottom {
  transform: translateX(-50%);
}

.infotip-pop--top {
  transform: translate(-50%, -100%);
}
</style>
