<script setup lang="ts">
defineProps<{ score: number; vote: number; disabled?: boolean }>()
defineEmits<{ vote: [value: number] }>()
</script>
<template>
  <div class="inline-flex items-center gap-1 rounded-full bg-muted/60 px-1 text-sm">
    <button type="button" class="min-h-10 min-w-10 rounded-full p-2 hover:bg-primary/10 disabled:opacity-40" :class="vote === 1 ? 'text-primary' : ''" :aria-label="$t('opinions.threads.upvote')" :aria-pressed="vote === 1" :disabled="disabled" @click="$emit('vote', vote === 1 ? 0 : 1)">↑</button>
    <span class="min-w-5 text-center font-semibold tabular-nums">{{ score }}</span>
    <button type="button" class="min-h-10 min-w-10 rounded-full p-2 hover:bg-primary/10 disabled:opacity-40" :class="vote === -1 ? 'text-primary' : ''" :aria-label="$t('opinions.threads.downvote')" :aria-pressed="vote === -1" :disabled="disabled" @click="$emit('vote', vote === -1 ? 0 : -1)">↓</button>
  </div>
</template>
