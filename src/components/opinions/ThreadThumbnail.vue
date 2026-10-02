<script setup lang="ts">
import { ref, watch, onBeforeUnmount } from 'vue'
import { useLocalApi } from '@/composables/useLocalApi'
const props = defineProps<{ cid?: string | null; kind?: string; topic?: string }>()
const { invoke } = useLocalApi()
const url = ref('')
let generation = 0
function clear() { if (url.value) URL.revokeObjectURL(url.value); url.value = '' }
watch(() => props.cid, async cid => {
  const ticket = ++generation
  clear()
  if (!cid) return
  try {
    const bytes = await invoke<number[]>('content_resolve_bytes', { identifier: cid })
    if (ticket !== generation) return
    url.value = URL.createObjectURL(new Blob([new Uint8Array(bytes)]))
  } catch { /* The topic tile remains available offline. */ }
}, { immediate: true })
onBeforeUnmount(() => { generation++; clear() })
</script>
<template>
  <div class="thread-thumb" :data-topic="topic">
    <img v-if="url" :src="url" alt="" class="h-full w-full object-cover" />
    <svg v-else viewBox="0 0 80 64" fill="none" aria-hidden="true">
      <rect x="12" y="10" width="56" height="44" rx="10" stroke="currentColor" stroke-width="2" opacity=".4" />
      <path v-if="kind === 'video'" d="M34 22v21l18-11z" fill="currentColor" />
      <path v-else-if="kind === 'link'" d="m35 37 10-10m-13 5-3 3a8 8 0 0 0 11 11l6-6m-12-16 6-6a8 8 0 0 1 11 11l-3 3" stroke="currentColor" stroke-width="3" stroke-linecap="round" />
      <path v-else d="M25 24h30M25 32h25M25 40h17" stroke="currentColor" stroke-width="3" stroke-linecap="round" />
    </svg>
  </div>
</template>
<style scoped>
.thread-thumb { width:112px; height:84px; flex-shrink:0; overflow:hidden; border-radius:12px; color:#6ee7b7; background:linear-gradient(135deg,#172b36,#183c3e); }
.thread-thumb[data-topic*="cs"] { color:#7dd3fc; background:linear-gradient(135deg,#172039,#1e4059); }
.thread-thumb[data-topic*="design"] { color:#f9a8d4; background:linear-gradient(135deg,#301d39,#542741); }
.thread-thumb svg { width:100%; height:100%; }
@media(max-width:640px) { .thread-thumb { width:80px; height:64px; } }
</style>
