<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
import { useRouter } from 'vue-router'
import { useTutoringRoom } from '@/composables/useTutoringRoom'
import type { DeviceCheckResult, DeviceList } from '@/types'

const router = useRouter()
const {
  sessions,
  lastError,
  loading,
  refreshSessions,
  createRoom,
  joinRoom,
  listDevices,
} = useTutoringRoom()

const isMobilePlatform = typeof navigator !== 'undefined' && /iPhone|iPad|iPod|Android/i.test(navigator.userAgent)

const showCreateModal = ref(false)
const showJoinModal = ref(false)
const newRoomTitle = ref('')
const createDisplayName = ref('')
const joinTicket = ref('')
const joinTitle = ref('')
const joinDisplayName = ref('')

// Device check state (shared between create and join flows)
const deviceCheck = ref<DeviceCheckResult | null>(null)
const checkingDevices = ref(false)
// 'form' | 'preview' — which step the modal is on
const createStep = ref<'form' | 'preview'>('form')
const joinStep = ref<'form' | 'preview'>('form')

// Device selection state
const devices = ref<DeviceList | null>(null)
const selectedCamera = ref<string | null>(null)
const selectedMicInput = ref<string | null>(null)
const selectedAudioOutput = ref<string | null>(null)

onMounted(() => {
  refreshSessions()
})

const pastSessions = computed(() =>
  sessions.value.filter(s => s.status !== 'active')
)

const activeSession = computed(() =>
  sessions.value.find(s => s.status === 'active')
)

function resetCreateModal() {
  showCreateModal.value = false
  createStep.value = 'form'
  newRoomTitle.value = ''
  createDisplayName.value = ''
  deviceCheck.value = null
  devices.value = null
  selectedCamera.value = null
  selectedMicInput.value = null
  selectedAudioOutput.value = null
}

function resetJoinModal() {
  showJoinModal.value = false
  joinStep.value = 'form'
  joinTicket.value = ''
  joinTitle.value = ''
  joinDisplayName.value = ''
  deviceCheck.value = null
  devices.value = null
  selectedCamera.value = null
  selectedMicInput.value = null
  selectedAudioOutput.value = null
}

async function handleCreatePreview() {
  if (!newRoomTitle.value.trim()) return
  checkingDevices.value = true
  deviceCheck.value = null
  devices.value = null
  try {
    // Single call — derive device check from the device list to avoid
    // concurrent nokhwa_initialize calls that deadlock on macOS.
    const devList = await listDevices()
    devices.value = devList
    deviceCheck.value = {
      has_camera: devList.cameras.length > 0,
      camera_name: devList.cameras[0]?.name ?? null,
      has_audio: devList.audio_inputs.length > 0,
      error: null,
    }
    // Pre-select default devices
    selectedCamera.value = devList.cameras[0]?.index ?? null
    selectedMicInput.value = devList.audio_inputs.find(d => d.is_default)?.id ?? devList.audio_inputs[0]?.id ?? null
    selectedAudioOutput.value = devList.audio_outputs.find(d => d.is_default)?.id ?? devList.audio_outputs[0]?.id ?? null
    createStep.value = 'preview'
  } finally {
    checkingDevices.value = false
  }
}

async function handleCreateConfirm() {
  if (!newRoomTitle.value.trim()) return
  try {
    const session = await createRoom(
      newRoomTitle.value.trim(),
      createDisplayName.value.trim() || undefined,
      selectedCamera.value,
      selectedMicInput.value,
      selectedAudioOutput.value,
    )
    resetCreateModal()
    router.push(`/tutoring/${session.id}`)
  } catch {
    // error is in lastError
  }
}

async function handleJoinPreview() {
  if (!joinTicket.value.trim()) return
  checkingDevices.value = true
  deviceCheck.value = null
  devices.value = null
  try {
    const devList = await listDevices()
    devices.value = devList
    deviceCheck.value = {
      has_camera: devList.cameras.length > 0,
      camera_name: devList.cameras[0]?.name ?? null,
      has_audio: devList.audio_inputs.length > 0,
      error: null,
    }
    selectedCamera.value = devList.cameras[0]?.index ?? null
    selectedMicInput.value = devList.audio_inputs.find(d => d.is_default)?.id ?? devList.audio_inputs[0]?.id ?? null
    selectedAudioOutput.value = devList.audio_outputs.find(d => d.is_default)?.id ?? devList.audio_outputs[0]?.id ?? null
    joinStep.value = 'preview'
  } finally {
    checkingDevices.value = false
  }
}

