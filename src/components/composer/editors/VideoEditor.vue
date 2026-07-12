<script setup lang="ts">
// Video element editor: blob upload + duration probe + chapter markers.
import { onMounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { useLocalApi } from '@/composables/useLocalApi'
import { AppBadge, AppButton } from '@/components/ui'
import type { Element, VideoChapterInput } from '@/types'

const props = defineProps<{ element: Element }>()
const emit = defineEmits<{ updated: [Element] }>()

const { invoke } = useLocalApi()
const { t } = useI18n()

const uploading = ref(false)
const progress = ref('')
const error = ref('')

const markers = ref<VideoChapterInput[]>([])
const markersDirty = ref(false)
const savingMarkers = ref(false)

onMounted(async () => {
  markers.value = await invoke<VideoChapterInput[]>('list_video_chapters', {
    elementId: props.element.id,
  }).catch(() => [])
})

async function onFileChange(e: Event) {
  const file = (e.target as HTMLInputElement).files?.[0]
  if (!file) return
  uploading.value = true
  error.value = ''
  progress.value = t('instructor.editors.video.reading', { size: Math.round(file.size / 1024 / 1024) })
  try {
    const buf = await file.arrayBuffer()
    progress.value = t('instructor.editors.video.saving')
    const result = await invoke<{ hash: string; size: number }>('content_add', {
      data: Array.from(new Uint8Array(buf)),
    })

    // Best-effort duration probe.
    let duration: number | null = null
    const probe = document.createElement('video')
    probe.preload = 'metadata'
    probe.src = URL.createObjectURL(file)
    await new Promise<void>((resolve) => {
      probe.onloadedmetadata = () => resolve()
      probe.onerror = () => resolve()
    })
    if (Number.isFinite(probe.duration) && probe.duration > 0) {
      duration = Math.round(probe.duration)
    }
    URL.revokeObjectURL(probe.src)

    const updated = await invoke<Element>('update_element', {
      elementId: props.element.id,
      req: { content_hash: result.hash, duration_seconds: duration },
    })
    emit('updated', updated)
    progress.value = ''
  } catch (err) {
    error.value = t('instructor.editors.video.uploadFailed', { error: String(err) })
  } finally {
    uploading.value = false
  }
}

function addMarker() {
  markers.value.push({ title: '', start_seconds: 0 })
  markersDirty.value = true
}

function removeMarker(i: number) {
  markers.value.splice(i, 1)
  markersDirty.value = true
}

async function saveMarkers() {
  savingMarkers.value = true
  error.value = ''
  try {
    const clean = markers.value
      .filter(m => m.title.trim().length > 0)
      .map(m => ({ title: m.title.trim(), start_seconds: Math.max(0, m.start_seconds) }))
    await invoke('set_video_chapters', { elementId: props.element.id, chapters: clean })
    markers.value = clean
    markersDirty.value = false
  } catch (err) {
    error.value = String(err)
  } finally {
    savingMarkers.value = false
  }
}
</script>

<template>
  <div class="space-y-5">
    <div>
      <h3 class="text-sm font-semibold text-foreground mb-2">{{ $t('instructor.editors.video.fileHeading') }}</h3>
      <div v-if="element.content_cid" class="flex items-center gap-2 text-sm mb-2">
        <AppBadge variant="success">{{ $t('instructor.editors.shared.uploaded') }}</AppBadge>
        <code class="text-xs text-muted-foreground truncate">{{ element.content_cid.slice(0, 24) }}…</code>
        <span v-if="element.duration_seconds" class="text-xs text-muted-foreground">
          · {{ $t('instructor.editors.video.minutes', { count: Math.round(element.duration_seconds / 60) }) }}
        </span>
      </div>
      <input
        type="file"
        accept="video/*"
        class="block w-full text-sm text-muted-foreground file:me-4 file:rounded-md file:border-0 file:bg-primary/10 file:px-4 file:py-2 file:text-sm file:font-semibold file:text-primary hover:file:bg-primary/15 cursor-pointer"
        :disabled="uploading"
        @change="onFileChange"
      >
      <p v-if="uploading" class="mt-1 text-sm text-muted-foreground">{{ progress }}</p>
    </div>

    <div>
      <div class="flex items-center justify-between mb-2">
        <h3 class="text-sm font-semibold text-foreground">{{ $t('instructor.editors.video.markersHeading') }}</h3>
        <div class="flex gap-2">
          <AppButton variant="ghost" size="xs" @click="addMarker">{{ $t('instructor.editors.video.addMarker') }}</AppButton>
          <AppButton v-if="markersDirty" size="xs" :loading="savingMarkers" @click="saveMarkers">
            {{ $t('instructor.editors.video.saveMarkers') }}
          </AppButton>
        </div>
      </div>
      <p v-if="!markers.length" class="text-xs text-muted-foreground">
        {{ $t('instructor.editors.video.markersHint') }}
      </p>
      <div v-for="(m, i) in markers" :key="i" class="flex items-center gap-2 mb-2">
        <input
          v-model="m.title"
          type="text"
          :placeholder="$t('instructor.editors.video.markerTitlePlaceholder')"
          class="flex-1 rounded-md border border-border bg-background px-3 py-2 text-sm"
          @input="markersDirty = true"
        >
        <input
          v-model.number="m.start_seconds"
          type="number"
          min="0"
          class="w-28 rounded-md border border-border bg-background px-2 py-2 text-sm text-center"
          :title="$t('instructor.editors.video.startSeconds')"
          @input="markersDirty = true"
        >
        <button
          type="button"
          class="rounded-md p-2 text-muted-foreground hover:bg-muted hover:text-foreground"
          @click="removeMarker(i)"
        >
          <svg class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
            <path stroke-linecap="round" stroke-linejoin="round" d="M6 18L18 6M6 6l12 12" />
          </svg>
        </button>
      </div>
    </div>

    <p v-if="error" class="text-sm text-error">{{ error }}</p>
  </div>
</template>
