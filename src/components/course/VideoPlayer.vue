<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from 'vue'
import { useLocalApi } from '@/composables/useLocalApi'
import { AppSpinner } from '@/components/ui'

type QualityOption = {
  id: string
  label: string
  needsTranscoding: boolean
}

const props = defineProps<{
  contentCid: string | null
  title?: string
}>()

const emit = defineEmits<{
  (e: 'complete'): void
  (e: 'timeupdate', seconds: number): void
}>()

const { invoke } = useLocalApi()

const wrapperRef = ref<HTMLElement | null>(null)
const videoRef = ref<HTMLVideoElement | null>(null)

const videoUrl = ref<string | null>(null)
const loading = ref(false)
const error = ref<string | null>(null)

const currentTime = ref(0)
const duration = ref(0)
const bufferedEnd = ref(0)
const volume = ref(1)
const isMuted = ref(false)
const isPlaying = ref(false)
const hasEnded = ref(false)
const isFullscreen = ref(false)
const pseudoFullscreen = ref(false)
const isPiP = ref(false)

const showControls = ref(true)
const showSettings = ref(false)
const isScrubbing = ref(false)
const scrubTime = ref(0)
const controlsTimer = ref<number | null>(null)

const activeSpeed = ref('1')
const activeQuality = ref('auto')
const captionEnabled = ref(false)
const infoMessage = ref<string | null>(null)
const infoTimer = ref<number | null>(null)

const speedOptions = [
  { id: '0.25', label: '0.25x' },
  { id: '0.5', label: '0.5x' },
  { id: '0.75', label: '0.75x' },
  { id: '1', label: 'Normal' },
  { id: '1.25', label: '1.25x' },
  { id: '1.5', label: '1.5x' },
  { id: '1.75', label: '1.75x' },
  { id: '2', label: '2x' },
]

const qualityOptions: QualityOption[] = [
  { id: 'auto', label: 'Auto', needsTranscoding: false },
  { id: '1080p', label: '1080p', needsTranscoding: true },
  { id: '720p', label: '720p', needsTranscoding: true },
  { id: '480p', label: '480p', needsTranscoding: true },
]

const effectiveTime = computed(() => (isScrubbing.value ? scrubTime.value : currentTime.value))
const fullscreenActive = computed(() => isFullscreen.value || pseudoFullscreen.value)
const progressPercent = computed(() => (duration.value > 0 ? (effectiveTime.value / duration.value) * 100 : 0))
const bufferedPercent = computed(() => (duration.value > 0 ? (bufferedEnd.value / duration.value) * 100 : 0))
const volumePercent = computed(() => Math.round((isMuted.value ? 0 : volume.value) * 100))

function clearControlsTimer() {
  if (controlsTimer.value !== null) {
    window.clearTimeout(controlsTimer.value)
    controlsTimer.value = null
  }
}

function clearInfoTimer() {
  if (infoTimer.value !== null) {
    window.clearTimeout(infoTimer.value)
    infoTimer.value = null
  }
}

function setInfo(message: string) {
  infoMessage.value = message
  clearInfoTimer()
  infoTimer.value = window.setTimeout(() => {
    infoMessage.value = null
  }, 2200)
}

function scheduleControlsHide() {
  clearControlsTimer()
  if (!isPlaying.value || showSettings.value) {
    showControls.value = true
    return
  }
  controlsTimer.value = window.setTimeout(() => {
    if (!isScrubbing.value && isPlaying.value && !showSettings.value) {
      showControls.value = false
    }
  }, 2200)
}

function revealControls() {
  showControls.value = true
  scheduleControlsHide()
}

function formatTime(seconds: number): string {
  if (!Number.isFinite(seconds) || seconds < 0) return '0:00'
  const total = Math.floor(seconds)
  const h = Math.floor(total / 3600)
  const m = Math.floor((total % 3600) / 60)
  const s = total % 60
  if (h > 0) {
    return `${h}:${String(m).padStart(2, '0')}:${String(s).padStart(2, '0')}`
  }
  return `${m}:${String(s).padStart(2, '0')}`
}

function updateBuffered() {
  const video = videoRef.value
  if (!video) return
  try {
    if (video.buffered.length > 0) {
      bufferedEnd.value = video.buffered.end(video.buffered.length - 1) || 0
    }
  } catch {
    bufferedEnd.value = 0
  }
}

