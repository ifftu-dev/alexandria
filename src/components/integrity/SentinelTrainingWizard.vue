<script setup lang="ts">
/**
 * SentinelTrainingWizard — Guided calibration flow for building a behavioral
 * profile. Walks the user through typing and mouse exercises to establish
 * baseline patterns. All data stays on-device.
 */
import { ref, computed, nextTick, onBeforeUnmount } from 'vue'
import { AppButton } from '@/components/ui'
import { useSentinel } from '@/composables/useSentinel'

const emit = defineEmits<{
  complete: []
  cancel: []
}>()

const {
  startTrainingKeystrokes,
  startTrainingMouse,
  getTrainingMetrics,
  clearTrainingBuffers,
  saveTrainingProfile,
  resetProfile,
  getProfile,
  reportFaceDetection,
  trainAIModels,
  enrollFace,
} = useSentinel()

type Step = 'welcome' | 'typing' | 'mouse' | 'awareness' | 'camera' | 'review'
const steps: Step[] = ['welcome', 'typing', 'mouse', 'awareness', 'camera', 'review']

const currentStep = ref<Step>('welcome')
const currentStepIndex = computed(() => steps.indexOf(currentStep.value))
const progress = computed(() => ((currentStepIndex.value) / (steps.length - 1)) * 100)

// Typing calibration state
const typingText = 'The quick brown fox jumps over the lazy dog. Programming requires careful attention to detail and consistent practice to build strong problem-solving skills.'
const typedText = ref('')
const typingMetrics = ref<{ speedWpm: number; avgDwellMs: number; avgFlightMs: number; keystrokeCount: number } | null>(null)
const typingComplete = ref(false)
let typingCleanup: (() => void) | null = null
let typingPollTimer: ReturnType<typeof setInterval> | null = null

// Mouse calibration state
const mouseTargets = ref<{ id: number; x: number; y: number; hit: boolean }[]>([])
const mouseMetrics = ref<{ consistency: number; isHuman: boolean; moveCount: number; clickCount: number } | null>(null)
const mouseComplete = ref(false)
let mouseCleanup: (() => void) | null = null
let mousePollTimer: ReturnType<typeof setInterval> | null = null

// Camera state
const cameraEnabled = ref(false)
const cameraError = ref<string | null>(null)
const faceDetected = ref(false)
const cameraSkipped = ref(false)
const videoRef = ref<HTMLVideoElement | null>(null)
let cameraStream: MediaStream | null = null
let faceDetectionInterval: ReturnType<typeof setInterval> | null = null

// Review state
const savedProfile = ref<Record<string, unknown> | null>(null)
const saving = ref(false)
const aiTrainingResults = ref<{
  keystrokeAE: { trained: boolean; loss: number; samples: number }
  mouseCNN: { trained: boolean; loss: number; samples: number; priorTrajectories: number }
  faceEmbedder: { enrolled: boolean; progress: number }
} | null>(null)

// =========================================================================
// Step navigation
// =========================================================================
const goToStep = (step: Step) => {
  cleanupCurrentStep()
  currentStep.value = step
  nextTick(() => initStep(step))
}

const nextStep = () => {
  const idx = currentStepIndex.value
  if (idx < steps.length - 1) {
    goToStep(steps[idx + 1]!)
  }
}

const prevStep = () => {
  const idx = currentStepIndex.value
  if (idx > 0) {
    goToStep(steps[idx - 1]!)
  }
}

// =========================================================================
// Step initialization
// =========================================================================
const initStep = (step: Step) => {
  if (step === 'typing') {
    typedText.value = ''
    typingComplete.value = false
    typingMetrics.value = null
    clearTrainingBuffers()
    typingCleanup = startTrainingKeystrokes()
    typingPollTimer = setInterval(() => {
      const m = getTrainingMetrics()
      typingMetrics.value = {
        speedWpm: m.typing.speedWpm,
        avgDwellMs: m.typing.avgDwellMs,
        avgFlightMs: m.typing.avgFlightMs,
        keystrokeCount: m.keystrokeCount,
      }
    }, 300)
  }
  else if (step === 'mouse') {
    mouseComplete.value = false
    mouseMetrics.value = null
    clearTrainingBuffers()
    generateMouseTargets()
    mouseCleanup = startTrainingMouse()
    mousePollTimer = setInterval(() => {
      const m = getTrainingMetrics()
      mouseMetrics.value = {
        consistency: m.mouse.consistency,
        isHuman: m.mouse.isHuman,
        moveCount: m.mouseMoveCount,
        clickCount: m.mouseClickCount,
      }
    }, 300)
  }
  else if (step === 'review') {
    loadReviewData()
  }
}