async function handleJoinConfirm() {
  if (!joinTicket.value.trim()) return
  try {
    const session = await joinRoom(
      joinTicket.value.trim(),
      joinTitle.value.trim() || undefined,
      joinDisplayName.value.trim() || undefined,
      selectedCamera.value,
      selectedMicInput.value,
      selectedAudioOutput.value,
    )
    resetJoinModal()
    router.push(`/tutoring/${session.id}`)
  } catch {
    // error is in lastError
  }
}

function goToActiveSession() {
  if (activeSession.value) {
    router.push(`/tutoring/${activeSession.value.id}`)
  }
}

function formatDate(iso: string) {
  try {
    return new Date(iso).toLocaleDateString(undefined, {
      month: 'short',
      day: 'numeric',
      hour: '2-digit',
      minute: '2-digit',
    })
  } catch {
    return iso
  }
}
</script>

<template>
  <div class="space-y-6">
    <!-- Header -->
    <div class="flex items-center justify-between">
      <div class="flex items-center gap-3">
        <div class="flex h-10 w-10 items-center justify-center rounded-lg bg-primary/10">
          <svg class="h-5 w-5 text-primary" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.5">
            <path stroke-linecap="round" stroke-linejoin="round" d="M15 10l4.553-2.276A1 1 0 0121 8.618v6.764a1 1 0 01-1.447.894L15 14M5 18h8a2 2 0 002-2V8a2 2 0 00-2-2H5a2 2 0 00-2 2v8a2 2 0 002 2z" />
          </svg>
        </div>
        <div>
          <h1 class="text-2xl font-bold text-foreground">Live Tutoring</h1>
          <p class="text-sm text-muted-foreground">P2P video sessions powered by iroh — no servers, no limits.</p>
        </div>
      </div>
    </div>

    <!-- Error banner -->
    <div v-if="lastError" class="rounded-lg border border-destructive/30 bg-destructive/5 px-4 py-3 text-sm text-destructive">
      {{ lastError }}
    </div>

    <!-- Mobile notice -->
    <div v-if="isMobilePlatform" class="rounded-xl border border-amber-500/30 bg-amber-500/5 p-5">
      <div class="flex items-start gap-3">
        <div class="flex h-10 w-10 items-center justify-center rounded-lg bg-amber-500/10 shrink-0">
          <svg class="h-5 w-5 text-amber-500" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.5">
            <path stroke-linecap="round" stroke-linejoin="round" d="M10.5 1.5H8.25A2.25 2.25 0 006 3.75v16.5a2.25 2.25 0 002.25 2.25h7.5A2.25 2.25 0 0018 20.25V3.75a2.25 2.25 0 00-2.25-2.25H13.5m-3 0V3h3V1.5m-3 0h3m-3 18.75h3" />
          </svg>
        </div>
        <div>
          <h3 class="text-sm font-semibold text-foreground">Desktop Only (for now)</h3>
          <p class="mt-1 text-sm text-muted-foreground">
            Live tutoring requires camera and audio encoding that isn't available on mobile yet.
            Start or join sessions from the desktop app. Mobile audio-only sessions are coming in Phase 2.
          </p>
        </div>
      </div>
    </div>

    <!-- Active session banner -->
    <div
      v-if="activeSession"
      class="relative overflow-hidden rounded-xl border border-primary/30 bg-primary/5 p-5 cursor-pointer transition-colors hover:bg-primary/10"
      @click="goToActiveSession"
    >
      <div class="flex items-center gap-3">
        <span class="relative flex h-3 w-3">
          <span class="absolute inline-flex h-full w-full animate-ping rounded-full bg-primary opacity-75" />
          <span class="relative inline-flex h-3 w-3 rounded-full bg-primary" />
        </span>
        <div class="flex-1 min-w-0">
          <p class="font-semibold text-foreground">{{ activeSession.title }}</p>
          <p class="text-xs text-muted-foreground">Session in progress — click to rejoin</p>
        </div>
        <svg class="h-5 w-5 text-primary" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
          <path stroke-linecap="round" stroke-linejoin="round" d="M9 5l7 7-7 7" />
        </svg>
      </div>
    </div>

    <!-- Action cards -->
    <div class="grid grid-cols-1 gap-4 sm:grid-cols-2">
      <!-- Create Room -->
      <button
        class="group rounded-xl border border-border bg-card p-6 text-left transition-all hover:border-primary/40 hover:shadow-md disabled:opacity-50 disabled:cursor-not-allowed disabled:hover:border-border disabled:hover:shadow-none"
        :disabled="isMobilePlatform"
        @click="showCreateModal = true; createStep = 'form'"
      >
        <div class="flex h-12 w-12 items-center justify-center rounded-lg bg-primary/10 transition-colors group-hover:bg-primary/20">
          <svg class="h-6 w-6 text-primary" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.5">
            <path stroke-linecap="round" stroke-linejoin="round" d="M12 4.5v15m7.5-7.5h-15" />
          </svg>
        </div>
        <h3 class="mt-4 text-lg font-semibold text-foreground">Start a Session</h3>
        <p class="mt-1 text-sm text-muted-foreground">Create a new tutoring room with camera and microphone. Share the invite ticket with participants.</p>
      </button>

      <!-- Join Room -->
      <button
        class="group rounded-xl border border-border bg-card p-6 text-left transition-all hover:border-primary/40 hover:shadow-md disabled:opacity-50 disabled:cursor-not-allowed disabled:hover:border-border disabled:hover:shadow-none"
        :disabled="isMobilePlatform"
        @click="showJoinModal = true; joinStep = 'form'"
      >
        <div class="flex h-12 w-12 items-center justify-center rounded-lg bg-accent/10 transition-colors group-hover:bg-accent/20">
          <svg class="h-6 w-6 text-accent-foreground" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.5">
            <path stroke-linecap="round" stroke-linejoin="round" d="M15.75 9V5.25A2.25 2.25 0 0013.5 3h-6a2.25 2.25 0 00-2.25 2.25v13.5A2.25 2.25 0 007.5 21h6a2.25 2.25 0 002.25-2.25V15m3 0l3-3m0 0l-3-3m3 3H9" />
          </svg>
        </div>
        <h3 class="mt-4 text-lg font-semibold text-foreground">Join a Session</h3>
        <p class="mt-1 text-sm text-muted-foreground">Enter a room ticket to join an existing tutoring session. Your camera and mic will activate on join.</p>
      </button>
    </div>

    <!-- Past sessions -->
    <div v-if="pastSessions.length > 0">
      <h2 class="text-lg font-semibold text-foreground mb-3">Past Sessions</h2>
      <div class="divide-y divide-border rounded-xl border border-border bg-card overflow-hidden">
        <div
          v-for="session in pastSessions"
          :key="session.id"
          class="flex items-center gap-3 px-4 py-3"
        >
          <div class="flex h-8 w-8 items-center justify-center rounded-lg bg-muted">
            <svg class="h-4 w-4 text-muted-foreground" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
              <path stroke-linecap="round" stroke-linejoin="round" d="M15 10l4.553-2.276A1 1 0 0121 8.618v6.764a1 1 0 01-1.447.894L15 14M5 18h8a2 2 0 002-2V8a2 2 0 00-2-2H5a2 2 0 00-2 2v8a2 2 0 002 2z" />
            </svg>
          </div>
          <div class="flex-1 min-w-0">
            <p class="text-sm font-medium text-foreground truncate">{{ session.title }}</p>
            <p class="text-xs text-muted-foreground">{{ formatDate(session.created_at) }}</p>
          </div>
          <span
            class="inline-flex items-center rounded-full px-2 py-0.5 text-[0.65rem] font-medium"
            :class="session.status === 'ended' ? 'bg-muted text-muted-foreground' : 'bg-destructive/10 text-destructive'"
          >
            {{ session.status }}
          </span>
        </div>
      </div>
    </div>

    <!-- Empty state -->
    <div v-else-if="!loading" class="flex flex-col items-center justify-center py-16 text-center">
      <div class="flex h-16 w-16 items-center justify-center rounded-full bg-muted/30 mb-4">
        <svg class="h-8 w-8 text-muted-foreground/50" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.5">
          <path stroke-linecap="round" stroke-linejoin="round" d="M15 10l4.553-2.276A1 1 0 0121 8.618v6.764a1 1 0 01-1.447.894L15 14M5 18h8a2 2 0 002-2V8a2 2 0 00-2-2H5a2 2 0 00-2 2v8a2 2 0 002 2z" />
        </svg>
      </div>
      <h3 class="text-sm font-medium text-foreground">No sessions yet</h3>
      <p class="mt-1 text-xs text-muted-foreground max-w-xs">Start or join a live tutoring session. All video and audio streams peer-to-peer via iroh.</p>
    </div>

    <!-- Create Room Modal -->
    <Teleport to="body">
      <Transition
        enter-active-class="transition-all duration-200"
        enter-from-class="opacity-0"
        enter-to-class="opacity-100"
        leave-active-class="transition-all duration-150"
        leave-from-class="opacity-100"
        leave-to-class="opacity-0"
      >
        <div v-if="showCreateModal" class="fixed inset-0 z-50 flex items-center justify-center bg-black/50 backdrop-blur-sm" @click.self="resetCreateModal">
          <div class="w-full max-w-md rounded-xl border border-border bg-card p-6 shadow-xl mx-4">
            <!-- Step 1: Form -->
            <template v-if="createStep === 'form'">
              <h2 class="text-lg font-semibold text-foreground">Start a Tutoring Session</h2>
              <p class="mt-1 text-sm text-muted-foreground">Give your session a name. Participants will join using the ticket you share.</p>
              <div class="mt-4 space-y-3">
                <div>
                  <label class="text-sm font-medium text-foreground" for="room-title">Session Title</label>
                  <input
                    id="room-title"
                    v-model="newRoomTitle"
                    type="text"
                    placeholder="e.g. Graph Algorithms Review"
                    class="mt-1 w-full rounded-lg border border-border bg-background px-3 py-2 text-sm text-foreground placeholder-muted-foreground focus:border-primary focus:outline-none focus:ring-1 focus:ring-primary"
                    @keydown.enter="handleCreatePreview"
                  />
                </div>
                <div>
                  <label class="text-sm font-medium text-foreground" for="create-display-name">Your Name (optional)</label>
                  <input
                    id="create-display-name"
                    v-model="createDisplayName"
                    type="text"
                    placeholder="e.g. Alice"
                    class="mt-1 w-full rounded-lg border border-border bg-background px-3 py-2 text-sm text-foreground placeholder-muted-foreground focus:border-primary focus:outline-none focus:ring-1 focus:ring-primary"
                  />
                </div>
              </div>
              <div class="mt-6 flex justify-end gap-2">
                <button
                  class="rounded-lg border border-border px-4 py-2 text-sm font-medium text-foreground transition-colors hover:bg-muted"
                  @click="resetCreateModal"
                >
                  Cancel
                </button>
                <button
                  class="rounded-lg bg-primary px-4 py-2 text-sm font-medium text-primary-foreground transition-colors hover:bg-primary/90 disabled:opacity-50"
                  :disabled="!newRoomTitle.trim() || checkingDevices"
                  @click="handleCreatePreview"
                >
                  {{ checkingDevices ? 'Checking...' : 'Next' }}
                </button>
              </div>
            </template>

            <!-- Step 2: Device preview -->
            <template v-else>
              <h2 class="text-lg font-semibold text-foreground">Device Check</h2>
              <p class="mt-1 text-sm text-muted-foreground">Verify your devices before starting.</p>

              <div class="mt-4 space-y-3">
                <!-- Camera selector -->
                <div class="rounded-lg border border-border p-3 space-y-2">
                  <div class="flex items-center gap-3">
                    <div
                      class="flex h-9 w-9 items-center justify-center rounded-lg shrink-0"
                      :class="deviceCheck?.has_camera ? 'bg-success/10' : 'bg-destructive/10'"
                    >
                      <svg class="h-4.5 w-4.5" :class="deviceCheck?.has_camera ? 'text-success' : 'text-destructive'" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                        <path stroke-linecap="round" stroke-linejoin="round" d="M15 10l4.553-2.276A1 1 0 0121 8.618v6.764a1 1 0 01-1.447.894L15 14M5 18h8a2 2 0 002-2V8a2 2 0 00-2-2H5a2 2 0 00-2 2v8a2 2 0 002 2z" />
                      </svg>
                    </div>
                    <div class="flex-1 min-w-0">
                      <p class="text-sm font-medium text-foreground">Camera</p>
                    </div>
                    <svg v-if="deviceCheck?.has_camera" class="h-4 w-4 text-success shrink-0" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                      <path stroke-linecap="round" stroke-linejoin="round" d="M4.5 12.75l6 6 9-13.5" />
                    </svg>
                    <svg v-else class="h-4 w-4 text-warning shrink-0" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                      <path stroke-linecap="round" stroke-linejoin="round" d="M12 9v3.75m-9.303 3.376c-.866 1.5.217 3.374 1.948 3.374h14.71c1.73 0 2.813-1.874 1.948-3.374L13.949 3.378c-.866-1.5-3.032-1.5-3.898 0L2.697 16.126zM12 15.75h.007v.008H12v-.008z" />
                    </svg>
                  </div>
                  <select
                    v-if="devices && devices.cameras.length > 0"
                    v-model="selectedCamera"
                    class="w-full rounded-md border border-border bg-background px-2.5 py-1.5 text-xs text-foreground focus:border-primary focus:outline-none focus:ring-1 focus:ring-primary"
                  >
                    <option v-for="cam in devices.cameras" :key="cam.index" :value="cam.index">
                      {{ cam.name }}
                    </option>
                  </select>
                  <p v-else class="text-xs text-muted-foreground">No cameras detected — session will be audio-only</p>
                </div>

                <!-- Microphone selector -->
                <div class="rounded-lg border border-border p-3 space-y-2">
                  <div class="flex items-center gap-3">
                    <div
                      class="flex h-9 w-9 items-center justify-center rounded-lg shrink-0"
                      :class="deviceCheck?.has_audio ? 'bg-success/10' : 'bg-destructive/10'"
                    >
                      <svg class="h-4.5 w-4.5" :class="deviceCheck?.has_audio ? 'text-success' : 'text-destructive'" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                        <path stroke-linecap="round" stroke-linejoin="round" d="M12 18.75a6 6 0 006-6v-1.5m-6 7.5a6 6 0 01-6-6v-1.5m6 7.5v3.75m-3.75 0h7.5M12 15.75a3 3 0 01-3-3V4.5a3 3 0 116 0v8.25a3 3 0 01-3 3z" />
                      </svg>
                    </div>
                    <div class="flex-1 min-w-0">
                      <p class="text-sm font-medium text-foreground">Microphone</p>
                    </div>
                    <svg v-if="deviceCheck?.has_audio" class="h-4 w-4 text-success shrink-0" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                      <path stroke-linecap="round" stroke-linejoin="round" d="M4.5 12.75l6 6 9-13.5" />
                    </svg>
                    <svg v-else class="h-4 w-4 text-warning shrink-0" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                      <path stroke-linecap="round" stroke-linejoin="round" d="M12 9v3.75m-9.303 3.376c-.866 1.5.217 3.374 1.948 3.374h14.71c1.73 0 2.813-1.874 1.948-3.374L13.949 3.378c-.866-1.5-3.032-1.5-3.898 0L2.697 16.126zM12 15.75h.007v.008H12v-.008z" />
                    </svg>
                  </div>
                  <select
                    v-if="devices && devices.audio_inputs.length > 0"
                    v-model="selectedMicInput"
                    class="w-full rounded-md border border-border bg-background px-2.5 py-1.5 text-xs text-foreground focus:border-primary focus:outline-none focus:ring-1 focus:ring-primary"
                  >
                    <option v-for="mic in devices.audio_inputs" :key="mic.id" :value="mic.id">
                      {{ mic.name || mic.id }}{{ mic.is_default ? ' (Default)' : '' }}
                    </option>
                  </select>
                  <p v-else class="text-xs text-muted-foreground">No microphones detected</p>
                </div>

                <!-- Speaker output selector -->
                <div class="rounded-lg border border-border p-3 space-y-2">
                  <div class="flex items-center gap-3">
                    <div class="flex h-9 w-9 items-center justify-center rounded-lg bg-muted shrink-0">
                      <svg class="h-4.5 w-4.5 text-foreground" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                        <path stroke-linecap="round" stroke-linejoin="round" d="M19.114 5.636a9 9 0 010 12.728M16.463 8.288a5.25 5.25 0 010 7.424M6.75 8.25l4.72-4.72a.75.75 0 011.28.53v15.88a.75.75 0 01-1.28.53l-4.72-4.72H4.51c-.88 0-1.704-.507-1.938-1.354A9.01 9.01 0 012.25 12c0-.83.112-1.633.322-2.396C2.806 8.756 3.63 8.25 4.51 8.25H6.75z" />
                      </svg>
                    </div>
                    <div class="flex-1 min-w-0">
                      <p class="text-sm font-medium text-foreground">Speaker</p>
                    </div>
                    <svg v-if="devices && devices.audio_outputs.length > 0" class="h-4 w-4 text-success shrink-0" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                      <path stroke-linecap="round" stroke-linejoin="round" d="M4.5 12.75l6 6 9-13.5" />
                    </svg>
                  </div>
                  <select
                    v-if="devices && devices.audio_outputs.length > 0"
                    v-model="selectedAudioOutput"
                    class="w-full rounded-md border border-border bg-background px-2.5 py-1.5 text-xs text-foreground focus:border-primary focus:outline-none focus:ring-1 focus:ring-primary"
                  >
                    <option v-for="spk in devices.audio_outputs" :key="spk.id" :value="spk.id">
                      {{ spk.name || spk.id }}{{ spk.is_default ? ' (Default)' : '' }}
                    </option>
                  </select>
                  <p v-else class="text-xs text-muted-foreground">No speakers detected</p>
                </div>

                <!-- Info text -->
                <p v-if="!deviceCheck?.has_camera && !deviceCheck?.has_audio" class="text-xs text-muted-foreground text-center mt-1">
                  You can still join — the session will use text chat only.
                </p>
              </div>

              <div class="mt-6 flex justify-end gap-2">
                <button
                  class="rounded-lg border border-border px-4 py-2 text-sm font-medium text-foreground transition-colors hover:bg-muted"
                  @click="createStep = 'form'"
                >
                  Back
                </button>
                <button
                  class="rounded-lg bg-primary px-4 py-2 text-sm font-medium text-primary-foreground transition-colors hover:bg-primary/90 disabled:opacity-50"
                  :disabled="loading"
                  @click="handleCreateConfirm"
                >
                  {{ loading ? 'Starting...' : 'Start Session' }}
                </button>
              </div>
            </template>
          </div>
        </div>
      </Transition>
    </Teleport>

    <!-- Join Room Modal -->
    <Teleport to="body">
      <Transition
        enter-active-class="transition-all duration-200"
        enter-from-class="opacity-0"
        enter-to-class="opacity-100"
        leave-active-class="transition-all duration-150"
        leave-from-class="opacity-100"
        leave-to-class="opacity-0"
      >
        <div v-if="showJoinModal" class="fixed inset-0 z-50 flex items-center justify-center bg-black/50 backdrop-blur-sm" @click.self="resetJoinModal">
          <div class="w-full max-w-md rounded-xl border border-border bg-card p-6 shadow-xl mx-4">
            <!-- Step 1: Form -->
            <template v-if="joinStep === 'form'">
              <h2 class="text-lg font-semibold text-foreground">Join a Tutoring Session</h2>
              <p class="mt-1 text-sm text-muted-foreground">Paste the room ticket shared by the host.</p>
              <div class="mt-4 space-y-3">
                <div>
                  <label class="text-sm font-medium text-foreground" for="join-ticket">Room Ticket</label>
                  <textarea
                    id="join-ticket"
                    v-model="joinTicket"
                    rows="3"
                    placeholder="Paste room ticket here..."
                    class="mt-1 w-full rounded-lg border border-border bg-background px-3 py-2 text-sm text-foreground font-mono placeholder-muted-foreground focus:border-primary focus:outline-none focus:ring-1 focus:ring-primary resize-none"
                  />
                </div>
                <div>
                  <label class="text-sm font-medium text-foreground" for="join-title">Session Label (optional)</label>
                  <input
                    id="join-title"
                    v-model="joinTitle"
                    type="text"
                    placeholder="e.g. My Study Session"
                    class="mt-1 w-full rounded-lg border border-border bg-background px-3 py-2 text-sm text-foreground placeholder-muted-foreground focus:border-primary focus:outline-none focus:ring-1 focus:ring-primary"
                  />
                </div>
                <div>
                  <label class="text-sm font-medium text-foreground" for="join-display-name">Your Name (optional)</label>
                  <input
                    id="join-display-name"
                    v-model="joinDisplayName"
                    type="text"
                    placeholder="e.g. Bob"
                    class="mt-1 w-full rounded-lg border border-border bg-background px-3 py-2 text-sm text-foreground placeholder-muted-foreground focus:border-primary focus:outline-none focus:ring-1 focus:ring-primary"
                  />
                </div>
              </div>
              <div class="mt-6 flex justify-end gap-2">
                <button
                  class="rounded-lg border border-border px-4 py-2 text-sm font-medium text-foreground transition-colors hover:bg-muted"
                  @click="resetJoinModal"
                >
                  Cancel
                </button>
                <button
                  class="rounded-lg bg-primary px-4 py-2 text-sm font-medium text-primary-foreground transition-colors hover:bg-primary/90 disabled:opacity-50"
                  :disabled="!joinTicket.trim() || checkingDevices"
                  @click="handleJoinPreview"
                >
                  {{ checkingDevices ? 'Checking...' : 'Next' }}
                </button>
              </div>
            </template>

            <!-- Step 2: Device preview -->
            <template v-else>
              <h2 class="text-lg font-semibold text-foreground">Device Check</h2>
              <p class="mt-1 text-sm text-muted-foreground">Verify your devices before joining.</p>

              <div class="mt-4 space-y-3">
                <!-- Camera selector -->
                <div class="rounded-lg border border-border p-3 space-y-2">
                  <div class="flex items-center gap-3">
                    <div
                      class="flex h-9 w-9 items-center justify-center rounded-lg shrink-0"
                      :class="deviceCheck?.has_camera ? 'bg-success/10' : 'bg-destructive/10'"
                    >
                      <svg class="h-4.5 w-4.5" :class="deviceCheck?.has_camera ? 'text-success' : 'text-destructive'" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                        <path stroke-linecap="round" stroke-linejoin="round" d="M15 10l4.553-2.276A1 1 0 0121 8.618v6.764a1 1 0 01-1.447.894L15 14M5 18h8a2 2 0 002-2V8a2 2 0 00-2-2H5a2 2 0 00-2 2v8a2 2 0 002 2z" />
                      </svg>
                    </div>
                    <div class="flex-1 min-w-0">
                      <p class="text-sm font-medium text-foreground">Camera</p>
                    </div>
                    <svg v-if="deviceCheck?.has_camera" class="h-4 w-4 text-success shrink-0" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                      <path stroke-linecap="round" stroke-linejoin="round" d="M4.5 12.75l6 6 9-13.5" />
                    </svg>
                    <svg v-else class="h-4 w-4 text-warning shrink-0" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                      <path stroke-linecap="round" stroke-linejoin="round" d="M12 9v3.75m-9.303 3.376c-.866 1.5.217 3.374 1.948 3.374h14.71c1.73 0 2.813-1.874 1.948-3.374L13.949 3.378c-.866-1.5-3.032-1.5-3.898 0L2.697 16.126zM12 15.75h.007v.008H12v-.008z" />
                    </svg>
                  </div>
                  <select
                    v-if="devices && devices.cameras.length > 0"
                    v-model="selectedCamera"
                    class="w-full rounded-md border border-border bg-background px-2.5 py-1.5 text-xs text-foreground focus:border-primary focus:outline-none focus:ring-1 focus:ring-primary"
                  >
                    <option v-for="cam in devices.cameras" :key="cam.index" :value="cam.index">
                      {{ cam.name }}
                    </option>
                  </select>
                  <p v-else class="text-xs text-muted-foreground">No cameras detected — session will be audio-only</p>
                </div>

                <!-- Microphone selector -->
                <div class="rounded-lg border border-border p-3 space-y-2">
                  <div class="flex items-center gap-3">
                    <div
                      class="flex h-9 w-9 items-center justify-center rounded-lg shrink-0"
                      :class="deviceCheck?.has_audio ? 'bg-success/10' : 'bg-destructive/10'"
                    >
                      <svg class="h-4.5 w-4.5" :class="deviceCheck?.has_audio ? 'text-success' : 'text-destructive'" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                        <path stroke-linecap="round" stroke-linejoin="round" d="M12 18.75a6 6 0 006-6v-1.5m-6 7.5a6 6 0 01-6-6v-1.5m6 7.5v3.75m-3.75 0h7.5M12 15.75a3 3 0 01-3-3V4.5a3 3 0 116 0v8.25a3 3 0 01-3 3z" />
                      </svg>
                    </div>
                    <div class="flex-1 min-w-0">
                      <p class="text-sm font-medium text-foreground">Microphone</p>
                    </div>
                    <svg v-if="deviceCheck?.has_audio" class="h-4 w-4 text-success shrink-0" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                      <path stroke-linecap="round" stroke-linejoin="round" d="M4.5 12.75l6 6 9-13.5" />
                    </svg>
                    <svg v-else class="h-4 w-4 text-warning shrink-0" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                      <path stroke-linecap="round" stroke-linejoin="round" d="M12 9v3.75m-9.303 3.376c-.866 1.5.217 3.374 1.948 3.374h14.71c1.73 0 2.813-1.874 1.948-3.374L13.949 3.378c-.866-1.5-3.032-1.5-3.898 0L2.697 16.126zM12 15.75h.007v.008H12v-.008z" />
                    </svg>
                  </div>
                  <select
                    v-if="devices && devices.audio_inputs.length > 0"
                    v-model="selectedMicInput"
                    class="w-full rounded-md border border-border bg-background px-2.5 py-1.5 text-xs text-foreground focus:border-primary focus:outline-none focus:ring-1 focus:ring-primary"
                  >
                    <option v-for="mic in devices.audio_inputs" :key="mic.id" :value="mic.id">
                      {{ mic.name || mic.id }}{{ mic.is_default ? ' (Default)' : '' }}
                    </option>
                  </select>
                  <p v-else class="text-xs text-muted-foreground">No microphones detected</p>
                </div>

                <!-- Speaker output selector -->
                <div class="rounded-lg border border-border p-3 space-y-2">
                  <div class="flex items-center gap-3">
                    <div class="flex h-9 w-9 items-center justify-center rounded-lg bg-muted shrink-0">
                      <svg class="h-4.5 w-4.5 text-foreground" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                        <path stroke-linecap="round" stroke-linejoin="round" d="M19.114 5.636a9 9 0 010 12.728M16.463 8.288a5.25 5.25 0 010 7.424M6.75 8.25l4.72-4.72a.75.75 0 011.28.53v15.88a.75.75 0 01-1.28.53l-4.72-4.72H4.51c-.88 0-1.704-.507-1.938-1.354A9.01 9.01 0 012.25 12c0-.83.112-1.633.322-2.396C2.806 8.756 3.63 8.25 4.51 8.25H6.75z" />
                      </svg>
                    </div>
                    <div class="flex-1 min-w-0">
                      <p class="text-sm font-medium text-foreground">Speaker</p>
                    </div>
                    <svg v-if="devices && devices.audio_outputs.length > 0" class="h-4 w-4 text-success shrink-0" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                      <path stroke-linecap="round" stroke-linejoin="round" d="M4.5 12.75l6 6 9-13.5" />
                    </svg>
                  </div>
                  <select
                    v-if="devices && devices.audio_outputs.length > 0"
                    v-model="selectedAudioOutput"
                    class="w-full rounded-md border border-border bg-background px-2.5 py-1.5 text-xs text-foreground focus:border-primary focus:outline-none focus:ring-1 focus:ring-primary"
                  >
                    <option v-for="spk in devices.audio_outputs" :key="spk.id" :value="spk.id">
                      {{ spk.name || spk.id }}{{ spk.is_default ? ' (Default)' : '' }}
                    </option>
                  </select>
                  <p v-else class="text-xs text-muted-foreground">No speakers detected</p>
                </div>

                <!-- Info text -->
                <p v-if="!deviceCheck?.has_camera && !deviceCheck?.has_audio" class="text-xs text-muted-foreground text-center mt-1">
                  You can still join — the session will use text chat only.
                </p>
              </div>

              <div class="mt-6 flex justify-end gap-2">
                <button
                  class="rounded-lg border border-border px-4 py-2 text-sm font-medium text-foreground transition-colors hover:bg-muted"
                  @click="joinStep = 'form'"
                >
                  Back
                </button>
                <button
                  class="rounded-lg bg-primary px-4 py-2 text-sm font-medium text-primary-foreground transition-colors hover:bg-primary/90 disabled:opacity-50"
                  :disabled="loading"
                  @click="handleJoinConfirm"
                >
                  {{ loading ? 'Joining...' : 'Join Session' }}
                </button>
              </div>
            </template>
          </div>
        </div>
      </Transition>
    </Teleport>
  </div>
</template>
