<script setup lang="ts">
// Lays out a call's tiles as equal boxes, as large as the space allows (see
// utils/videoGrid). Each tile is given the size through the slot's
// `tileStyle`; the grid measures itself, so it fills whatever its parent
// gives it and re-flows when the window or a side panel changes size.
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import { bestGrid } from '@/utils/videoGrid'

const props = withDefaults(defineProps<{
  count: number
  gap?: number
  aspect?: number
}>(), {
  gap: 12,
  aspect: 16 / 9,
})

const area = ref<HTMLElement | null>(null)
const size = ref({ width: 0, height: 0 })
let observer: ResizeObserver | null = null

function measure() {
  const rect = area.value?.getBoundingClientRect()
  if (rect) size.value = { width: rect.width, height: rect.height }
}

onMounted(() => {
  measure()
  if (area.value && typeof ResizeObserver !== 'undefined') {
    observer = new ResizeObserver(measure)
    observer.observe(area.value)
  }
})
onBeforeUnmount(() => observer?.disconnect())

const layout = computed(() => bestGrid(props.count, size.value.width, size.value.height, props.aspect, props.gap))
const tileStyle = computed(() => ({ width: `${layout.value.width}px`, height: `${layout.value.height}px` }))
</script>

<template>
  <div ref="area" class="relative min-h-0 w-full overflow-hidden" data-testid="video-grid">
    <div class="flex h-full w-full flex-wrap content-center items-center justify-center" :style="{ gap: `${gap}px` }">
      <slot :tile-style="tileStyle" />
    </div>
  </div>
</template>
