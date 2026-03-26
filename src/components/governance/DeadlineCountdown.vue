<script setup lang="ts">
import { ref, computed, onMounted, onBeforeUnmount } from 'vue'

const props = defineProps<{
  deadline: string | null
  label?: string
}>()

const now = ref(Date.now())
let interval: ReturnType<typeof setInterval> | null = null

onMounted(() => {
  interval = setInterval(() => { now.value = Date.now() }, 1000)
})

onBeforeUnmount(() => {
  if (interval) clearInterval(interval)
})

const remaining = computed(() => {
  if (!props.deadline) return null
  const target = new Date(props.deadline).getTime()
  const diff = target - now.value
  if (diff <= 0) return null

  const days = Math.floor(diff / 86400000)
  const hours = Math.floor((diff % 86400000) / 3600000)
  const mins = Math.floor((diff % 3600000) / 60000)
  const secs = Math.floor((diff % 60000) / 1000)

  if (days > 0) return `${days}d ${hours}h`
  if (hours > 0) return `${hours}h ${mins}m`
  return `${mins}m ${secs}s`
})

const isPast = computed(() => {
  if (!props.deadline) return false
  return new Date(props.deadline).getTime() <= now.value
})
</script>

<template>
  <span v-if="deadline" class="inline-flex items-center gap-1 text-xs">
    <svg class="h-3 w-3" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
      <path stroke-linecap="round" stroke-linejoin="round" d="M12 6v6h4.5m4.5 0a9 9 0 11-18 0 9 9 0 0118 0z" />
    </svg>
    <span v-if="isPast" class="text-muted-foreground">{{ label || 'Deadline' }} passed</span>
    <span v-else-if="remaining" class="text-foreground font-medium">{{ remaining }}</span>
  </span>
</template>