function onTimeUpdate() {
  const video = videoRef.value
  if (!video) return
  currentTime.value = video.currentTime
  duration.value = video.duration || 0
  updateBuffered()
  emit('timeupdate', video.currentTime)
}

function onLoadedMetadata() {
  const video = videoRef.value
  if (!video) return
  duration.value = video.duration || 0
  currentTime.value = video.currentTime || 0
  updateBuffered()
}

function onEnded() {
  hasEnded.value = true
  isPlaying.value = false
  showControls.value = true
  emit('complete')
}

async function togglePlay() {
  const video = videoRef.value
  if (!video) return
  try {
    if (video.paused || video.ended) {
      await video.play()
      hasEnded.value = false
      isPlaying.value = true
    } else {
      video.pause()
      isPlaying.value = false
    }
  } catch {
    setInfo('Unable to start playback')
  }
  scheduleControlsHide()
}

function seekTo(seconds: number) {
  const video = videoRef.value
  if (!video || duration.value <= 0) return
  const clamped = Math.max(0, Math.min(duration.value, seconds))
  video.currentTime = clamped
  currentTime.value = clamped
}

function seekBy(delta: number) {
  seekTo(currentTime.value + delta)
  setInfo(`${delta > 0 ? '+' : ''}${Math.round(delta)}s`)
}

function onProgressInput(event: Event) {
  const target = event.target as HTMLInputElement
  const value = Number(target.value)
  scrubTime.value = value
  if (!isScrubbing.value) {
    seekTo(value)
  }
}

function onProgressPointerDown() {
  isScrubbing.value = true
  revealControls()
}

function onProgressPointerUp(event: Event) {
  const target = event.target as HTMLInputElement
  const value = Number(target.value)
  isScrubbing.value = false
  seekTo(value)
  scheduleControlsHide()
}

function setPlaybackRate(rate: string) {
  const video = videoRef.value
  const numeric = Number(rate)
  if (!video || Number.isNaN(numeric)) return
  video.playbackRate = numeric
  activeSpeed.value = rate
  setInfo(`Speed: ${speedOptions.find((s) => s.id === rate)?.label ?? `${rate}x`}`)
}

function toggleMute() {
  const video = videoRef.value
  if (!video) return
  isMuted.value = !isMuted.value
  video.muted = isMuted.value
}

function onVolumeInput(event: Event) {
  const video = videoRef.value
  if (!video) return
  const target = event.target as HTMLInputElement
  const next = Math.max(0, Math.min(1, Number(target.value)))
  volume.value = next
  video.volume = next
  if (next === 0) {
    isMuted.value = true
    video.muted = true
  } else if (isMuted.value) {
    isMuted.value = false
    video.muted = false
  }
}

async function toggleFullscreen() {
  const wrapper = wrapperRef.value
  const video = videoRef.value as (HTMLVideoElement & { webkitEnterFullscreen?: () => void }) | null
  if (!wrapper) return

  if (pseudoFullscreen.value) {
    pseudoFullscreen.value = false
    return
  }

  try {
    if (!document.fullscreenElement && typeof wrapper.requestFullscreen === 'function') {
      await wrapper.requestFullscreen()
    } else {
      if (document.fullscreenElement && typeof document.exitFullscreen === 'function') {
        await document.exitFullscreen()
      } else if (video?.webkitEnterFullscreen) {
        video.webkitEnterFullscreen()
      }
    }
  } catch {
    if (video?.webkitEnterFullscreen) {
      try {
        video.webkitEnterFullscreen()
        return
      } catch {
        pseudoFullscreen.value = true
        setInfo('Using in-app fullscreen')
      }
    } else {
      pseudoFullscreen.value = true
      setInfo('Using in-app fullscreen')
    }
  }
}

async function togglePiP() {
  const video = videoRef.value
  if (!video) return
  const doc = document as Document & {
    pictureInPictureEnabled?: boolean
    pictureInPictureElement?: Element | null
    exitPictureInPicture?: () => Promise<void>
  }

  if (!doc.pictureInPictureEnabled) {
    setInfo('Picture-in-picture unavailable')
    return
  }

  try {
    if (doc.pictureInPictureElement) {
      await doc.exitPictureInPicture?.()
    } else {
      await (video as HTMLVideoElement & { requestPictureInPicture?: () => Promise<void> }).requestPictureInPicture?.()
    }
  } catch {
    setInfo('Picture-in-picture unavailable')
  }
}

