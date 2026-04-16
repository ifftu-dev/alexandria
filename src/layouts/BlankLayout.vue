<script setup lang="ts">
import { onMounted, onUnmounted } from 'vue'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { usePlatform } from '@/composables/usePlatform'

const { isMobilePlatform } = usePlatform()

onMounted(() => {
  if (!isMobilePlatform) {
    document.addEventListener('mousedown', onBlankMouseDown)
  }
})

onUnmounted(() => {
  document.removeEventListener('mousedown', onBlankMouseDown)
})

async function onBlankMouseDown(e: MouseEvent) {
  if (e.button !== 0) return
  const target = e.target as HTMLElement | null
  if (!target) return
  if (target.closest('button, input, textarea, select, a, [role="option"]')) return
  // Skip scrollbar area
  const scrollable = target.closest('.overflow-y-auto') as HTMLElement | null
  if (scrollable) {
    const rect = scrollable.getBoundingClientRect()
    if (e.clientX > rect.right - 16) return
  }
  const inTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window
  if (!inTauri) return
  try {
    await getCurrentWindow().startDragging()
  } catch { /* non-critical */ }
}
</script>

<template>
  <div class="h-full overflow-y-auto bg-background safe-area-top safe-area-bottom safe-area-lr">
    <slot />
  </div>
</template>
