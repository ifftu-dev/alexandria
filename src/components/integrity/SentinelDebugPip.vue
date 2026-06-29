<script setup lang="ts">
/**
 * Sentinel debug PiP (dev only) — a floating picture-in-picture window
 * showing what the Sentinel engine "sees" live: the camera feed with the
 * YuNet face box + 5 landmarks + gaze direction overlaid, plus the live
 * signal readout from the active monitoring session.
 *
 * Runs its own camera preview (independent of any course session) so it
 * works for tuning even outside an assessment; the numeric session
 * signals come from `useSentinel().debug`, populated by the real session.
 * Gated behind `import.meta.env.DEV` by the caller.
 */
import { ref, onMounted, onBeforeUnmount } from 'vue'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { useLocalApi } from '@/composables/useLocalApi'
import { useSentinel } from '@/composables/useSentinel'
import type { FaceDetection, ScoreGazeResponse } from '@/types'

const { invoke } = useLocalApi()
const { debug } = useSentinel()

const open = ref(false)
const cameraOn = ref(false)
const camError = ref<string | null>(null)
const canvasRef = ref<HTMLCanvasElement | null>(null)
const videoRef = ref<HTMLVideoElement | null>(null)

const lastGaze = ref<ScoreGazeResponse['estimate'] | null>(null)
const lastDetections = ref<FaceDetection[]>([])
const inferMs = ref(0)

let stream: MediaStream | null = null
let drawTimer: ReturnType<typeof setInterval> | null = null
let inferTimer: ReturnType<typeof setInterval> | null = null
let busy = false

const PREVIEW_W = 224

async function startCamera() {
  camError.value = null
  try {
    stream = await navigator.mediaDevices.getUserMedia({
      video: { width: 320, height: 240, facingMode: 'user' },
      audio: false,
    })
    cameraOn.value = true
    await Promise.resolve()
    if (videoRef.value) {
      videoRef.value.srcObject = stream
      await videoRef.value.play().catch(() => {})
    }
    drawTimer = setInterval(draw, 100)
    inferTimer = setInterval(infer, 700)
  } catch (e) {
    camError.value = e instanceof Error ? e.message : 'camera unavailable'
  }
}

function stopCamera() {
  if (drawTimer) { clearInterval(drawTimer); drawTimer = null }
  if (inferTimer) { clearInterval(inferTimer); inferTimer = null }
  if (stream) { stream.getTracks().forEach(t => t.stop()); stream = null }
  cameraOn.value = false
  lastGaze.value = null
  lastDetections.value = []
}

function frameSize() {
  const v = videoRef.value
  const vw = v?.videoWidth || 320
  const vh = v?.videoHeight || 240
  const scale = Math.min(1, PREVIEW_W / vw)
  return { w: Math.max(1, Math.round(vw * scale)), h: Math.max(1, Math.round(vh * scale)) }
}

function draw() {
  const v = videoRef.value
  const c = canvasRef.value
  if (!v || !c || v.readyState < 2) return
  const { w, h } = frameSize()
  if (c.width !== w) c.width = w
  if (c.height !== h) c.height = h
  const ctx = c.getContext('2d')
  if (!ctx) return
  ctx.drawImage(v, 0, 0, w, h)

  // Overlay the latest detection + gaze.
  for (const d of lastDetections.value) {
    const [x, y, bw, bh] = d.bbox
    ctx.strokeStyle = '#10b981'
    ctx.lineWidth = 2
    ctx.strokeRect(x, y, bw, bh)
    ctx.fillStyle = '#f59e0b'
    for (const [lx, ly] of d.landmarks5) {
      ctx.beginPath()
      ctx.arc(lx, ly, 2, 0, Math.PI * 2)
      ctx.fill()
    }
  }
  const g = lastGaze.value
  if (g && lastDetections.value[0] && !g.occluded) {
    const [bx, by, bw, bh] = lastDetections.value[0].bbox
    const cx = bx + bw / 2
    const cy = by + bh / 2
    // yaw/pitch proxies are roughly [-1,1]; scale to a visible vector.
    const dx = g.yaw * bw * 0.9
    const dy = g.pitch * bh * 0.9
    ctx.strokeStyle = g.onScreen ? '#22c55e' : '#ef4444'
    ctx.lineWidth = 3
    ctx.beginPath()
    ctx.moveTo(cx, cy)
    ctx.lineTo(cx + dx, cy + dy)
    ctx.stroke()
  }
}

