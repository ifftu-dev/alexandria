<script setup lang="ts">
import { ref, useId } from 'vue'
import { useModalFocus } from '@/composables/useModalFocus'

interface Props {
  open: boolean
  title?: string
  mobileFullscreen?: boolean
  maxWidth?: string
}

const props = withDefaults(defineProps<Props>(), {
  title: '',
  maxWidth: '32rem',
})

const emit = defineEmits<{ close: [] }>()

const dialog = ref<HTMLElement | null>(null)
const titleId = useId()
const layer = useModalFocus(() => props.open, dialog, () => emit('close'))

function onBackdropClick(e: MouseEvent) {
  if (e.target === e.currentTarget) {
    emit('close')
  }
}

</script>

<template>
  <Teleport to="body">
    <Transition name="modal-surface"
      @before-leave="(el: Element) => el.setAttribute('inert', '')"
      @before-enter="(el: Element) => el.removeAttribute('inert')">
      <div
        v-if="open"
        data-backdrop
        data-modal-layer
        :style="{ '--modal-depth': layer }"
        :class="{ 'modal-fullscreen': mobileFullscreen }"
        class="modal-backdrop fixed inset-0 flex items-center justify-center p-4"
        @click="onBackdropClick"
      >
          <div
            v-if="open"
            ref="dialog"
            class="modal-surface-card card p-6 w-full flex flex-col max-h-[calc(100dvh-2rem)]"
            :style="{ maxWidth }"
            role="dialog"
            aria-modal="true"
            :aria-labelledby="title || $slots.header ? titleId : undefined"
            tabindex="-1"
          >
            <div v-if="title || $slots.header" class="flex shrink-0 items-center justify-between gap-3 mb-4">
              <div :id="titleId" class="min-w-0 break-words">
                <slot name="header">
                  <h2 class="text-base font-semibold">{{ title }}</h2>
                </slot>
              </div>
              <button
                :data-modal-initial-focus="mobileFullscreen || undefined"
                class="modal-close pressable shrink-0 p-2 rounded-md text-muted-foreground hover:bg-muted/50 *:pointer-events-none"
                @click="emit('close')"
                :aria-label="$t('common.actions.close')"
              >
                <svg class="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                  <path stroke-linecap="round" stroke-linejoin="round" d="M6 18L18 6M6 6l12 12" />
                </svg>
              </button>
            </div>

            <div class="min-h-0 overflow-y-auto overscroll-contain">
              <slot />
            </div>

            <div v-if="$slots.footer" class="shrink-0 mt-4 pt-4 border-t border-border">
              <slot name="footer" />
            </div>
          </div>
      </div>
    </Transition>
  </Teleport>
</template>

<style scoped>
.modal-backdrop { background: rgb(8 12 22 / 58%); }
.modal-surface-card { box-shadow: var(--shadow-lg); }
.modal-close { min-width: 44px; min-height: 44px; display: grid; place-items: center; }
@media (max-width: 700px), (pointer: coarse) and (max-height: 500px) and (max-width: 950px) {
  .modal-fullscreen { padding: 0; }
  .modal-fullscreen .modal-surface-card {
    height: min(100dvh, var(--app-height, 100dvh));
    max-height: 100%;
    padding: max(6px, var(--sat, env(safe-area-inset-top))) max(12px, var(--sar, env(safe-area-inset-right))) max(8px, var(--sab, env(safe-area-inset-bottom))) max(12px, var(--sal, env(safe-area-inset-left)));
    border: 0;
    border-radius: 0;
  }
}
.modal-surface-enter-active { transition: opacity var(--transition-base); }
.modal-surface-leave-active { transition: opacity var(--transition-fast); pointer-events: none; }
.modal-surface-enter-active .modal-surface-card,
.modal-surface-leave-active .modal-surface-card { transition: transform var(--transition-base); }
.modal-surface-enter-from, .modal-surface-leave-to { opacity: 0; }
.modal-surface-enter-from .modal-surface-card { transform: translateY(10px) scale(0.98); }
.modal-surface-leave-to .modal-surface-card { transform: translateY(4px) scale(0.99); }
</style>
