<script setup lang="ts">
import { ref, onMounted, watch } from 'vue'
import { useLocalApi } from '@/composables/useLocalApi'
import { AppSpinner } from '@/components/ui'

const props = defineProps<{
  contentCid: string | null
  contentInline?: string | null
}>()

const emit = defineEmits<{
  (e: 'complete'): void
}>()

const { invoke } = useLocalApi()
const content = ref('')
const loading = ref(false)
const error = ref<string | null>(null)

async function loadContent() {
  // Prefer inline content (works on all platforms including mobile)
  if (props.contentInline) {
    content.value = props.contentInline
    return
  }
  if (!props.contentCid) { content.value = ''; return }
  loading.value = true
  error.value = null
  try {
    const bytes = await invoke<number[]>('content_resolve_bytes', { identifier: props.contentCid })
    const decoder = new TextDecoder()
    content.value = decoder.decode(new Uint8Array(bytes))
  } catch (e: unknown) {
    error.value = `Failed to load content: ${e}`
    content.value = ''
  } finally {
    loading.value = false
  }
}

onMounted(loadContent)
watch(() => [props.contentCid, props.contentInline], loadContent)
</script>

<template>
  <div class="text-content">
    <AppSpinner v-if="loading" label="Loading content..." />

    <div v-else-if="error" class="text-sm text-[rgb(var(--color-destructive))]">
      {{ error }}
    </div>

    <div v-else-if="content" class="prose max-w-none" v-html="content" />

    <div v-else class="text-sm text-[rgb(var(--color-muted-foreground))] italic">
      No content available.
    </div>
  </div>
</template>
