<script setup lang="ts">
import { AppButton } from '@/components/ui'
import type { Element } from '@/types'

defineProps<{
  element: Element
  downloading: boolean
  error: string | null
}>()

defineEmits<{
  (e: 'download'): void
}>()

function formatFileSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`
}
</script>

<template>
  <div class="rounded-xl border border-border bg-card p-8">
    <div class="flex items-start gap-5">
      <div class="flex h-14 w-14 flex-shrink-0 items-center justify-center rounded-xl bg-green-100 dark:bg-green-900/30">
        <svg class="h-7 w-7 text-green-600 dark:text-green-400" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.5">
          <path stroke-linecap="round" stroke-linejoin="round" d="M4 16v1a3 3 0 003 3h10a3 3 0 003-3v-1m-4-4l-4 4m0 0l-4-4m4 4V4" />
        </svg>
      </div>
      <div class="flex-1 min-w-0">
        <h3 class="text-base font-semibold text-foreground">
          {{ (element as { filename?: string }).filename || element.title }}
        </h3>
        <div class="mt-1 flex items-center gap-3 text-sm text-muted-foreground">
          <span v-if="(element as { mime_type?: string }).mime_type">{{ (element as { mime_type?: string }).mime_type }}</span>
          <span v-if="(element as { size_bytes?: number }).size_bytes">{{ formatFileSize((element as unknown as { size_bytes: number }).size_bytes) }}</span>
        </div>
        <p v-if="(element as { description?: string }).description" class="mt-3 text-sm text-muted-foreground">
          {{ (element as { description?: string }).description }}
        </p>
        <div class="mt-4">
          <AppButton :loading="downloading" :disabled="downloading" @click="$emit('download')">
            <svg class="mr-2 h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
              <path stroke-linecap="round" stroke-linejoin="round" d="M4 16v1a3 3 0 003 3h10a3 3 0 003-3v-1m-4-4l-4 4m0 0l-4-4m4 4V4" />
            </svg>
            Download
          </AppButton>
          <p v-if="error" class="mt-2 text-xs text-destructive">
            {{ error }}
          </p>
        </div>
      </div>
    </div>
  </div>
</template>