function toggleCaptions() {
  captionEnabled.value = !captionEnabled.value
  if (captionEnabled.value) {
    setInfo('Captions stub: transcript track support will be wired next')
  } else {
    setInfo('Captions off')
  }
}

function selectQuality(option: QualityOption) {
  if (option.needsTranscoding) {
    setInfo(`Quality ${option.label} stub: requires transcoded renditions`)
    activeQuality.value = option.id
    return
  }
  activeQuality.value = option.id
  setInfo(`Quality: ${option.label}`)
}

async function loadVideo() {
  if (!props.contentCid) {
    videoUrl.value = null
    return
  }

  loading.value = true
  error.value = null
  try {
    const bytes = await invoke<number[]>('content_resolve_bytes', { identifier: props.contentCid })
    const blob = new Blob([new Uint8Array(bytes)], { type: 'video/mp4' })
    if (videoUrl.value) {
      URL.revokeObjectURL(videoUrl.value)
    }
    videoUrl.value = URL.createObjectURL(blob)
    hasEnded.value = false
    currentTime.value = 0
    duration.value = 0
    bufferedEnd.value = 0
  } catch (e: unknown) {
    error.value = `Failed to load video: ${String(e)}`
    videoUrl.value = null
  } finally {
    loading.value = false
  }
}

function onContainerClick() {
  togglePlay()
}

function onKeydown(event: KeyboardEvent) {
  const target = event.target as HTMLElement | null
  const tag = target?.tagName?.toLowerCase()
  if (tag === 'input' || tag === 'textarea' || target?.isContentEditable) return
  if (!videoUrl.value) return

  if (event.key === ' ' || event.key.toLowerCase() === 'k') {
    event.preventDefault()
    togglePlay()
  } else if (event.key.toLowerCase() === 'j') {
    event.preventDefault()
    seekBy(-10)
  } else if (event.key.toLowerCase() === 'l') {
    event.preventDefault()
    seekBy(10)
  } else if (event.key === 'ArrowLeft') {
    event.preventDefault()
    seekBy(-5)
  } else if (event.key === 'ArrowRight') {
    event.preventDefault()
    seekBy(5)
  } else if (event.key === 'ArrowUp') {
    event.preventDefault()
    const next = Math.min(1, volume.value + 0.05)
    volume.value = next
    const v = videoRef.value
    if (v) {
      v.volume = next
      if (next > 0 && isMuted.value) {
        isMuted.value = false
        v.muted = false
      }
    }
  } else if (event.key === 'ArrowDown') {
    event.preventDefault()
    const next = Math.max(0, volume.value - 0.05)
    volume.value = next
    const v = videoRef.value
    if (v) {
      v.volume = next
      if (next === 0) {
        isMuted.value = true
        v.muted = true
      }
    }
  } else if (event.key.toLowerCase() === 'm') {
    event.preventDefault()
    toggleMute()
  } else if (event.key.toLowerCase() === 'f') {
    event.preventDefault()
    toggleFullscreen()
  } else if (event.key === 'Escape' && pseudoFullscreen.value) {
    event.preventDefault()
    pseudoFullscreen.value = false
  } else if (event.key.toLowerCase() === 'c') {
    event.preventDefault()
    toggleCaptions()
  } else if (event.key === '>') {
    event.preventDefault()
    const idx = speedOptions.findIndex((s) => s.id === activeSpeed.value)
    const next = Math.min(speedOptions.length - 1, idx + 1)
    setPlaybackRate(speedOptions[next]!.id)
  } else if (event.key === '<') {
    event.preventDefault()
    const idx = speedOptions.findIndex((s) => s.id === activeSpeed.value)
    const next = Math.max(0, idx - 1)
    setPlaybackRate(speedOptions[next]!.id)
  }
}

function onFullscreenChange() {
  isFullscreen.value = !!document.fullscreenElement
}

function onPiPEnter() {
  isPiP.value = true
}

function onPiPLeave() {
  isPiP.value = false
}

