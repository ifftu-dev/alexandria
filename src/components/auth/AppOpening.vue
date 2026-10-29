<script setup lang="ts">
import { onMounted, onBeforeUnmount, ref, watch } from 'vue'
import { useMotion } from '@/composables/useMotion'
import BrandMark from './BrandMark.vue'

const props = defineProps<{ ready: boolean }>()
const { reduced } = useMotion()
const visible = ref(!reduced.value && !document.hidden)
const position = ref<Record<string, string>>({ left: 'calc(50% - 48px)', top: 'calc(50% - 64px)', width: '96px', height: '96px' })
let deadline: ReturnType<typeof setTimeout> | undefined
let target: HTMLElement | null = null
function finish(): void {
  visible.value = false
  if (deadline !== undefined) clearTimeout(deadline)
  target?.removeAttribute('data-launch-arriving')
  target = null
}
function dock(): void {
  if (!props.ready || !visible.value) return
  target = document.querySelector<HTMLElement>('[data-launch-target]')
  const bounds = target?.getBoundingClientRect()
  if (!bounds || bounds.width === 0 || bounds.height === 0 || bounds.bottom < 0 || bounds.top >= innerHeight) { finish(); return }
  // Ready UI accepts input now. Only the mark's local assembly continues;
  // no flight across usable controls and no animation promise gates routing.
  position.value = { left: `${bounds.left}px`, top: `${bounds.top}px`, width: `${bounds.width}px`, height: `${bounds.height}px` }
  target?.setAttribute('data-launch-arriving', '')
}
watch(() => props.ready, dock, { flush: 'post' })
watch(reduced, value => { if (value) finish() })
onMounted(() => {
  if (!visible.value) return
  deadline = setTimeout(finish, 1200)
  dock()
  document.addEventListener('pointerdown', finish, { capture: true, passive: true })
  document.addEventListener('keydown', finish, true)
  document.addEventListener('visibilitychange', finish)
  window.addEventListener('resize', finish)
  window.addEventListener('scroll', finish, true)
})
onBeforeUnmount(() => {
  finish()
  document.removeEventListener('pointerdown', finish, true)
  document.removeEventListener('keydown', finish, true)
  document.removeEventListener('visibilitychange', finish)
  window.removeEventListener('resize', finish)
  window.removeEventListener('scroll', finish, true)
})
</script>

<template>
  <div v-if="visible || !ready" data-opening class="app-opening" :style="position" aria-hidden="true" inert>
    <BrandMark :opening="visible" />
  </div>
</template>

<style scoped>
.app-opening { position: fixed; z-index: 40; pointer-events: none; }
:global([data-launch-arriving] > svg) { visibility: hidden; }
</style>
