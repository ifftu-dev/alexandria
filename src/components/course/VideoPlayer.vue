<script setup lang="ts">
import { ref, onMounted, watch, onUnmounted, computed } from 'vue'
import { useLocalApi } from '@/composables/useLocalApi'
import { AppSpinner, AppButton } from '@/components/ui'

const props = defineProps<{
  contentCid: string | null
  title?: string
}>()

const emit = defineEmits<{
  (e: 'complete'): void
  (e: 'timeupdate', seconds: number): void
}>()

const { invoke } = useLocalApi()
const videoUrl = ref<string | null>(null)
const loading = ref(false)
const error = ref<string | null>(null)
const videoRef = ref<HTMLVideoElement | null>(null)
const progress = ref(0)
const duration = ref(0)
const isPlaying = ref(false)
const hasEnded = ref(false)

async function loadVideo() {
  if (!props.contentCid) { videoUrl.value = null; return }
  loading.value = true
  error.value = null
  try {
    const bytes = await invoke<number[]>('content_resolve_bytes', { identifier: props.contentCid })
    // Create an object URL from the blob
    const blob = new Blob([new Uint8Array(bytes)], { type: 'video/mp4' })
    if (videoUrl.value) URL.revokeObjectURL(videoUrl.value)
    videoUrl.value = URL.createObjectURL(blob)
  } catch (e: unknown) {
    error.value = `Failed to load video: ${e}`
    videoUrl.value = null
  } finally {
    loading.value = false
  }
}

function onTimeUpdate() {
  if (!videoRef.value) return
  progress.value = videoRef.value.currentTime
  duration.value = videoRef.value.duration || 0
  emit('timeupdate', videoRef.value.currentTime)
}

function onEnded() {
  hasEnded.value = true
  isPlaying.value = false
  emit('complete')
}

function togglePlay() {
  if (!videoRef.value) return
  if (videoRef.value.paused) {
    videoRef.value.play()
    isPlaying.value = true
  } else {
    videoRef.value.pause()
    isPlaying.value = false
  }
}

const progressPercent = computed(() => {
  if (!duration.value) return 0
  return Math.round((progress.value / duration.value) * 100)
})

function formatTime(seconds: number): string {
  const m = Math.floor(seconds / 60)
  const s = Math.floor(seconds % 60)
  return `${m}:${s.toString().padStart(2, '0')}`
}

onMounted(loadVideo)
watch(() => props.contentCid, loadVideo)

onUnmounted(() => {
  if (videoUrl.value) URL.revokeObjectURL(videoUrl.value)
})
</script>

<template>
  <div class="video-player">
    <AppSpinner v-if="loading" label="Loading video..." />

    <div v-else-if="error" class="text-sm text-destructive">
      {{ error }}
    </div>

    <div v-else-if="videoUrl" class="space-y-3">
      <div class="relative rounded-lg overflow-hidden bg-black aspect-video">
        <video
          ref="videoRef"
          :src="videoUrl"
          class="w-full h-full"
          @timeupdate="onTimeUpdate"
          @ended="onEnded"
          @play="isPlaying = true"
          @pause="isPlaying = false"
        />
      </div>

      <!-- Controls -->
      <div class="flex items-center gap-3">
        <AppButton size="sm" variant="secondary" @click="togglePlay">
          {{ isPlaying ? 'Pause' : 'Play' }}
        </AppButton>

        <div class="flex-1 h-1.5 bg-muted/30 rounded-full overflow-hidden">
          <div
            class="h-full bg-primary transition-all duration-300"
            :style="{ width: `${progressPercent}%` }"
          />
        </div>

        <span class="text-xs text-muted-foreground font-mono whitespace-nowrap">
          {{ formatTime(progress) }} / {{ formatTime(duration) }}
        </span>
      </div>

      <div v-if="hasEnded" class="flex items-center gap-2 text-sm text-success">
        <svg class="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
          <path stroke-linecap="round" stroke-linejoin="round" d="M5 13l4 4L19 7" />
        </svg>
        Video completed
      </div>
    </div>

    <div v-else class="flex flex-col items-center justify-center rounded-lg border border-dashed border-border bg-muted/30 aspect-video">
      <svg class="w-12 h-12 text-muted-foreground/50 mb-3" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.5">
        <path stroke-linecap="round" stroke-linejoin="round" d="m15.75 10.5 4.72-4.72a.75.75 0 0 1 1.28.53v11.38a.75.75 0 0 1-1.28.53l-4.72-4.72M4.5 18.75h9a2.25 2.25 0 0 0 2.25-2.25v-9a2.25 2.25 0 0 0-2.25-2.25h-9A2.25 2.25 0 0 0 2.25 7.5v9a2.25 2.25 0 0 0 2.25 2.25Z" />
      </svg>
      <p class="text-sm font-medium text-muted-foreground">{{ title ?? 'Video' }}</p>
      <p class="text-xs text-muted-foreground/60 mt-1">Video content not yet available on this node</p>
    </div>
  </div>
</template>