onMounted(() => {
  void loadVideo()
  window.addEventListener('keydown', onKeydown)
  document.addEventListener('fullscreenchange', onFullscreenChange)
})

watch(videoRef, (video, prev) => {
  const vPrev = prev as (HTMLVideoElement & {
    _alexWebkitBegin?: EventListener
    _alexWebkitEnd?: EventListener
  }) | null
  if (vPrev?._alexWebkitBegin) {
    vPrev.removeEventListener('webkitbeginfullscreen', vPrev._alexWebkitBegin)
  }
  if (vPrev?._alexWebkitEnd) {
    vPrev.removeEventListener('webkitendfullscreen', vPrev._alexWebkitEnd)
  }

  const v = video as (HTMLVideoElement & {
    _alexWebkitBegin?: EventListener
    _alexWebkitEnd?: EventListener
  }) | null
  if (!v) return
  v._alexWebkitBegin = () => {
    isFullscreen.value = true
  }
  v._alexWebkitEnd = () => {
    isFullscreen.value = false
  }
  v.addEventListener('webkitbeginfullscreen', v._alexWebkitBegin)
  v.addEventListener('webkitendfullscreen', v._alexWebkitEnd)
})

watch(() => props.contentCid, () => {
  void loadVideo()
})

watch(isPlaying, () => {
  scheduleControlsHide()
})

watch(pseudoFullscreen, (enabled) => {
  document.body.style.overflow = enabled ? 'hidden' : ''
})

onUnmounted(() => {
  if (videoUrl.value) URL.revokeObjectURL(videoUrl.value)
  document.body.style.overflow = ''
  clearControlsTimer()
  clearInfoTimer()
  window.removeEventListener('keydown', onKeydown)
  document.removeEventListener('fullscreenchange', onFullscreenChange)
})
</script>

