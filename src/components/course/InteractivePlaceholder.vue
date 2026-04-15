<script setup lang="ts">
import TextContent from '@/components/course/TextContent.vue'

defineProps<{
  contentCid: string | null
  contentInline?: string | null
}>()
</script>

<template>
  <div class="space-y-4">
    <TextContent :content-cid="contentCid" :content-inline="contentInline" />
    <div class="flex items-center gap-2 rounded-lg bg-amber-50 p-3 text-xs text-amber-700 dark:bg-amber-900/20 dark:text-amber-400">
      <svg class="h-4 w-4 flex-shrink-0" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
        <path stroke-linecap="round" stroke-linejoin="round" d="M13 10V3L4 14h7v7l9-11h-7z" />
      </svg>
      Interactive simulation support coming in a future update.
    </div>
  </div>
</template>
