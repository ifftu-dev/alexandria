<script setup lang="ts">
defineProps<{
  requiredLevel: string
  met: boolean
}>()

const levelColors: Record<string, string> = {
  remember: 'text-muted-foreground',
  understand: 'text-primary',
  apply: 'text-accent',
  analyze: 'text-warning',
  evaluate: 'text-success',
  create: 'text-governance',
}
</script>

<template>
  <div v-if="!met" class="flex items-center gap-1.5 rounded-lg bg-muted/30 px-2.5 py-1.5 text-xs">
    <svg class="h-3.5 w-3.5 text-muted-foreground" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
      <path stroke-linecap="round" stroke-linejoin="round" d="M16.5 10.5V6.75a4.5 4.5 0 10-9 0v3.75m-.75 11.25h10.5a2.25 2.25 0 002.25-2.25v-6.75a2.25 2.25 0 00-2.25-2.25H6.75a2.25 2.25 0 00-2.25 2.25v6.75a2.25 2.25 0 002.25 2.25z" />
    </svg>
    <span class="text-muted-foreground">
      Requires <span :class="levelColors[requiredLevel] || 'text-foreground'" class="font-medium">{{ requiredLevel }}</span> proficiency
    </span>
  </div>
</template>
