<script setup lang="ts">
import { ref, watch, onMounted, onBeforeUnmount, useId } from 'vue'
defineOptions({ inheritAttrs: false })
const props = defineProps<{ text: string }>()
const wrapEl = ref<HTMLElement | null>(null)
const tooltip = ref(false)
const position = ref({ top: 0, left: 0 })
const tooltipId = useId()
let trigger: HTMLElement | null = null
function show() {
  if (!trigger) return
  tooltip.value = true
  const rect = trigger.getBoundingClientRect()
  position.value = { top: Math.min(rect.bottom + 6, window.innerHeight - 80), left: Math.max(8, Math.min(rect.left, window.innerWidth - 296)) }
}
function hide() { tooltip.value = false }
function key(event: KeyboardEvent) { if (event.key === 'Escape') hide() }
onMounted(() => {
  trigger = wrapEl.value?.closest('button, a') ?? wrapEl.value
  trigger?.addEventListener('mouseenter', show)
  trigger?.addEventListener('mouseleave', hide)
  trigger?.addEventListener('focus', show)
  trigger?.addEventListener('blur', hide)
  trigger?.addEventListener('keydown', key)
  trigger?.setAttribute('aria-describedby', tooltipId)
  window.addEventListener('scroll', hide, true)
})
onBeforeUnmount(() => {
  trigger?.removeEventListener('mouseenter', show)
  trigger?.removeEventListener('mouseleave', hide)
  trigger?.removeEventListener('focus', show)
  trigger?.removeEventListener('blur', hide)
  trigger?.removeEventListener('keydown', key)
  trigger?.removeAttribute('aria-describedby')
  window.removeEventListener('scroll', hide, true)
})
watch(() => props.text, hide)
</script>
<template>
  <span v-bind="$attrs" ref="wrapEl" class="ticker-wrap">
    <span class="ticker-track">{{ text }}</span>
  </span>
  <Teleport to="body"><span v-if="tooltip" :id="tooltipId" role="tooltip" class="ticker-tooltip" :style="{ top: `${position.top}px`, left: `${position.left}px` }">{{ text }}</span></Teleport>
</template>
<style scoped>
.ticker-wrap { display:block; overflow:hidden; max-width:100%; min-width:0; white-space:nowrap; }
.ticker-track { display:block; overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }
.ticker-tooltip { position:fixed; z-index:100; max-width:280px; pointer-events:none; white-space:normal; overflow-wrap:anywhere; border:1px solid var(--app-border); border-radius:8px; padding:8px 12px; font-size:12px; line-height:1.5; background:var(--app-card); color:var(--app-foreground); box-shadow:0 8px 24px #0003; }
</style>