const cleanupCurrentStep = () => {
  if (typingCleanup) { typingCleanup(); typingCleanup = null }
  if (typingPollTimer) { clearInterval(typingPollTimer); typingPollTimer = null }
  if (mouseCleanup) { mouseCleanup(); mouseCleanup = null }
  if (mousePollTimer) { clearInterval(mousePollTimer); mousePollTimer = null }
  stopCamera()
}

// =========================================================================
// Typing step
// =========================================================================
const onTypingInput = (e: Event) => {
  const target = e.target as HTMLTextAreaElement
  typedText.value = target.value
  if (typedText.value.length >= typingText.length * 0.8) {
    typingComplete.value = true
  }
}

const finishTyping = async () => {
  await saveTrainingProfile()
  nextStep()
}

// =========================================================================
// Mouse step
// =========================================================================
const generateMouseTargets = () => {
  const targets: { id: number; x: number; y: number; hit: boolean }[] = []
  for (let i = 0; i < 8; i++) {
    targets.push({
      id: i,
      x: 10 + Math.random() * 80,
      y: 10 + Math.random() * 80,
      hit: false,
    })
  }
  mouseTargets.value = targets
}

const hitTarget = (id: number) => {
  const target = mouseTargets.value.find(t => t.id === id)
  if (target) target.hit = true
  if (mouseTargets.value.every(t => t.hit)) {
    mouseComplete.value = true
  }
}

const finishMouse = async () => {
  await saveTrainingProfile()
  nextStep()
}

// =========================================================================
// Camera step
// =========================================================================
const enableCamera = async () => {
  cameraError.value = null
  try {
    cameraStream = await navigator.mediaDevices.getUserMedia({
      video: { width: 320, height: 240, facingMode: 'user' },
      audio: false,
    })
    cameraEnabled.value = true
    await nextTick()
    if (videoRef.value) {
      videoRef.value.srcObject = cameraStream
      await videoRef.value.play()
    }
    startFaceDetection()
  }
  catch (err) {
    const error = err as Error
    if (error.name === 'NotAllowedError') {
      cameraError.value = 'Camera permission was denied.'
    }
    else if (error.name === 'NotFoundError') {
      cameraError.value = 'No camera found on this device.'
    }
    else {
      cameraError.value = 'Could not access camera.'
    }
  }
}

const startFaceDetection = () => {
  if (faceDetectionInterval) clearInterval(faceDetectionInterval)
  const canvas = document.createElement('canvas')
  canvas.width = 320
  canvas.height = 240
  const ctx = canvas.getContext('2d', { willReadFrequently: true })

  faceDetectionInterval = setInterval(() => {
    if (!videoRef.value || !ctx || videoRef.value.readyState < 2) return
    ctx.drawImage(videoRef.value, 0, 0, 320, 240)
    const imageData = ctx.getImageData(0, 0, 320, 240)
    const data = imageData.data

    let skinPixels = 0
    for (let i = 0; i < data.length; i += 16) {
      const r = data[i]!
      const g = data[i + 1]!
      const b = data[i + 2]!
      const y = 0.299 * r + 0.587 * g + 0.114 * b
      const cb = 128 - 0.168736 * r - 0.331264 * g + 0.5 * b
      const cr = 128 + 0.5 * r - 0.418688 * g - 0.081312 * b
      if (y > 80 && cb > 77 && cb < 127 && cr > 133 && cr < 173) skinPixels++
    }

    const skinRatio = skinPixels / (320 * 240 / 4)
    const present = skinRatio > 0.04
    faceDetected.value = present
    reportFaceDetection(present, present ? 1 : 0, present ? 0.8 : 0.2)

    if (present && videoRef.value) {
      enrollFace(videoRef.value)
    }
  }, 2000)
}

const stopCamera = () => {
  if (faceDetectionInterval) { clearInterval(faceDetectionInterval); faceDetectionInterval = null }
  if (cameraStream) { cameraStream.getTracks().forEach(t => t.stop()); cameraStream = null }
  cameraEnabled.value = false
  faceDetected.value = false
}

