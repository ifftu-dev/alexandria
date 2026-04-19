<script setup lang="ts">
import { ref, onMounted, onUnmounted } from 'vue'
import jsQR from 'jsqr'

const emit = defineEmits<{
  scan: [value: string]
  error: [message: string]
}>()

const videoRef = ref<HTMLVideoElement | null>(null)
const canvasRef = ref<HTMLCanvasElement | null>(null)
const status = ref<'requesting' | 'scanning' | 'denied' | 'unsupported' | 'error'>('requesting')
const errorMessage = ref<string | null>(null)

let stream: MediaStream | null = null
let rafId: number | null = null
let stopped = false

async function start() {
  if (!navigator.mediaDevices?.getUserMedia) {
    status.value = 'unsupported'
    emit('error', 'Camera API not available on this platform')
    return
  }

  try {
    stream = await navigator.mediaDevices.getUserMedia({
      video: { facingMode: { ideal: 'environment' } },
      audio: false,
    })
  } catch (e) {
    const err = e as DOMException
    if (err.name === 'NotAllowedError' || err.name === 'PermissionDeniedError') {
      status.value = 'denied'
      emit('error', 'Camera permission denied')
    } else {
      status.value = 'error'
      errorMessage.value = err.message || String(e)
      emit('error', errorMessage.value)
    }
    return
  }

  if (!videoRef.value) return
  videoRef.value.srcObject = stream
  await videoRef.value.play()
  status.value = 'scanning'
  scanLoop()
}

function scanLoop() {
  if (stopped) return
  const video = videoRef.value
  const canvas = canvasRef.value
  if (!video || !canvas || video.readyState < 2) {
    rafId = requestAnimationFrame(scanLoop)
    return
  }

  const width = video.videoWidth
  const height = video.videoHeight
  if (width === 0 || height === 0) {
    rafId = requestAnimationFrame(scanLoop)
    return
  }

  canvas.width = width
  canvas.height = height
  const ctx = canvas.getContext('2d', { willReadFrequently: true })
  if (!ctx) {
    rafId = requestAnimationFrame(scanLoop)
    return
  }

  ctx.drawImage(video, 0, 0, width, height)
  const imageData = ctx.getImageData(0, 0, width, height)
  const code = jsQR(imageData.data, width, height, { inversionAttempts: 'dontInvert' })

  if (code && code.data) {
    stop()
    emit('scan', code.data)
    return
  }

  rafId = requestAnimationFrame(scanLoop)
}

function stop() {
  stopped = true
  if (rafId !== null) {
    cancelAnimationFrame(rafId)
    rafId = null
  }
  if (stream) {
    stream.getTracks().forEach(t => t.stop())
    stream = null
  }
  if (videoRef.value) {
    videoRef.value.srcObject = null
  }
}

onMounted(start)
onUnmounted(stop)

defineExpose({ stop })
</script>

<template>
  <div class="relative overflow-hidden rounded-lg bg-black">
    <video
      ref="videoRef"
      class="block w-full aspect-square object-cover"
      autoplay
      muted
      playsinline
    />
    <canvas ref="canvasRef" class="hidden" />

    <!-- Scan overlay -->
    <div v-if="status === 'scanning'" class="pointer-events-none absolute inset-0 flex items-center justify-center">
      <div class="h-3/5 w-3/5 rounded-lg border-2 border-white/80 shadow-[0_0_0_9999px_rgba(0,0,0,0.4)]" />
    </div>

    <!-- Status messages -->
    <div
      v-if="status !== 'scanning'"
      class="absolute inset-0 flex items-center justify-center bg-black/70 p-4 text-center text-sm text-white"
    >
      <template v-if="status === 'requesting'">
        <p>Requesting camera access…</p>
      </template>
      <template v-else-if="status === 'denied'">
        <p>Camera permission denied. Enable camera access in system settings and try again.</p>
      </template>
      <template v-else-if="status === 'unsupported'">
        <p>Camera is not available on this device.</p>
      </template>
      <template v-else>
        <p>{{ errorMessage || 'Camera error' }}</p>
      </template>
    </div>
  </div>
</template>
