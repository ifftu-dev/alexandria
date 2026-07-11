<script setup lang="ts">
// PDF / downloadable editor — a single blob upload into content_cid.
import { ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { useLocalApi } from '@/composables/useLocalApi'
import { AppBadge } from '@/components/ui'
import type { Element } from '@/types'

const props = defineProps<{ element: Element }>()
const emit = defineEmits<{ updated: [Element] }>()

const { invoke } = useLocalApi()
const { t } = useI18n()

const uploading = ref(false)
const error = ref('')

async function onFileChange(e: Event) {
  const file = (e.target as HTMLInputElement).files?.[0]
  if (!file) return
  uploading.value = true
  error.value = ''
  try {
    const buf = await file.arrayBuffer()
    const result = await invoke<{ hash: string }>('content_add', {
      data: Array.from(new Uint8Array(buf)),
    })
    const updated = await invoke<Element>('update_element', {
      elementId: props.element.id,
      req: { content_hash: result.hash },
    })
    emit('updated', updated)
  } catch (err) {
    error.value = t('instructor.editors.file.uploadFailed', { error: String(err) })
  } finally {
    uploading.value = false
  }
}
</script>

<template>
  <div class="space-y-3">
    <h3 class="text-sm font-semibold text-foreground">
      {{ element.element_type === 'pdf' ? $t('instructor.editors.file.pdfHeading') : $t('instructor.editors.file.downloadHeading') }}
    </h3>
    <div v-if="element.content_cid" class="flex items-center gap-2 text-sm">
      <AppBadge variant="success">{{ $t('instructor.editors.shared.uploaded') }}</AppBadge>
      <code class="text-xs text-muted-foreground truncate">{{ element.content_cid.slice(0, 24) }}…</code>
    </div>
    <input
      type="file"
      :accept="element.element_type === 'pdf' ? 'application/pdf' : undefined"
      class="block w-full text-sm text-muted-foreground file:mr-4 file:rounded-md file:border-0 file:bg-primary/10 file:px-4 file:py-2 file:text-sm file:font-semibold file:text-primary hover:file:bg-primary/15 cursor-pointer"
      :disabled="uploading"
      @change="onFileChange"
    >
    <p v-if="uploading" class="text-sm text-muted-foreground">{{ $t('instructor.editors.file.uploading') }}</p>
    <p v-if="error" class="text-sm text-error">{{ error }}</p>
  </div>
</template>