const skipCamera = () => {
  cameraSkipped.value = true
  nextStep()
}

// =========================================================================
// Review step
// =========================================================================
const loadReviewData = async () => {
  aiTrainingResults.value = await trainAIModels()
  savedProfile.value = getProfile() as unknown as Record<string, unknown>
}

const finishWizard = async () => {
  saving.value = true
  await saveTrainingProfile()
  saving.value = false
  emit('complete')
}

const restartWizard = async () => {
  await resetProfile()
  typedText.value = ''
  typingComplete.value = false
  typingMetrics.value = null
  mouseComplete.value = false
  mouseMetrics.value = null
  cameraEnabled.value = false
  cameraSkipped.value = false
  savedProfile.value = null
  aiTrainingResults.value = null
  goToStep('welcome')
}

onBeforeUnmount(() => {
  cleanupCurrentStep()
})
</script>

<template>
  <div class="card">
    <!-- Progress bar -->
    <div class="h-1 overflow-hidden rounded-t-xl bg-muted">
      <div
        class="h-full rounded-full bg-primary transition-all duration-500"
        :style="{ width: `${progress}%` }"
      />
    </div>

    <!-- Step indicators -->
    <div class="flex items-center justify-center gap-2 px-6 pt-5">
      <button
        v-for="(step, i) in steps"
        :key="step"
        class="flex items-center gap-1.5"
        :class="i <= currentStepIndex ? 'cursor-pointer' : 'cursor-default'"
        @click="i <= currentStepIndex ? goToStep(step) : undefined"
      >
        <div
          class="flex h-6 w-6 items-center justify-center rounded-full text-xs font-medium transition-colors"
          :class="i < currentStepIndex
            ? 'bg-primary text-white'
            : i === currentStepIndex
              ? 'border-2 border-primary text-primary'
              : 'border border-border text-muted-foreground'"
        >
          <svg v-if="i < currentStepIndex" class="h-3.5 w-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="3">
            <path stroke-linecap="round" stroke-linejoin="round" d="M5 13l4 4L19 7" />
          </svg>
          <span v-else>{{ i + 1 }}</span>
        </div>
        <span
          v-if="i < steps.length - 1"
          class="hidden h-px w-4 sm:block"
          :class="i < currentStepIndex ? 'bg-primary' : 'bg-border'"
        />
      </button>
    </div>

    <div class="p-6">
      <!-- ================ WELCOME ================ -->
      <div v-if="currentStep === 'welcome'" class="mx-auto max-w-lg text-center">
        <div class="mx-auto mb-5 flex h-16 w-16 items-center justify-center rounded-2xl bg-primary/10">
          <svg class="h-8 w-8 text-primary" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.5">
            <path stroke-linecap="round" stroke-linejoin="round" d="M9 12.75L11.25 15 15 9.75m-3-7.036A11.959 11.959 0 013.598 6 11.99 11.99 0 003 9.749c0 5.592 3.824 10.29 9 11.623 5.176-1.332 9-6.03 9-11.622 0-1.31-.21-2.571-.598-3.751h-.152c-3.196 0-6.1-1.248-8.25-3.285z" />
          </svg>
        </div>
        <h2 class="mb-2 text-lg font-bold text-foreground">
          Train Sentinel
        </h2>
        <p class="mb-4 text-sm text-muted-foreground">
          This short calibration teaches Sentinel how you naturally type and move your mouse.
          It takes about 2 minutes and helps ensure your assessments are accurately attributed to you.
        </p>
        <div class="mb-6 rounded-lg border border-emerald-200 bg-emerald-50 p-3 text-left dark:border-emerald-800/40 dark:bg-emerald-900/20">
          <p class="text-xs text-emerald-700 dark:text-emerald-400">
            Everything stays on your device. We never store raw keystrokes, mouse coordinates, or video.
            Only a statistical profile (averages and sample counts) is saved locally.
          </p>
        </div>
        <AppButton variant="primary" @click="nextStep">
          Begin Calibration
        </AppButton>
      </div>

      <!-- ================ TYPING CALIBRATION ================ -->
      <div v-else-if="currentStep === 'typing'">
        <div class="mb-4">
          <h2 class="text-base font-semibold text-foreground">Typing Calibration</h2>
          <p class="mt-1 text-sm text-muted-foreground">
            Type the text below naturally. Sentinel will learn your keystroke rhythm -- how long you hold each key and the gaps between them.
          </p>
        </div>

        <!-- Target text -->
        <div class="mb-4 rounded-lg bg-muted/50 p-4 text-sm leading-relaxed text-foreground">
          <span
            v-for="(char, i) in typingText.split('')"
            :key="i"
            :class="i < typedText.length
              ? typedText[i] === char
                ? 'text-emerald-600 dark:text-emerald-400'
                : 'text-red-500 underline'
              : i === typedText.length
                ? 'bg-primary/20 text-primary'
                : 'text-muted-foreground'"
          >{{ char }}</span>
        </div>

        <!-- Input area -->
        <textarea
          class="input mb-4 min-h-[5rem] w-full resize-none font-mono text-sm"
          placeholder="Start typing here..."
          :value="typedText"
          autocomplete="off"
          autocorrect="off"
          autocapitalize="off"
          spellcheck="false"
          @input="onTypingInput"
        />

        <!-- Live metrics -->
        <div class="mb-4 grid grid-cols-4 gap-3">
          <div class="rounded-lg bg-muted/30 p-3 text-center">
            <p class="text-xs text-muted-foreground">Speed</p>
            <p class="mt-0.5 font-mono text-lg font-bold text-foreground">
              {{ typingMetrics?.speedWpm ?? 0 }}
            </p>
            <p class="text-xs text-muted-foreground">WPM</p>
          </div>
          <div class="rounded-lg bg-muted/30 p-3 text-center">
            <p class="text-xs text-muted-foreground">Dwell</p>
            <p class="mt-0.5 font-mono text-lg font-bold text-foreground">
              {{ (typingMetrics?.avgDwellMs ?? 0).toFixed(0) }}
            </p>
            <p class="text-xs text-muted-foreground">ms</p>
          </div>
          <div class="rounded-lg bg-muted/30 p-3 text-center">
            <p class="text-xs text-muted-foreground">Flight</p>
            <p class="mt-0.5 font-mono text-lg font-bold text-foreground">
              {{ (typingMetrics?.avgFlightMs ?? 0).toFixed(0) }}
            </p>
            <p class="text-xs text-muted-foreground">ms</p>
          </div>
          <div class="rounded-lg bg-muted/30 p-3 text-center">
            <p class="text-xs text-muted-foreground">Keys</p>
            <p class="mt-0.5 font-mono text-lg font-bold text-foreground">
              {{ typingMetrics?.keystrokeCount ?? 0 }}
            </p>
            <p class="text-xs text-muted-foreground">total</p>
          </div>
        </div>

        <!-- Completion -->
        <div class="flex items-center justify-between">
          <AppButton variant="ghost" size="sm" @click="prevStep">Back</AppButton>
          <div class="flex items-center gap-3">
            <div class="h-1.5 w-32 overflow-hidden rounded-full bg-muted">
              <div
                class="h-full rounded-full bg-primary transition-all duration-300"
                :style="{ width: `${Math.min(100, (typedText.length / (typingText.length * 0.8)) * 100)}%` }"
              />
            </div>
            <AppButton
              variant="primary"
              size="sm"
              :disabled="!typingComplete"
              @click="finishTyping"
            >
              Continue
            </AppButton>
          </div>
        </div>
      </div>

      <!-- ================ MOUSE CALIBRATION ================ -->
      <div v-else-if="currentStep === 'mouse'">
        <div class="mb-4">
          <h2 class="text-base font-semibold text-foreground">Mouse Calibration</h2>
          <p class="mt-1 text-sm text-muted-foreground">
            Click each target as it appears. Move naturally -- Sentinel is learning your mouse velocity and movement patterns.
          </p>
        </div>

        <!-- Target area -->
        <div
          class="relative mb-4 h-64 overflow-hidden rounded-lg border border-border bg-muted/20 sm:h-80"
        >
          <button
            v-for="target in mouseTargets"
            :key="target.id"
            class="absolute flex items-center justify-center transition-all duration-300"
            :class="target.hit
              ? 'scale-0 opacity-0'
              : 'scale-100 opacity-100 hover:scale-110'"
            :style="{
              left: `${target.x}%`,
              top: `${target.y}%`,
              transform: `translate(-50%, -50%) ${target.hit ? 'scale(0)' : 'scale(1)'}`,
            }"
            @click="hitTarget(target.id)"
          >
            <div class="relative">
              <div class="h-10 w-10 rounded-full border-2 border-primary bg-primary/15" />
              <div class="absolute inset-0 flex items-center justify-center">
                <div class="h-2.5 w-2.5 rounded-full bg-primary" />
              </div>
            </div>
          </button>

          <!-- Completion check -->
          <div
            v-if="mouseComplete"
            class="absolute inset-0 flex items-center justify-center bg-card/90"
          >
            <div class="text-center">
              <div class="mx-auto mb-2 flex h-12 w-12 items-center justify-center rounded-full bg-emerald-100 dark:bg-emerald-900/30">
                <svg class="h-6 w-6 text-emerald-600 dark:text-emerald-400" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                  <path stroke-linecap="round" stroke-linejoin="round" d="M5 13l4 4L19 7" />
                </svg>
              </div>
              <p class="text-sm font-medium text-foreground">All targets hit</p>
            </div>
          </div>
        </div>

        <!-- Live metrics -->
        <div class="mb-4 grid grid-cols-3 gap-3">
          <div class="rounded-lg bg-muted/30 p-3 text-center">
            <p class="text-xs text-muted-foreground">Moves</p>
            <p class="mt-0.5 font-mono text-lg font-bold text-foreground">
              {{ mouseMetrics?.moveCount ?? 0 }}
            </p>
          </div>
          <div class="rounded-lg bg-muted/30 p-3 text-center">
            <p class="text-xs text-muted-foreground">Clicks</p>
            <p class="mt-0.5 font-mono text-lg font-bold text-foreground">
              {{ mouseMetrics?.clickCount ?? 0 }}
            </p>
          </div>
          <div class="rounded-lg bg-muted/30 p-3 text-center">
            <p class="text-xs text-muted-foreground">Targets</p>
            <p class="mt-0.5 font-mono text-lg font-bold text-foreground">
              {{ mouseTargets.filter(t => t.hit).length }}/{{ mouseTargets.length }}
            </p>
          </div>
        </div>

        <div class="flex items-center justify-between">
          <AppButton variant="ghost" size="sm" @click="prevStep">Back</AppButton>
          <AppButton
            variant="primary"
            size="sm"
            :disabled="!mouseComplete"
            @click="finishMouse"
          >
            Continue
          </AppButton>
        </div>
      </div>

      <!-- ================ AWARENESS ================ -->
      <div v-else-if="currentStep === 'awareness'">
        <div class="mb-4">
          <h2 class="text-base font-semibold text-foreground">What Else Sentinel Watches</h2>
          <p class="mt-1 text-sm text-muted-foreground">
            Beyond typing and mouse patterns, Sentinel monitors these signals during assessments. No action needed here -- just be aware.
          </p>
        </div>

        <div class="space-y-3">
          <div class="flex gap-3 rounded-lg border border-border bg-card p-4">
            <div class="flex h-9 w-9 flex-shrink-0 items-center justify-center rounded-lg bg-amber-100 dark:bg-amber-900/30">
              <svg class="h-4 w-4 text-amber-600 dark:text-amber-400" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                <rect x="2" y="3" width="20" height="18" rx="2" /><path d="M2 9h20" />
              </svg>
            </div>
            <div>
              <p class="text-sm font-medium text-foreground">Tab Focus</p>
              <p class="mt-0.5 text-xs text-muted-foreground">
                Tracks when you switch away from the assessment tab and how long you're gone.
                Occasional switches are normal -- excessive switching flags the session.
              </p>
            </div>
          </div>

          <div class="flex gap-3 rounded-lg border border-border bg-card p-4">
            <div class="flex h-9 w-9 flex-shrink-0 items-center justify-center rounded-lg bg-blue-100 dark:bg-blue-900/30">
              <svg class="h-4 w-4 text-blue-600 dark:text-blue-400" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                <path stroke-linecap="round" stroke-linejoin="round" d="M15.666 3.888A2.25 2.25 0 0013.5 2.25h-3c-1.03 0-1.9.693-2.166 1.638m7.332 0c.055.194.084.4.084.612v0a.75.75 0 01-.75.75H9.75a.75.75 0 01-.75-.75v0c0-.212.03-.418.084-.612m7.332 0c.646.049 1.288.11 1.927.184 1.1.128 1.907 1.077 1.907 2.185V19.5a2.25 2.25 0 01-2.25 2.25H6.75A2.25 2.25 0 014.5 19.5V6.257c0-1.108.806-2.057 1.907-2.185a48.208 48.208 0 011.927-.184" />
              </svg>
            </div>
            <div>
              <p class="text-sm font-medium text-foreground">Clipboard</p>
              <p class="mt-0.5 text-xs text-muted-foreground">
                Detects paste events and counts characters pasted. Small pastes are fine --
                pasting large blocks of text into essay answers raises a flag.
              </p>
            </div>
          </div>

          <div class="flex gap-3 rounded-lg border border-border bg-card p-4">
            <div class="flex h-9 w-9 flex-shrink-0 items-center justify-center rounded-lg bg-purple-100 dark:bg-purple-900/30">
              <svg class="h-4 w-4 text-purple-600 dark:text-purple-400" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                <path stroke-linecap="round" stroke-linejoin="round" d="M17.25 6.75L22.5 12l-5.25 5.25m-10.5 0L1.5 12l5.25-5.25m7.5-3l-4.5 16.5" />
              </svg>
            </div>
            <div>
              <p class="text-sm font-medium text-foreground">Developer Tools</p>
              <p class="mt-0.5 text-xs text-muted-foreground">
                A heuristic detects if browser DevTools are open by checking window dimension changes.
                This is a soft signal -- it won't fail you, but it contributes to the overall integrity score.
              </p>
            </div>
          </div>

          <div class="flex gap-3 rounded-lg border border-border bg-card p-4">
            <div class="flex h-9 w-9 flex-shrink-0 items-center justify-center rounded-lg bg-rose-100 dark:bg-rose-900/30">
              <svg class="h-4 w-4 text-rose-600 dark:text-rose-400" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                <path stroke-linecap="round" stroke-linejoin="round" d="M7.864 4.243A7.5 7.5 0 0119.5 10.5c0 2.92-.556 5.709-1.568 8.268M5.742 6.364A7.465 7.465 0 004 10.5a7.464 7.464 0 01-1.15 3.993m1.989 3.559A11.209 11.209 0 008.25 10.5a3.75 3.75 0 117.5 0c0 .527-.021 1.049-.064 1.565M12 10.5a14.94 14.94 0 01-3.6 9.75m6.633-4.596a18.666 18.666 0 01-2.485 5.33" />
              </svg>
            </div>
            <div>
              <p class="text-sm font-medium text-foreground">Device Fingerprint</p>
              <p class="mt-0.5 text-xs text-muted-foreground">
                A SHA-256 hash of your browser's rendering characteristics creates a probabilistic device identifier.
                This detects multi-account usage -- the same device used by different accounts.
              </p>
            </div>
          </div>
        </div>

        <div class="mt-5 flex items-center justify-between">
          <AppButton variant="ghost" size="sm" @click="prevStep">Back</AppButton>
          <AppButton variant="primary" size="sm" @click="nextStep">Continue</AppButton>
        </div>
      </div>

      <!-- ================ CAMERA ================ -->
      <div v-else-if="currentStep === 'camera'">
        <div class="mb-4">
          <h2 class="text-base font-semibold text-foreground">Face Presence (Optional)</h2>
          <p class="mt-1 text-sm text-muted-foreground">
            Optionally enable your camera to confirm you're present during assessments. The video feed is processed entirely
            on your device -- no images or video are ever transmitted.
          </p>
        </div>

        <div class="mx-auto max-w-sm">
          <div v-if="!cameraEnabled && !cameraSkipped" class="text-center">
            <div class="mx-auto mb-4 flex h-48 w-48 items-center justify-center rounded-full border-4 border-dashed border-border bg-muted/30">
              <svg class="h-16 w-16 text-muted-foreground/40" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1">
                <path stroke-linecap="round" stroke-linejoin="round" d="M15.75 6a3.75 3.75 0 11-7.5 0 3.75 3.75 0 017.5 0zM4.501 20.118a7.5 7.5 0 0114.998 0A17.933 17.933 0 0112 21.75c-2.676 0-5.216-.584-7.499-1.632z" />
              </svg>
            </div>
            <div class="flex justify-center gap-3">
              <AppButton variant="primary" size="sm" @click="enableCamera">
                Enable Camera
              </AppButton>
              <AppButton variant="ghost" size="sm" @click="skipCamera">
                Skip
              </AppButton>
            </div>
            <div v-if="cameraError" class="mt-3 rounded-lg border border-amber-200 bg-amber-50 p-2 dark:border-amber-800/40 dark:bg-amber-900/20">
              <p class="text-xs text-amber-700 dark:text-amber-400">{{ cameraError }}</p>
            </div>
          </div>

          <div v-else-if="cameraEnabled" class="text-center">
            <div class="relative mx-auto mb-4 h-48 w-48 overflow-hidden rounded-full border-4 transition-colors" :class="faceDetected ? 'border-emerald-500' : 'border-amber-500'">
              <video
                ref="videoRef"
                class="h-full w-full object-cover"
                muted
                playsinline
              />
            </div>
            <div class="mb-4 flex items-center justify-center gap-2">
              <div class="h-2.5 w-2.5 rounded-full" :class="faceDetected ? 'bg-emerald-500' : 'bg-amber-500'" />
              <span class="text-sm" :class="faceDetected ? 'text-emerald-600 dark:text-emerald-400' : 'text-amber-600 dark:text-amber-400'">
                {{ faceDetected ? 'Face detected' : 'No face detected -- position yourself in frame' }}
              </span>
            </div>
            <AppButton variant="primary" size="sm" :disabled="!faceDetected" @click="nextStep">
              Continue
            </AppButton>
          </div>

          <div v-else class="text-center">
            <p class="mb-4 text-sm text-muted-foreground">
              Camera skipped. You can enable it later during any assessment.
            </p>
          </div>
        </div>

        <div class="mt-5 flex items-center justify-between">
          <AppButton variant="ghost" size="sm" @click="prevStep">Back</AppButton>
          <AppButton v-if="cameraSkipped" variant="primary" size="sm" @click="nextStep">Continue</AppButton>
        </div>
      </div>

      <!-- ================ REVIEW ================ -->
      <div v-else-if="currentStep === 'review'">
        <div class="mb-4 text-center">
          <div class="mx-auto mb-4 flex h-14 w-14 items-center justify-center rounded-2xl bg-emerald-100 dark:bg-emerald-900/30">
            <svg class="h-7 w-7 text-emerald-600 dark:text-emerald-400" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
              <path stroke-linecap="round" stroke-linejoin="round" d="M9 12.75L11.25 15 15 9.75M21 12a9 9 0 11-18 0 9 9 0 0118 0z" />
            </svg>
          </div>
          <h2 class="text-base font-semibold text-foreground">Calibration Complete</h2>
          <p class="mt-1 text-sm text-muted-foreground">
            Your behavioral profile has been built. Here's what Sentinel learned:
          </p>
        </div>

        <div v-if="savedProfile" class="mx-auto max-w-md space-y-3">
          <!-- Typing summary -->
          <div class="rounded-lg border border-border bg-muted/20 p-4">
            <p class="mb-2 text-xs font-medium text-muted-foreground">TYPING PATTERN</p>
            <div class="grid grid-cols-3 gap-3 text-center">
              <div>
                <p class="font-mono text-lg font-bold text-foreground">
                  {{ ((savedProfile as any)?.typingPattern?.avgDwellTime ?? 0).toFixed(0) }}
                </p>
                <p class="text-xs text-muted-foreground">ms dwell</p>
              </div>
              <div>
                <p class="font-mono text-lg font-bold text-foreground">
                  {{ ((savedProfile as any)?.typingPattern?.avgFlightMs ?? (savedProfile as any)?.typingPattern?.avgFlightTime ?? 0).toFixed(0) }}
                </p>
                <p class="text-xs text-muted-foreground">ms flight</p>
              </div>
              <div>
                <p class="font-mono text-lg font-bold text-foreground">
                  {{ ((savedProfile as any)?.typingPattern?.speedWpm ?? 0).toFixed(0) }}
                </p>
                <p class="text-xs text-muted-foreground">WPM</p>
              </div>
            </div>
          </div>

          <!-- Mouse summary -->
          <div class="rounded-lg border border-border bg-muted/20 p-4">
            <p class="mb-2 text-xs font-medium text-muted-foreground">MOUSE PATTERN</p>
            <div class="grid grid-cols-2 gap-3 text-center">
              <div>
                <p class="font-mono text-lg font-bold text-foreground">
                  {{ ((savedProfile as any)?.mousePattern?.avgVelocity ?? 0).toFixed(2) }}
                </p>
                <p class="text-xs text-muted-foreground">px/ms velocity</p>
              </div>
              <div>
                <p class="font-mono text-lg font-bold text-foreground">
                  {{ (savedProfile as any)?.mousePattern?.sampleCount ?? 0 }}
                </p>
                <p class="text-xs text-muted-foreground">samples</p>
              </div>
            </div>
          </div>

          <!-- Camera status -->
          <div class="rounded-lg border border-border bg-muted/20 p-4">
            <p class="mb-2 text-xs font-medium text-muted-foreground">FACE PRESENCE</p>
            <p class="text-sm text-foreground">
              {{ cameraSkipped ? 'Skipped -- can be enabled during assessments' : 'Configured and tested' }}
            </p>
          </div>

          <!-- AI Models training results -->
          <div v-if="aiTrainingResults" class="rounded-lg border border-primary/30 bg-primary/5 p-4">
            <p class="mb-3 text-xs font-medium text-primary">AI MODELS TRAINED</p>
            <div class="space-y-2">
              <div class="flex items-center justify-between text-sm">
                <span class="text-muted-foreground">Keystroke Autoencoder</span>
                <span
                  class="inline-flex items-center gap-1 rounded-full px-2 py-0.5 text-xs font-medium"
                  :class="aiTrainingResults.keystrokeAE.trained
                    ? 'bg-emerald-100 text-emerald-700 dark:bg-emerald-900/30 dark:text-emerald-400'
                    : 'bg-amber-100 text-amber-700 dark:bg-amber-900/30 dark:text-amber-400'"
                >
                  {{ aiTrainingResults.keystrokeAE.trained ? 'Trained' : 'Insufficient data' }}
                </span>
              </div>
              <div class="flex items-center justify-between text-sm">
                <span class="text-muted-foreground">Mouse Trajectory CNN</span>
                <span
                  class="inline-flex items-center gap-1 rounded-full px-2 py-0.5 text-xs font-medium"
                  :class="aiTrainingResults.mouseCNN.trained
                    ? 'bg-emerald-100 text-emerald-700 dark:bg-emerald-900/30 dark:text-emerald-400'
                    : 'bg-amber-100 text-amber-700 dark:bg-amber-900/30 dark:text-amber-400'"
                >
                  {{ aiTrainingResults.mouseCNN.trained ? 'Trained' : 'Insufficient data' }}
                </span>
              </div>
              <div class="flex items-center justify-between text-sm">
                <span class="text-muted-foreground">Face Verification</span>
                <span
                  class="inline-flex items-center gap-1 rounded-full px-2 py-0.5 text-xs font-medium"
                  :class="aiTrainingResults.faceEmbedder.enrolled
                    ? 'bg-emerald-100 text-emerald-700 dark:bg-emerald-900/30 dark:text-emerald-400'
                    : cameraSkipped
                      ? 'bg-gray-100 text-gray-600 dark:bg-gray-800 dark:text-gray-400'
                      : 'bg-amber-100 text-amber-700 dark:bg-amber-900/30 dark:text-amber-400'"
                >
                  {{ aiTrainingResults.faceEmbedder.enrolled ? 'Enrolled' : cameraSkipped ? 'Skipped' : 'Pending' }}
                </span>
              </div>
            </div>
            <p class="mt-3 text-xs text-muted-foreground">
              These models run alongside rule-based checks to verify it's you during assessments. All processing stays on your device.
            </p>
          </div>
        </div>

        <div class="mt-6 flex items-center justify-between">
          <AppButton variant="ghost" size="sm" @click="restartWizard">
            Recalibrate
          </AppButton>
          <AppButton
            variant="primary"
            size="sm"
            :disabled="saving"
            :loading="saving"
            @click="finishWizard"
          >
            {{ saving ? 'Saving...' : 'Finish' }}
          </AppButton>
        </div>
      </div>
    </div>
  </div>
</template>
