<script setup lang="ts">
defineProps<{ score: number; vote: number; disabled?: boolean }>()
defineEmits<{ vote: [value: number] }>()
</script>
<template>
  <div class="inline-flex shrink-0 items-center rounded-lg bg-muted/40 text-xs">
    <button type="button" class="vote-button rounded-s-lg hover:bg-primary/10 disabled:opacity-40" :class="vote === 1 ? 'text-primary' : 'text-muted-foreground'" :aria-label="$t('opinions.threads.upvote')" :aria-pressed="vote === 1" :disabled="disabled" @click="$emit('vote', vote === 1 ? 0 : 1)">
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" class="h-4 w-4" aria-hidden="true"><path stroke-linecap="round" stroke-linejoin="round" d="m6 11 6-6 6 6M12 5v14" /></svg>
    </button>
    <span class="min-w-5 text-center font-semibold tabular-nums">{{ score }}</span>
    <button type="button" class="vote-button rounded-e-lg hover:bg-primary/10 disabled:opacity-40" :class="vote === -1 ? 'text-primary' : 'text-muted-foreground'" :aria-label="$t('opinions.threads.downvote')" :aria-pressed="vote === -1" :disabled="disabled" @click="$emit('vote', vote === -1 ? 0 : -1)">
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" class="h-4 w-4" aria-hidden="true"><path stroke-linecap="round" stroke-linejoin="round" d="m6 13 6 6 6-6M12 19V5" /></svg>
    </button>
  </div>
</template>
<style scoped>
.vote-button { display:flex; align-items:center; justify-content:center; width:32px; height:32px; }
@media(pointer:coarse) { .vote-button { width:40px; height:44px; } }
</style>
