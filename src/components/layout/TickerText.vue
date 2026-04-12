<script setup lang="ts">
import { ref, watch, nextTick, onMounted, onBeforeUnmount } from 'vue'

// Renders a single line of text that scrolls horizontally (ticker-style)
// when it would otherwise overflow its container. Short text sits still.
//
// Uses ResizeObserver to react to container width changes (sidebar
// collapse/expand, window resize, font loading) without polling.

const props = defineProps<{
  text: string
  /** Seconds per full scroll cycle. Default 12s feels readable. */
  speed?: number
}>()

const wrapEl = ref<HTMLElement | null>(null)
const measureEl = ref<HTMLElement | null>(null)
const overflowing = ref(false)

let observer: ResizeObserver | null = null

async function measure() {
  await nextTick()
  const wrap = wrapEl.value
  const measure = measureEl.value
  if (!wrap || !measure) return
  // +1 to guard against sub-pixel rounding flicker
  overflowing.value = measure.scrollWidth > wrap.clientWidth + 1
}

onMounted(() => {
  measure()
  observer = new ResizeObserver(() => measure())
  if (wrapEl.value) observer.observe(wrapEl.value)
})

onBeforeUnmount(() => {
  observer?.disconnect()
  observer = null
})

// Re-measure when the text changes — rename, locale change, etc.
watch(() => props.text, () => measure())
</script>

<template>
  <span
    ref="wrapEl"
    :class="['ticker-wrap', { 'is-overflowing': overflowing }]"
  >
    <span class="ticker-track" :style="overflowing && speed ? { animationDuration: speed + 's' } : undefined">
      <span ref="measureEl" class="ticker-chunk">{{ text }}</span>
      <span v-if="overflowing" aria-hidden="true" class="ticker-chunk ticker-dup">{{ text }}</span>
    </span>
  </span>
</template>

<style scoped>
.ticker-wrap {
  display: block;
  overflow: hidden;
  max-width: 100%;
  min-width: 0;
  white-space: nowrap;
}

/* Fade the trailing edge only when actually scrolling — keeps short
   static labels crisp and avoids dimming their last character. */
.ticker-wrap.is-overflowing {
  mask-image: linear-gradient(to right, black 0%, black calc(100% - 1.25rem), transparent 100%);
  -webkit-mask-image: linear-gradient(to right, black 0%, black calc(100% - 1.25rem), transparent 100%);
}

.ticker-track {
  display: inline-flex;
  align-items: baseline;
  white-space: nowrap;
}

/* Gap between the two copies when ticking, so the loop doesn't feel
   cramped. Applied only in the overflowing state. */
.ticker-wrap.is-overflowing .ticker-chunk {
  padding-right: 2rem;
}

/* Seamless loop: translateX by exactly one copy's width (50% of track). */
.ticker-wrap.is-overflowing .ticker-track {
  animation: ticker-slide 12s linear infinite;
}

@keyframes ticker-slide {
  0%   { transform: translateX(0); }
  100% { transform: translateX(-50%); }
}

@media (prefers-reduced-motion: reduce) {
  .ticker-wrap.is-overflowing .ticker-track {
    animation: none;
  }
  .ticker-wrap.is-overflowing {
    /* Fall back to ellipsis when motion is reduced. */
    mask-image: none;
    -webkit-mask-image: none;
    text-overflow: ellipsis;
  }
}
</style>