<template>
  <div class="video-player">
    <AppSpinner v-if="loading" label="Loading video..." />

    <div v-else-if="error" class="text-sm text-destructive">
      {{ error }}
    </div>

    <div
      v-else-if="videoUrl"
      ref="wrapperRef"
      class="group relative aspect-video overflow-hidden rounded-xl bg-black"
      :class="pseudoFullscreen ? 'fixed inset-0 z-[80] aspect-auto rounded-none' : ''"
      @mousemove="revealControls"
      @mouseleave="scheduleControlsHide"
      @click="onContainerClick"
      @dblclick.stop="toggleFullscreen"
    >
      <video
        ref="videoRef"
        :src="videoUrl"
        class="h-full w-full"
        @timeupdate="onTimeUpdate"
        @loadedmetadata="onLoadedMetadata"
        @progress="updateBuffered"
        @ended="onEnded"
        @play="isPlaying = true"
        @pause="isPlaying = false"
        @enterpictureinpicture="onPiPEnter"
        @leavepictureinpicture="onPiPLeave"
      />

      <div v-if="infoMessage" class="pointer-events-none absolute left-1/2 top-1/2 -translate-x-1/2 -translate-y-1/2 rounded-md bg-black/75 px-3 py-1.5 text-xs font-medium text-white">
        {{ infoMessage }}
      </div>

      <button
        v-if="!isPlaying"
        type="button"
        class="absolute left-1/2 top-1/2 z-10 flex h-16 w-16 -translate-x-1/2 -translate-y-1/2 items-center justify-center rounded-full bg-black/65 text-white transition hover:bg-black/80"
        @click.stop="togglePlay"
      >
        <svg class="h-8 w-8" fill="currentColor" viewBox="0 0 24 24" aria-hidden="true">
          <path d="M8 5.14v14l11-7z" />
        </svg>
      </button>

      <div
        class="absolute inset-x-0 bottom-0 z-20 bg-gradient-to-t from-black/80 to-transparent px-3 pb-2 pt-8 text-white transition-opacity duration-200"
        :class="showControls ? 'opacity-100' : 'pointer-events-none opacity-0'"
        @click.stop
      >
        <div class="mb-2">
          <input
            class="yt-range yt-range-progress h-1.5 w-full cursor-pointer rounded-full"
            type="range"
            min="0"
            :max="duration || 0"
            :step="0.1"
            :value="effectiveTime"
            :style="{ '--played': `${progressPercent}%`, '--buffered': `${bufferedPercent}%` }"
            @input="onProgressInput"
            @pointerdown="onProgressPointerDown"
            @pointerup="onProgressPointerUp"
          >
        </div>

        <div class="flex items-center gap-2">
          <button type="button" class="video-icon-btn" @click="togglePlay">
            <svg v-if="isPlaying" class="h-5 w-5" fill="currentColor" viewBox="0 0 24 24" aria-hidden="true">
              <path d="M6 5h4v14H6zM14 5h4v14h-4z" />
            </svg>
            <svg v-else class="h-5 w-5" fill="currentColor" viewBox="0 0 24 24" aria-hidden="true">
              <path d="M8 5.14v14l11-7z" />
            </svg>
          </button>

          <button type="button" class="video-icon-btn" @click="seekBy(-10)">
            <svg class="h-5 w-5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.7" aria-hidden="true">
              <path stroke-linecap="round" stroke-linejoin="round" d="M12 8V4l-4 4 4 4V8c2.761 0 5 2.239 5 5a5 5 0 01-9.163 2.748" />
            </svg>
          </button>

          <button type="button" class="video-icon-btn" @click="seekBy(10)">
            <svg class="h-5 w-5 scale-x-[-1]" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.7" aria-hidden="true">
              <path stroke-linecap="round" stroke-linejoin="round" d="M12 8V4l-4 4 4 4V8c2.761 0 5 2.239 5 5a5 5 0 01-9.163 2.748" />
            </svg>
          </button>

          <button type="button" class="video-icon-btn" @click="toggleMute">
            <svg v-if="isMuted || volume === 0" class="h-5 w-5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.8" aria-hidden="true">
              <path stroke-linecap="round" stroke-linejoin="round" d="M11 5 6 9H3v6h3l5 4V5Zm5 5-4 4m0-4 4 4" />
            </svg>
            <svg v-else class="h-5 w-5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.8" aria-hidden="true">
              <path stroke-linecap="round" stroke-linejoin="round" d="M11 5 6 9H3v6h3l5 4V5Zm4 4a3 3 0 0 1 0 6m2.5-8.5a6 6 0 0 1 0 11" />
            </svg>
          </button>

          <input
            class="yt-range h-1.5 w-20 rounded-full"
            type="range"
            min="0"
            max="1"
            step="0.01"
            :value="isMuted ? 0 : volume"
            :style="{ '--played': `${volumePercent}%` }"
            @input="onVolumeInput"
          >

          <span class="ml-1 text-xs font-mono text-white/90">
            {{ formatTime(currentTime) }} / {{ formatTime(duration) }}
          </span>

          <div class="ml-auto flex items-center gap-1">
            <button
              type="button"
              class="video-icon-btn video-icon-btn--cc"
              :class="captionEnabled ? 'text-primary-300' : ''"
              @click="toggleCaptions"
            >
              CC
            </button>

            <div class="relative">
              <button type="button" class="video-icon-btn" @click="showSettings = !showSettings">
                <svg class="h-[1.35rem] w-[1.35rem]" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.8" aria-hidden="true">
                  <path stroke-linecap="round" stroke-linejoin="round" d="M4 6h10" />
                  <path stroke-linecap="round" stroke-linejoin="round" d="M4 12h16" />
                  <path stroke-linecap="round" stroke-linejoin="round" d="M4 18h10" />
                  <circle cx="16" cy="6" r="2" />
                  <circle cx="10" cy="12" r="2" />
                  <circle cx="16" cy="18" r="2" />
                </svg>
              </button>

              <div v-if="showSettings" class="absolute bottom-10 right-0 w-64 rounded-lg border border-white/10 bg-black/90 p-2 shadow-xl">
                <div class="px-2 py-1 text-[11px] uppercase tracking-wide text-white/60">Playback Speed</div>
                <button
                  v-for="speed in speedOptions"
                  :key="speed.id"
                  type="button"
                  class="flex w-full items-center justify-between rounded px-2 py-1.5 text-left text-sm hover:bg-white/10"
                  @click="setPlaybackRate(speed.id)"
                >
                  <span>{{ speed.label }}</span>
                  <span v-if="activeSpeed === speed.id" class="text-xs text-primary-300">Active</span>
                </button>

                <div class="mt-2 px-2 py-1 text-[11px] uppercase tracking-wide text-white/60">Quality</div>
                <button
                  v-for="quality in qualityOptions"
                  :key="quality.id"
                  type="button"
                  class="flex w-full items-center justify-between rounded px-2 py-1.5 text-left text-sm hover:bg-white/10"
                  @click="selectQuality(quality)"
                >
                  <span>{{ quality.label }}</span>
                  <span v-if="quality.needsTranscoding" class="text-[10px] text-white/50">stub</span>
                  <span v-else-if="activeQuality === quality.id" class="text-xs text-primary-300">Active</span>
                </button>
              </div>
            </div>

            <button type="button" class="video-icon-btn" @click="togglePiP">
              <svg class="h-5 w-5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.8" aria-hidden="true">
                <path stroke-linecap="round" stroke-linejoin="round" d="M3 5h18v14H3V5Zm11 6h6v5h-6v-5Z" />
              </svg>
            </button>

            <button type="button" class="video-icon-btn" @click="toggleFullscreen">
              <svg v-if="!fullscreenActive" class="h-5 w-5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.8" aria-hidden="true">
                <path stroke-linecap="round" stroke-linejoin="round" d="M4 9V4h5M15 4h5v5M20 15v5h-5M9 20H4v-5" />
              </svg>
              <svg v-else class="h-5 w-5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.8" aria-hidden="true">
                <path stroke-linecap="round" stroke-linejoin="round" d="M9 4H4v5M20 9V4h-5M15 20h5v-5M4 15v5h5" />
              </svg>
            </button>
          </div>
        </div>
      </div>

      <div v-if="isPiP" class="absolute right-3 top-3 rounded-md bg-black/70 px-2 py-1 text-[10px] font-medium uppercase tracking-wide text-white/90">
        PiP
      </div>
    </div>

    <div v-else class="flex aspect-video flex-col items-center justify-center rounded-lg border border-dashed border-border bg-muted/30">
      <svg class="mb-3 h-12 w-12 text-muted-foreground/50" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.5">
        <path stroke-linecap="round" stroke-linejoin="round" d="m15.75 10.5 4.72-4.72a.75.75 0 0 1 1.28.53v11.38a.75.75 0 0 1-1.28.53l-4.72-4.72M4.5 18.75h9a2.25 2.25 0 0 0 2.25-2.25v-9a2.25 2.25 0 0 0-2.25-2.25h-9A2.25 2.25 0 0 0 2.25 7.5v9a2.25 2.25 0 0 0 2.25 2.25Z" />
      </svg>
      <p class="text-sm font-medium text-muted-foreground">{{ title ?? 'Video' }}</p>
      <p class="mt-1 text-xs text-muted-foreground/60">Video content not yet available on this node</p>
    </div>
  </div>
