<script setup lang="ts">
import { ref, onMounted, onUnmounted, useId } from 'vue'

withDefaults(defineProps<{
  /** Tooltip body. Use the default slot instead for rich content. */
  text?: string
  /** Accessible name for the trigger button. */
  label?: string
  /** Popover side relative to the icon. */
  placement?: 'top' | 'bottom'
}>(), {
  text: '',
  label: 'More information',
  placement: 'top',
})

// Click-to-toggle rather than hover-only: hover popovers are unreachable on
// touch (the app ships to iOS via WKWebView), so the trigger must respond to
// tap. Hover still previews on pointer devices via CSS.
const open = ref(false)
const root = ref<HTMLElement | null>(null)
const popId = useId()

function toggle() {
  open.value = !open.value
}

function onOutside(e: MouseEvent) {
  if (root.value && !root.value.contains(e.target as Node)) open.value = false
}

function onKey(e: KeyboardEvent) {
  if (e.key === 'Escape') open.value = false
}

onMounted(() => {
  document.addEventListener('click', onOutside)
  document.addEventListener('keydown', onKey)
})
onUnmounted(() => {
  document.removeEventListener('click', onOutside)
  document.removeEventListener('keydown', onKey)
})
</script>

<template>
  <span ref="root" class="infotip">
    <span class="infotip-anchor">
      <button
        type="button"
        class="infotip-trigger"
        :class="{ 'infotip-trigger--active': open }"
        :aria-label="label"
        :aria-expanded="open"
        :aria-describedby="popId"
        @click.stop="toggle"
      >
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true">
          <circle cx="12" cy="12" r="9" />
          <path stroke-linecap="round" stroke-linejoin="round" d="M12 11v5" />
          <path stroke-linecap="round" d="M12 8h.01" />
        </svg>
      </button>

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
        >
          <slot>{{ text }}</slot>
        </span>
      </Transition>
    </span>
  </span>
</template>

<style scoped>
/* No position here so an external `absolute`/`relative` utility class can
   place the icon (e.g. a card corner). The popover anchors to the inner
   .infotip-anchor instead. */
.infotip {
  display: inline-flex;
  vertical-align: middle;
  line-height: 0;
}

.infotip-anchor {
  position: relative;
  display: inline-flex;
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

.infotip-pop {
  position: absolute;
  left: 50%;
  z-index: 50;
  width: max-content;
  max-width: 16rem;
  transform: translateX(-50%);
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

.infotip-pop--top {
  bottom: calc(100% + 0.375rem);
}

.infotip-pop--bottom {
  top: calc(100% + 0.375rem);
}

/* Hover preview on pointer devices; touch relies on the click toggle above. */
@media (hover: hover) {
  .infotip:hover .infotip-pop {
    opacity: 1;
  }
}
</style>
