<script setup lang="ts">
import { ref, computed, watch, nextTick, onMounted, onBeforeUnmount, useId } from 'vue'
defineOptions({ inheritAttrs: false })
const props = defineProps<{ text: string; speed?: number }>()
const wrapEl = ref<HTMLElement | null>(null)
const measureEl = ref<HTMLElement | null>(null)
const distance = ref(0)
const paused = ref(false)
const tooltip = ref(false)
const position = ref({ top: 0, left: 0 })
const tooltipId = useId()
const duration = computed(() => props.speed ?? Math.max(8, distance.value / 24))
let observer: ResizeObserver | null = null
let trigger: HTMLElement | null = null
let disposed = false
async function measure() {
  await nextTick()
  if (disposed || !wrapEl.value || !measureEl.value) return
  const width = measureEl.value.getBoundingClientRect().width
  distance.value = width > wrapEl.value.clientWidth + 1 ? width + 32 : 0
}
function show() {
  if (!trigger) return
  paused.value = true
  tooltip.value = true
  const rect = trigger.getBoundingClientRect()
  position.value = { top: Math.min(rect.bottom + 6, window.innerHeight - 80), left: Math.max(8, Math.min(rect.left, window.innerWidth - 296)) }
}
function hide() { paused.value = false; tooltip.value = false }
function key(event: KeyboardEvent) { if (event.key === 'Escape') hide() }
onMounted(() => {
  trigger = wrapEl.value?.closest('button, a') ?? wrapEl.value
  trigger?.addEventListener('mouseenter', show)
  trigger?.addEventListener('mouseleave', hide)
  trigger?.addEventListener('focus', show)
  trigger?.addEventListener('blur', hide)
  trigger?.addEventListener('keydown', key)
  trigger?.setAttribute('aria-describedby', tooltipId)
  observer = new ResizeObserver(() => { void measure() })
  if (wrapEl.value) observer.observe(wrapEl.value)
  if (measureEl.value) observer.observe(measureEl.value)
  void measure()
  void document.fonts?.ready.then(measure)
  window.addEventListener('scroll', hide, true)
})
onBeforeUnmount(() => {
  disposed = true
  observer?.disconnect()
  trigger?.removeEventListener('mouseenter', show)
  trigger?.removeEventListener('mouseleave', hide)
  trigger?.removeEventListener('focus', show)
  trigger?.removeEventListener('blur', hide)
  trigger?.removeEventListener('keydown', key)
  trigger?.removeAttribute('aria-describedby')
  window.removeEventListener('scroll', hide, true)
})
watch(() => props.text, () => { hide(); void measure() })
</script>
<template>
  <span v-bind="$attrs" ref="wrapEl" class="ticker-wrap" :class="{ 'is-overflowing': distance > 0 }">
    <span class="ticker-track" :style="{ '--ticker-distance': `-${distance}px`, animationDuration: `${duration}s`, animationPlayState: paused ? 'paused' : 'running' }">
      <span ref="measureEl" class="ticker-chunk">{{ text }}</span><span v-if="distance" aria-hidden="true" class="ticker-dup">{{ text }}</span>
    </span>
  </span>
  <Teleport to="body"><span v-if="tooltip" :id="tooltipId" role="tooltip" class="ticker-tooltip" :style="{ top: `${position.top}px`, left: `${position.left}px` }">{{ text }}</span></Teleport>
</template>
<style scoped>
.ticker-wrap { display:block; overflow:hidden; max-width:100%; min-width:0; white-space:nowrap; }
.ticker-wrap.is-overflowing { mask-image:linear-gradient(to right,black 0%,black calc(100% - 1rem),transparent 100%); }
.ticker-track { display:inline-flex; width:max-content; align-items:baseline; white-space:nowrap; }
.ticker-chunk,.ticker-dup { flex:none; }
.ticker-dup { padding-inline-start:32px; }
.is-overflowing .ticker-track { animation:ticker-slide linear infinite; will-change:transform; }
@keyframes ticker-slide { to { transform:translate3d(var(--ticker-distance),0,0); } }
.ticker-tooltip { position:fixed; z-index:100; max-width:280px; pointer-events:none; white-space:normal; overflow-wrap:anywhere; border:1px solid var(--app-border); border-radius:8px; padding:8px 12px; font-size:12px; line-height:1.5; background:var(--app-card); color:var(--app-foreground); box-shadow:0 8px 24px #0003; }
@media(prefers-reduced-motion:reduce) { .is-overflowing .ticker-track { animation:none; display:block; overflow:hidden; text-overflow:ellipsis; width:100%; } .ticker-dup { display:none; } .ticker-wrap.is-overflowing { mask-image:none; } }
</style>
