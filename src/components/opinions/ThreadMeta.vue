<script setup lang="ts">
defineProps<{ author: string; createdAt: number; topic?: string; own?: boolean; edited?: boolean }>()
</script>
<template>
  <div class="flex min-w-0 flex-wrap items-center gap-x-2 gap-y-1 text-xs text-muted-foreground">
    <span class="flex h-6 w-6 shrink-0 items-center justify-center rounded-full bg-primary/10 text-primary" aria-hidden="true">
      <svg class="h-3.5 w-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75"><circle cx="12" cy="8" r="3" /><path d="M5 21v-2a7 7 0 0 1 14 0v2" /></svg>
    </span>
    <span :title="author" :aria-label="author" class="font-medium text-foreground">{{ own ? $t('opinions.threads.you') : `…${author.slice(-8)}` }}</span>
    <span aria-hidden="true">·</span>
    <time :datetime="new Date(createdAt * 1000).toISOString()" :title="new Date(createdAt * 1000).toLocaleString()">{{ new Date(createdAt * 1000).toLocaleDateString(undefined, { month: 'short', day: 'numeric' }) }}</time>
    <span v-if="edited">{{ $t('opinions.threads.edited') }}</span>
    <span v-if="topic" class="ms-1 rounded bg-primary/10 px-2 py-0.5 font-medium text-primary">{{ topic }}</span>
  </div>
</template>