async function infer() {
  if (busy) return
  const v = videoRef.value
  // Skip work when the window/tab is hidden — nothing to observe.
  if (!v || v.readyState < 2 || document.hidden) return
  busy = true
  try {
    const { w, h } = frameSize()
    const off = document.createElement('canvas')
    off.width = w
    off.height = h
    const octx = off.getContext('2d', { willReadFrequently: true })
    if (!octx) return
    octx.drawImage(v, 0, 0, w, h)
    const img = octx.getImageData(0, 0, w, h)
    const frame = { width: w, height: h, rgba: Array.from(img.data) }
    const t0 = performance.now()
    // One YuNet pass — score_gaze returns the best detection for overlay.
    const gaze = await invoke<ScoreGazeResponse>('sentinel_score_gaze', {
      req: { frame, user_address: 'debug-pip', device_fp_prefix: 'debugpip' },
    })
    inferMs.value = Math.round(performance.now() - t0)
    lastDetections.value = gaze?.detection ? [gaze.detection] : []
    lastGaze.value = gaze?.estimate ?? null
  } catch {
    /* dev tool — ignore transient IPC errors */
  } finally {
    busy = false
  }
}

function toggle() {
  open.value = !open.value
  if (!open.value) stopCamera()
}

// Activation comes from the native Develop menu ("Sentinel Live View",
// ⌘⇧S) rather than an in-app button.
let unlistenToggle: UnlistenFn | null = null
onMounted(async () => {
  try {
    unlistenToggle = await listen('develop://toggle-sentinel', () => toggle())
  } catch { /* not in a Tauri context */ }
})

function pct(n: number) {
  return n < 0 ? 'n/a' : `${Math.round(n * 100)}%`
}
function fmt(v?: number | null, d = 2) {
  return v == null ? '—' : v.toFixed(d)
}
function yn(v?: boolean | null) {
  return v == null ? '—' : v ? 'yes' : 'no'
}

onBeforeUnmount(() => {
  stopCamera()
  if (unlistenToggle) { unlistenToggle(); unlistenToggle = null }
})
</script>