</template>

<style scoped>
.video-icon-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border-radius: 0.375rem;
  padding: 0.375rem;
  color: rgba(255, 255, 255, 0.94);
  transition: background-color 120ms ease, color 120ms ease;
}

.video-icon-btn:hover {
  background: rgba(255, 255, 255, 0.14);
}

.video-icon-btn--cc {
  font-size: 0.75rem;
  font-weight: 700;
  letter-spacing: 0.02em;
  line-height: 1;
}

.yt-range {
  --played: 0%;
  appearance: none;
  background: linear-gradient(to right, #ff3344 var(--played), rgba(255, 255, 255, 0.35) var(--played));
  border-radius: 999px;
  outline: none;
}

.yt-range-progress {
  --buffered: 0%;
  background:
    linear-gradient(to right, #ff3344 var(--played), transparent var(--played)),
    linear-gradient(to right, rgba(255, 255, 255, 0.45) var(--buffered), rgba(255, 255, 255, 0.25) var(--buffered));
}

.yt-range::-webkit-slider-thumb {
  appearance: none;
  width: 0.9rem;
  height: 0.9rem;
  border-radius: 999px;
  border: 2px solid #ffffff;
  background: #ff3344;
}

.yt-range::-moz-range-thumb {
  width: 0.9rem;
  height: 0.9rem;
  border-radius: 999px;
  border: 2px solid #ffffff;
  background: #ff3344;
}
</style>
