<script setup lang="ts">
import type { Element } from '@/types'

defineProps<{
  element: Element
  label: string
}>()
</script>

<template>
  <div class="rounded-xl border border-border bg-card p-8 text-center">
    <div class="mx-auto mb-3 flex h-12 w-12 items-center justify-center rounded-full bg-muted/30">
      <svg class="h-6 w-6 text-muted-foreground" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.5">
        <path stroke-linecap="round" stroke-linejoin="round" d="M7 21h10a2 2 0 002-2V9.414a1 1 0 00-.293-.707l-5.414-5.414A1 1 0 0012.586 3H7a2 2 0 00-2 2v14a2 2 0 002 2z" />
      </svg>
    </div>
    <p class="text-sm font-medium text-foreground">
      {{ label }}
    </p>
    <div v-if="element.content_cid" class="mt-2 text-xs text-muted-foreground">
      <p class="mb-1">Content CID:</p>
      <code class="break-all font-mono text-[10px]">{{ element.content_cid }}</code>
    </div>
    <div v-else class="mt-2 text-xs text-muted-foreground">
      No content attached to this element yet.
    </div>
  </div>
</template>