<template>
  <Teleport to="body">
    <!-- Panel (toggled from Develop → Sentinel Live View, ⌘⇧S) -->
    <div
      v-if="open"
      class="fixed bottom-16 right-4 z-[200] w-72 overflow-hidden rounded-xl border border-border bg-card shadow-2xl"
    >
      <div class="flex items-center justify-between border-b border-border px-3 py-2">
        <span class="text-xs font-semibold text-foreground">Sentinel — live view</span>
        <div class="flex items-center gap-2">
          <span class="rounded bg-amber-100 px-1.5 py-0.5 text-[10px] font-medium text-amber-700 dark:bg-amber-900/30 dark:text-amber-400">DEV</span>
          <button class="text-muted-foreground hover:text-foreground" title="Close (⌘⇧S)" @click="toggle">
            <svg class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2"><path stroke-linecap="round" stroke-linejoin="round" d="M6 18L18 6M6 6l12 12" /></svg>
          </button>
        </div>
      </div>

      <!-- Camera + overlay -->
      <div class="relative bg-black">
        <video ref="videoRef" class="hidden" muted playsinline />
        <canvas ref="canvasRef" class="w-full" />
        <div v-if="!cameraOn" class="flex items-center justify-center py-8">
          <button class="rounded-md bg-primary px-3 py-1.5 text-xs font-medium text-primary-foreground" @click="startCamera">
            Start camera preview
          </button>
        </div>
        <span v-if="cameraOn && lastGaze" class="absolute left-1.5 top-1.5 rounded px-1.5 py-0.5 text-[10px] font-medium"
          :class="lastGaze.occluded ? 'bg-gray-500 text-white' : lastGaze.onScreen ? 'bg-emerald-500 text-white' : 'bg-red-500 text-white'">
          {{ lastGaze.occluded ? 'no face' : lastGaze.onScreen ? 'on-screen' : 'OFF-SCREEN' }}
        </span>
      </div>
      <p v-if="camError" class="px-3 py-1 text-[11px] text-red-500">{{ camError }}</p>

      <!-- Readout — full tracked signal set -->
      <div class="max-h-[46vh] overflow-y-auto px-3 py-2">
        <div class="grid grid-cols-2 gap-x-3 gap-y-1 text-[11px]">
          <!-- Outcome -->
          <p class="col-span-2 mt-0.5 text-[10px] font-semibold uppercase tracking-wide text-muted-foreground/70">Outcome</p>
          <span class="text-muted-foreground">session</span>
          <span class="text-right font-mono" :class="debug.active ? 'text-emerald-500' : 'text-muted-foreground'">{{ debug.active ? 'active' : 'idle' }}</span>
          <span class="text-muted-foreground">integrity</span>
          <span class="text-right font-mono text-foreground">{{ pct(debug.integrity) }}</span>
          <span class="text-muted-foreground">consistency</span>
          <span class="text-right font-mono text-foreground">{{ pct(debug.consistency) }}</span>

          <!-- Typing -->
          <p class="col-span-2 mt-1.5 text-[10px] font-semibold uppercase tracking-wide text-muted-foreground/70">Typing</p>
          <span class="text-muted-foreground">consistency</span>
          <span class="text-right font-mono text-foreground">{{ fmt(debug.signals?.typing_consistency) }}</span>
          <span class="text-muted-foreground">speed</span>
          <span class="text-right font-mono text-foreground">{{ fmt(debug.signals?.typing_speed_wpm, 0) }} wpm</span>
          <span class="text-muted-foreground">keystroke buf (live)</span>
          <span class="text-right font-mono text-foreground">{{ debug.keystrokeBufferLen }}</span>

          <!-- Mouse -->
          <p class="col-span-2 mt-1.5 text-[10px] font-semibold uppercase tracking-wide text-muted-foreground/70">Mouse</p>
          <span class="text-muted-foreground">consistency</span>
          <span class="text-right font-mono text-foreground">{{ fmt(debug.signals?.mouse_consistency) }}</span>
          <span class="text-muted-foreground">human-like</span>
          <span class="text-right font-mono" :class="debug.signals?.is_human_likely === false ? 'text-red-500' : 'text-foreground'">{{ yn(debug.signals?.is_human_likely) }}</span>
          <span class="text-muted-foreground">points buf (live)</span>
          <span class="text-right font-mono text-foreground">{{ debug.mouseBufferLen }}</span>

          <!-- Clipboard -->
          <p class="col-span-2 mt-1.5 text-[10px] font-semibold uppercase tracking-wide text-muted-foreground/70">Clipboard</p>
          <span class="text-muted-foreground">paste events</span>
          <span class="text-right font-mono text-foreground">{{ debug.signals?.paste_events ?? 0 }}</span>
          <span class="text-muted-foreground">pasted chars</span>
          <span class="text-right font-mono text-foreground">{{ debug.signals?.pasted_chars ?? 0 }}</span>

          <!-- Environment -->
          <p class="col-span-2 mt-1.5 text-[10px] font-semibold uppercase tracking-wide text-muted-foreground/70">Environment</p>
          <span class="text-muted-foreground">tab switches</span>
          <span class="text-right font-mono text-foreground">{{ debug.signals?.tab_switches ?? 0 }}</span>
          <span class="text-muted-foreground">unfocused</span>
          <span class="text-right font-mono text-foreground">{{ Math.round((debug.signals?.unfocused_ms ?? 0) / 1000) }}s</span>
          <span class="text-muted-foreground">devtools</span>
          <span class="text-right font-mono" :class="debug.signals?.devtools_detected ? 'text-red-500' : 'text-foreground'">{{ yn(debug.signals?.devtools_detected) }}</span>
          <span class="text-muted-foreground">env changed</span>
          <span class="text-right font-mono" :class="debug.signals?.environment_changed ? 'text-red-500' : 'text-foreground'">{{ yn(debug.signals?.environment_changed) }}</span>
          <span class="text-muted-foreground">app focus lost</span>
          <span class="text-right font-mono" :class="debug.appFocusLostCount ? 'text-red-500' : 'text-foreground'">{{ debug.appFocusLostCount }}× / {{ Math.round(debug.appFocusLostMs / 1000) }}s</span>
          <span class="text-muted-foreground">switched to</span>
          <span class="text-right font-mono text-foreground truncate">{{ debug.lastApp || '—' }}</span>

          <!-- Camera -->
          <p class="col-span-2 mt-1.5 text-[10px] font-semibold uppercase tracking-wide text-muted-foreground/70">Camera</p>
          <span class="text-muted-foreground">faces (live)</span>
          <span class="text-right font-mono text-foreground">{{ lastDetections.length }}</span>
          <span class="text-muted-foreground">face present</span>
          <span class="text-right font-mono text-foreground">{{ yn(debug.signals?.face_present) }}</span>
          <span class="text-muted-foreground">face consistency</span>
          <span class="text-right font-mono text-foreground">{{ fmt(debug.signals?.face_consistency) }}</span>
          <span class="text-muted-foreground">ai face sim / match</span>
          <span class="text-right font-mono text-foreground">{{ fmt(debug.signals?.ai_face_similarity) }} / {{ yn(debug.signals?.ai_face_match) }}</span>

          <!-- Gaze -->
          <p class="col-span-2 mt-1.5 text-[10px] font-semibold uppercase tracking-wide text-muted-foreground/70">Gaze</p>
          <span class="text-muted-foreground">session checks</span>
          <span class="text-right font-mono" :class="debug.active ? 'text-emerald-500' : 'text-muted-foreground'">{{ debug.sessionGazeChecks }} <span class="text-muted-foreground/70">(~1/s)</span></span>
          <span class="text-muted-foreground">session read</span>
          <span class="text-right font-mono text-foreground">{{ debug.sessionGazeChecks ? `${debug.sessionGazeYaw.toFixed(2)} / ${debug.sessionGazePitch.toFixed(2)}` : '—' }}</span>
          <span class="text-muted-foreground">preview yaw / pitch</span>
          <span class="text-right font-mono text-foreground">{{ lastGaze ? `${lastGaze.yaw.toFixed(2)} / ${lastGaze.pitch.toFixed(2)}` : '—' }}</span>
          <span class="text-muted-foreground">off-screen ratio</span>
          <span class="text-right font-mono text-foreground">{{ debug.gazeOffscreenRatio != null ? pct(debug.gazeOffscreenRatio) : '—' }}</span>
          <span class="text-muted-foreground">down-glances</span>
          <span class="text-right font-mono text-foreground">{{ debug.gazeDownGlances }}/{{ debug.gazeTotalChecks }}</span>
          <span class="text-muted-foreground">occluded checks</span>
          <span class="text-right font-mono text-foreground">{{ debug.gazeOccludedChecks }}</span>

          <!-- AI scores -->
          <p class="col-span-2 mt-1.5 text-[10px] font-semibold uppercase tracking-wide text-muted-foreground/70">AI scores</p>
          <span class="text-muted-foreground">paste anomaly</span>
          <span class="text-right font-mono text-foreground">{{ debug.signals?.ai_paste_anomaly != null ? pct(debug.signals.ai_paste_anomaly) : 'n/a' }}</span>
          <span class="text-muted-foreground">keystroke anomaly</span>
          <span class="text-right font-mono text-foreground">{{ debug.signals?.ai_keystroke_anomaly != null ? pct(debug.signals.ai_keystroke_anomaly) : 'n/a' }}</span>
          <span class="text-muted-foreground">mouse human prob</span>
          <span class="text-right font-mono text-foreground">{{ debug.signals?.ai_mouse_human_prob != null ? pct(debug.signals.ai_mouse_human_prob) : 'n/a' }}</span>
          <span class="text-muted-foreground">infer latency</span>
          <span class="text-right font-mono text-foreground">{{ inferMs }}ms</span>
        </div>
      </div>
      <div v-if="debug.flags.length" class="flex flex-wrap gap-1 px-3 pb-2">
        <span v-for="f in debug.flags" :key="f" class="rounded bg-red-100 px-1.5 py-0.5 text-[10px] text-red-700 dark:bg-red-900/30 dark:text-red-400">{{ f }}</span>
      </div>
      <div v-if="cameraOn" class="border-t border-border px-3 py-1.5 text-right">
        <button class="text-[11px] text-muted-foreground hover:text-foreground" @click="stopCamera">stop camera</button>
      </div>
    </div>
  </Teleport>
</template>
