<script setup lang="ts">
import { watch, onMounted, onUnmounted } from 'vue'

interface Props {
  open: boolean
  title?: string
  maxWidth?: string
}

const props = withDefaults(defineProps<Props>(), {
  title: '',
  maxWidth: '32rem',
})

const emit = defineEmits<{ close: [] }>()

function onKeydown(e: KeyboardEvent) {
  if (e.key === 'Escape') emit('close')
}

function onBackdropClick(e: MouseEvent) {
  if ((e.target as HTMLElement).dataset.backdrop) {
    emit('close')
  }
}

onMounted(() => document.addEventListener('keydown', onKeydown))
onUnmounted(() => document.removeEventListener('keydown', onKeydown))

watch(() => props.open, (val) => {
  document.body.style.overflow = val ? 'hidden' : ''
})
</script>

<template>
  <Teleport to="body">
    <Transition
      enter-active-class="transition duration-200 ease-out"
      enter-from-class="opacity-0"
      enter-to-class="opacity-100"
      leave-active-class="transition duration-150 ease-in"
      leave-from-class="opacity-100"
      leave-to-class="opacity-0"
    >
      <div
        v-if="open"
        data-backdrop
        class="fixed inset-0 z-50 flex items-center justify-center bg-black/50 backdrop-blur-sm p-4"
        @click="onBackdropClick"
      >
        <Transition
          enter-active-class="transition duration-200 ease-out"
          enter-from-class="opacity-0 scale-95"
          enter-to-class="opacity-100 scale-100"
          leave-active-class="transition duration-150 ease-in"
          leave-from-class="opacity-100 scale-100"
          leave-to-class="opacity-0 scale-95"
        >
          <div
            v-if="open"
            class="card p-6 w-full"
            :style="{ maxWidth }"
            role="dialog"
            aria-modal="true"
          >
            <div v-if="title || $slots.header" class="flex items-center justify-between mb-4">
              <slot name="header">
                <h2 class="text-base font-semibold">{{ title }}</h2>
              </slot>
              <button
                class="p-1 rounded-md text-[rgb(var(--color-muted-foreground))] hover:bg-[rgb(var(--color-muted)/0.5)] transition-colors"
                @click="emit('close')"
                aria-label="Close"
              >
                <svg class="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                  <path stroke-linecap="round" stroke-linejoin="round" d="M6 18L18 6M6 6l12 12" />
                </svg>
              </button>
            </div>

            <slot />

            <div v-if="$slots.footer" class="mt-4 pt-4 border-t border-[rgb(var(--color-border))]">
              <slot name="footer" />
            </div>
          </div>
        </Transition>
      </div>
    </Transition>
  </Teleport>
</template>
