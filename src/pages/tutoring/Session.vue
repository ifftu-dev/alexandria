<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted, watch, nextTick } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useTutoringRoom } from '@/composables/useTutoringRoom'

const route = useRoute()
const router = useRouter()
const {
  sessionStatus,
  lastError,
  videoFrames,
  chatMessages,
  refreshStatus,
  leaveRoom,
  toggleVideo,
  toggleAudio,
  toggleScreenShare,
  sendChat,
  startPolling,
  stopPolling,
  setupEventListeners,
} = useTutoringRoom()

const sessionId = computed(() => route.params.id as string)
const ticketCopied = ref(false)
const showLeaveConfirm = ref(false)
const showChat = ref(false)
const chatInput = ref('')
const chatScrollRef = ref<HTMLElement | null>(null)

// Duration timer
const elapsedSeconds = ref(0)
let durationInterval: ReturnType<typeof setInterval> | null = null

onMounted(async () => {
  await setupEventListeners()
  refreshStatus()
  startPolling(2000)
  // Update elapsed time every second
  durationInterval = setInterval(() => {
    if (sessionStatus.value?.started_at) {
      elapsedSeconds.value = Math.floor((Date.now() - sessionStatus.value.started_at) / 1000)
    }
  }, 1000)
})

onUnmounted(() => {
  stopPolling()
  if (durationInterval) {
    clearInterval(durationInterval)
    durationInterval = null
  }
})

const isActive = computed(() => sessionStatus.value?.session_id === sessionId.value)
const peers = computed(() => sessionStatus.value?.peers ?? [])
const peerCount = computed(() => peers.value.length)
const connectedPeerCount = computed(() => peers.value.filter(p => p.connected).length)
const videoEnabled = computed(() => sessionStatus.value?.video_enabled ?? false)
const audioEnabled = computed(() => sessionStatus.value?.audio_enabled ?? false)
const screenSharing = computed(() => sessionStatus.value?.screen_sharing ?? false)
const selfVideoSrc = computed(() => videoFrames.value['self'] ?? null)

const formattedDuration = computed(() => {
  const s = elapsedSeconds.value
  const hours = Math.floor(s / 3600)
  const mins = Math.floor((s % 3600) / 60)
  const secs = s % 60
  if (hours > 0) {
    return `${hours}:${String(mins).padStart(2, '0')}:${String(secs).padStart(2, '0')}`
  }
  return `${mins}:${String(secs).padStart(2, '0')}`
})

const unreadChat = computed(() => {
  if (showChat.value) return 0
  return chatMessages.value.length
})

// Auto-scroll chat
watch(chatMessages, () => {
  nextTick(() => {
    if (chatScrollRef.value) {
      chatScrollRef.value.scrollTop = chatScrollRef.value.scrollHeight
    }
  })
})

async function copyTicket() {
  if (!sessionStatus.value?.ticket) return
  try {
    await navigator.clipboard.writeText(sessionStatus.value.ticket)
    ticketCopied.value = true
    setTimeout(() => { ticketCopied.value = false }, 2000)
  } catch {
    // fallback
  }
}

async function handleLeave() {
  try {
    await leaveRoom()
    router.push('/tutoring')
  } catch {
    // error handled in composable
  }
}

async function handleToggleVideo() {
  try {
    await toggleVideo(!videoEnabled.value)
  } catch {
    // error shown via lastError
  }
}

async function handleToggleAudio() {
  try {
    await toggleAudio(!audioEnabled.value)
  } catch {
    // error shown via lastError
  }
}

async function handleToggleScreenShare() {
  try {
    await toggleScreenShare(!screenSharing.value)
  } catch {
    // error shown via lastError
  }
}

async function handleSendChat() {
  const text = chatInput.value.trim()
  if (!text) return
  chatInput.value = ''
  try {
    await sendChat(text)
  } catch {
    // error shown via lastError
  }
}

function formatChatTime(ts: number) {
  const d = new Date(ts)
  return d.toLocaleTimeString(undefined, { hour: '2-digit', minute: '2-digit' })
}

function shortNodeId(id: string) {
  return id === 'self' ? 'You' : id.slice(0, 8) + '...'
}
</script>

<template>
  <div class="flex h-[calc(100vh-4rem)] flex-col">
    <!-- Top bar -->
    <div class="flex items-center justify-between border-b border-border px-4 py-3 shrink-0">
      <div class="flex items-center gap-3 min-w-0">
        <button
          class="flex items-center gap-1 rounded-lg px-2 py-1.5 text-sm text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
          @click="router.push('/tutoring')"
        >
          <svg class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
            <path stroke-linecap="round" stroke-linejoin="round" d="M15 19l-7-7 7-7" />
          </svg>
          Back
        </button>

        <div class="h-4 w-px bg-border" />

        <div class="flex items-center gap-2 min-w-0">
          <span class="relative flex h-2.5 w-2.5 shrink-0" v-if="isActive">
            <span class="absolute inline-flex h-full w-full animate-ping rounded-full bg-success opacity-75" />
            <span class="relative inline-flex h-2.5 w-2.5 rounded-full bg-success" />
          </span>
          <span v-else class="h-2.5 w-2.5 rounded-full bg-muted-foreground/30 shrink-0" />
          <span class="text-sm font-medium text-foreground truncate">
            {{ isActive ? 'Session Active' : 'Session Ended' }}
          </span>
          <!-- Duration timer -->
          <span v-if="isActive" class="rounded bg-muted px-1.5 py-0.5 text-xs font-mono text-muted-foreground tabular-nums">
            {{ formattedDuration }}
          </span>
        </div>
      </div>

      <div class="flex items-center gap-2 shrink-0">
        <!-- Peer count -->
        <div class="flex items-center gap-1.5 rounded-lg bg-muted px-2.5 py-1.5 text-xs font-medium text-muted-foreground">
          <svg class="h-3.5 w-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
            <path stroke-linecap="round" stroke-linejoin="round" d="M15 19.128a9.38 9.38 0 002.625.372 9.337 9.337 0 004.121-.952 4.125 4.125 0 00-7.533-2.493M15 19.128v-.003c0-1.113-.285-2.16-.786-3.07M15 19.128H9m6 0a5.97 5.97 0 00-.786-3.07M9 19.128v-.003c0-1.113.285-2.16.786-3.07M9 19.128H3.375a4.125 4.125 0 01-.003-8.25 4.125 4.125 0 017.533-2.493M9 19.128a5.97 5.97 0 01.786-3.07" />
          </svg>
          {{ connectedPeerCount }}/{{ peerCount }} peers
        </div>

        <!-- Copy ticket -->
        <button
          v-if="isActive && sessionStatus?.ticket"
          class="flex items-center gap-1.5 rounded-lg border border-border px-2.5 py-1.5 text-xs font-medium text-foreground transition-colors hover:bg-muted"
          @click="copyTicket"
        >
          <svg v-if="!ticketCopied" class="h-3.5 w-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
            <path stroke-linecap="round" stroke-linejoin="round" d="M15.666 3.888A2.25 2.25 0 0013.5 2.25h-3c-1.03 0-1.9.693-2.166 1.638m7.332 0c.055.194.084.4.084.612v0a.75.75 0 01-.75.75H9.75a.75.75 0 01-.75-.75v0c0-.212.03-.418.084-.612m7.332 0c.646.049 1.288.11 1.927.184 1.1.128 1.907 1.077 1.907 2.185V19.5a2.25 2.25 0 01-2.25 2.25H6.75A2.25 2.25 0 014.5 19.5V6.257c0-1.108.806-2.057 1.907-2.185a48.208 48.208 0 011.927-.184" />
          </svg>
          <svg v-else class="h-3.5 w-3.5 text-success" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
            <path stroke-linecap="round" stroke-linejoin="round" d="M4.5 12.75l6 6 9-13.5" />
          </svg>
          {{ ticketCopied ? 'Copied!' : 'Copy Invite' }}
        </button>

        <!-- Chat toggle -->
        <button
          v-if="isActive"
          class="relative flex items-center gap-1.5 rounded-lg border border-border px-2.5 py-1.5 text-xs font-medium transition-colors"
          :class="showChat ? 'bg-primary text-primary-foreground border-primary' : 'text-foreground hover:bg-muted'"
          @click="showChat = !showChat"
        >
          <svg class="h-3.5 w-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
            <path stroke-linecap="round" stroke-linejoin="round" d="M8.625 9.75a.375.375 0 11-.75 0 .375.375 0 01.75 0zm0 0H8.25m4.125 0a.375.375 0 11-.75 0 .375.375 0 01.75 0zm0 0H12m4.125 0a.375.375 0 11-.75 0 .375.375 0 01.75 0zm0 0h-.375m-13.5 3.01c0 1.6 1.123 2.994 2.707 3.227 1.087.16 2.185.283 3.293.369V21l4.184-4.183a1.14 1.14 0 01.778-.332 48.294 48.294 0 005.83-.498c1.585-.233 2.708-1.626 2.708-3.228V6.741c0-1.602-1.123-2.995-2.707-3.228A48.394 48.394 0 0012 3c-2.392 0-4.744.175-7.043.513C3.373 3.746 2.25 5.14 2.25 6.741v6.018z" />
          </svg>
          Chat
          <!-- Unread badge -->
          <span
            v-if="unreadChat > 0 && !showChat"
            class="absolute -top-1.5 -right-1.5 flex h-4 min-w-[1rem] items-center justify-center rounded-full bg-destructive px-1 text-[0.6rem] font-bold text-destructive-foreground"
          >
            {{ unreadChat > 99 ? '99+' : unreadChat }}
          </span>
        </button>

        <!-- Leave -->
        <button
          v-if="isActive"
          class="flex items-center gap-1.5 rounded-lg bg-destructive px-3 py-1.5 text-xs font-medium text-destructive-foreground transition-colors hover:bg-destructive/90"
          @click="showLeaveConfirm = true"
        >
          <svg class="h-3.5 w-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
            <path stroke-linecap="round" stroke-linejoin="round" d="M15.75 9V5.25A2.25 2.25 0 0013.5 3h-6a2.25 2.25 0 00-2.25 2.25v13.5A2.25 2.25 0 007.5 21h6a2.25 2.25 0 002.25-2.25V15M12 9l-3 3m0 0l3 3m-3-3h12.75" />
          </svg>
          Leave
        </button>
      </div>
    </div>

    <!-- Main content area with optional chat sidebar -->
    <div class="flex flex-1 overflow-hidden">
      <!-- Video area -->
      <div class="flex-1 flex flex-col" :class="showChat ? 'border-r border-border' : ''">
        <div v-if="isActive" class="flex-1 overflow-auto p-4">
          <div class="mx-auto max-w-5xl space-y-4">
            <!-- Self video / camera status -->
            <div class="relative mx-auto aspect-video w-full max-w-3xl overflow-hidden rounded-2xl border border-border bg-card">
              <!-- Live self-preview from camera/screen -->
              <img
                v-if="selfVideoSrc"
                :src="selfVideoSrc"
                class="absolute inset-0 h-full w-full object-cover"
                :class="screenSharing ? '' : 'scale-x-[-1]'"
                alt="Self preview"
              />
              <!-- Fallback placeholder when no video frames -->
              <div v-else class="absolute inset-0 flex flex-col items-center justify-center gap-3 text-muted-foreground">
                <svg class="h-12 w-12 opacity-30" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1">
                  <path v-if="!screenSharing" stroke-linecap="round" stroke-linejoin="round" d="M15 10l4.553-2.276A1 1 0 0121 8.618v6.764a1 1 0 01-1.447.894L15 14M5 18h8a2 2 0 002-2V8a2 2 0 00-2-2H5a2 2 0 00-2 2v8a2 2 0 002 2z" />
                  <path v-else stroke-linecap="round" stroke-linejoin="round" d="M9 17.25v1.007a3 3 0 01-.879 2.122L7.5 21h9l-.621-.621A3 3 0 0115 18.257V17.25m6-12V15a2.25 2.25 0 01-2.25 2.25H5.25A2.25 2.25 0 013 15V5.25m18 0A2.25 2.25 0 0018.75 3H5.25A2.25 2.25 0 003 5.25m18 0V12a9 9 0 01-9 9m9-15V12a9 9 0 01-9 9m0 0a9 9 0 01-9-9V5.25" />
                </svg>
                <div class="text-center">
                  <p class="text-sm font-medium">
                    {{ screenSharing ? 'Screen Sharing Active' : videoEnabled ? 'Starting camera...' : 'Camera Off' }}
                  </p>
                  <p class="text-xs opacity-70">
                    {{ screenSharing ? 'Your screen is being shared with peers' : videoEnabled ? 'Waiting for first frame' : 'Click the camera button to enable' }}
                  </p>
                </div>
              </div>

              <!-- Status overlay -->
              <div class="absolute bottom-3 left-3 flex items-center gap-2 rounded-lg bg-black/60 px-3 py-1.5 backdrop-blur-sm">
                <span class="relative flex h-2 w-2" v-if="videoEnabled || screenSharing">
                  <span class="absolute inline-flex h-full w-full animate-ping rounded-full bg-success opacity-75" />
                  <span class="relative inline-flex h-2 w-2 rounded-full bg-success" />
                </span>
                <span v-else class="h-2 w-2 rounded-full bg-muted-foreground/50" />
                <span class="text-xs font-medium text-white">You</span>
              </div>

              <!-- Audio indicator -->
              <div class="absolute bottom-3 right-3 flex items-center gap-1.5 rounded-lg bg-black/60 px-2 py-1 backdrop-blur-sm">
                <svg v-if="audioEnabled" class="h-3 w-3 text-success" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                  <path stroke-linecap="round" stroke-linejoin="round" d="M12 18.75a6 6 0 006-6v-1.5m-6 7.5a6 6 0 01-6-6v-1.5m6 7.5v3.75m-3.75 0h7.5M12 15.75a3 3 0 01-3-3V4.5a3 3 0 116 0v8.25a3 3 0 01-3 3z" />
                </svg>
                <svg v-else class="h-3 w-3 text-destructive" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                  <path stroke-linecap="round" stroke-linejoin="round" d="M12 18.75a6 6 0 006-6v-1.5m-6 7.5a6 6 0 01-6-6v-1.5m6 7.5v3.75m-3.75 0h7.5M12 15.75a3 3 0 01-3-3V4.5a3 3 0 116 0v8.25a3 3 0 01-3 3z" />
                  <line x1="3" y1="3" x2="21" y2="21" stroke="currentColor" stroke-width="2" stroke-linecap="round" />
                </svg>
                <span class="text-[0.6rem] font-medium text-white">
                  {{ audioEnabled ? 'Mic On' : 'Muted' }}
                </span>
              </div>
            </div>

            <!-- Peer video grid -->
            <div v-if="peers.length > 0" class="grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-3">
              <div
                v-for="peer in peers"
                :key="peer.node_id"
                class="relative aspect-video overflow-hidden rounded-xl border border-border bg-card"
              >
                <!-- Rendered video frame from Rust bridge -->
                <img
                  v-if="videoFrames[peer.node_id]"
                  :src="videoFrames[peer.node_id]"
                  class="absolute inset-0 h-full w-full object-cover"
                  :alt="`Video from ${peer.node_id.slice(0, 8)}`"
                />
                <!-- Placeholder when no video -->
                <div v-else class="absolute inset-0 flex flex-col items-center justify-center gap-2 text-muted-foreground">
                  <div class="flex h-14 w-14 items-center justify-center rounded-full bg-muted text-xl font-bold">
                    {{ peer.node_id.slice(0, 2).toUpperCase() }}
                  </div>
                  <span class="text-xs font-mono opacity-60">{{ peer.node_id.slice(0, 12) }}...</span>
                </div>
                <!-- Peer status overlay -->
                <div class="absolute bottom-2 left-2 flex items-center gap-1.5 rounded bg-black/60 px-2 py-1 backdrop-blur-sm">
                  <span
                    class="h-1.5 w-1.5 rounded-full"
                    :class="peer.connected ? 'bg-success' : 'bg-warning'"
                  />
                  <span class="text-[0.6rem] font-medium text-white">
                    {{ peer.connected ? peer.node_id.slice(0, 8) : 'Connecting...' }}
                  </span>
                </div>
              </div>
            </div>

            <!-- No peers yet -->
            <div v-else class="rounded-xl border border-dashed border-border/60 bg-muted/10 p-8 text-center">
              <svg class="mx-auto h-8 w-8 text-muted-foreground/30" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.5">
                <path stroke-linecap="round" stroke-linejoin="round" d="M18 18.72a9.094 9.094 0 003.741-.479 3 3 0 00-4.682-2.72m.94 3.198l.001.031c0 .225-.012.447-.037.666A11.944 11.944 0 0112 21c-2.17 0-4.207-.576-5.963-1.584A6.062 6.062 0 016 18.719m12 0a5.971 5.971 0 00-.941-3.197m0 0A5.995 5.995 0 0012 12.75a5.995 5.995 0 00-5.058 2.772m0 0a3 3 0 00-4.681 2.72 8.986 8.986 0 003.74.477m.94-3.197a5.971 5.971 0 00-.94 3.197M15 6.75a3 3 0 11-6 0 3 3 0 016 0zm6 3a2.25 2.25 0 11-4.5 0 2.25 2.25 0 014.5 0zm-13.5 0a2.25 2.25 0 11-4.5 0 2.25 2.25 0 014.5 0z" />
              </svg>
              <p class="mt-3 text-sm text-muted-foreground">Waiting for peers to join...</p>
              <p class="mt-1 text-xs text-muted-foreground/70">Share the invite ticket using the button above.</p>
            </div>
          </div>
        </div>

        <!-- Not in active session -->
        <div v-else class="flex-1 flex items-center justify-center">
          <div class="text-center py-16">
            <div class="flex h-16 w-16 items-center justify-center rounded-full bg-muted/30 mx-auto mb-4">
              <svg class="h-8 w-8 text-muted-foreground/50" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.5">
                <path stroke-linecap="round" stroke-linejoin="round" d="M15 10l4.553-2.276A1 1 0 0121 8.618v6.764a1 1 0 01-1.447.894L15 14M5 18h8a2 2 0 002-2V8a2 2 0 00-2-2H5a2 2 0 00-2 2v8a2 2 0 002 2z" />
              </svg>
            </div>
            <h3 class="text-sm font-medium text-foreground">Session not found</h3>
            <p class="mt-1 text-xs text-muted-foreground">This session may have ended or you haven't joined it.</p>
            <button
              class="mt-4 rounded-lg bg-primary px-4 py-2 text-sm font-medium text-primary-foreground transition-colors hover:bg-primary/90"
              @click="router.push('/tutoring')"
            >
              Back to Lobby
            </button>
          </div>
        </div>

        <!-- Control bar (bottom) -->
        <div v-if="isActive" class="flex items-center justify-center gap-3 border-t border-border px-4 py-3 bg-card shrink-0">
          <!-- Mic toggle -->
          <button
            class="flex h-11 w-11 items-center justify-center rounded-full border transition-all"
            :class="audioEnabled
              ? 'border-border bg-muted text-foreground hover:bg-muted/80'
              : 'border-destructive/50 bg-destructive/10 text-destructive hover:bg-destructive/20'"
            :title="audioEnabled ? 'Mute microphone' : 'Unmute microphone'"
            @click="handleToggleAudio"
          >
            <svg v-if="audioEnabled" class="h-5 w-5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
              <path stroke-linecap="round" stroke-linejoin="round" d="M12 18.75a6 6 0 006-6v-1.5m-6 7.5a6 6 0 01-6-6v-1.5m6 7.5v3.75m-3.75 0h7.5M12 15.75a3 3 0 01-3-3V4.5a3 3 0 116 0v8.25a3 3 0 01-3 3z" />
            </svg>
            <svg v-else class="h-5 w-5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
              <path stroke-linecap="round" stroke-linejoin="round" d="M12 18.75a6 6 0 006-6v-1.5m-6 7.5a6 6 0 01-6-6v-1.5m6 7.5v3.75m-3.75 0h7.5M12 15.75a3 3 0 01-3-3V4.5a3 3 0 116 0v8.25a3 3 0 01-3 3z" />
              <line x1="4" y1="4" x2="20" y2="20" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" />
            </svg>
          </button>

          <!-- Camera toggle -->
          <button
            class="flex h-11 w-11 items-center justify-center rounded-full border transition-all"
            :class="videoEnabled && !screenSharing
              ? 'border-border bg-muted text-foreground hover:bg-muted/80'
              : 'border-destructive/50 bg-destructive/10 text-destructive hover:bg-destructive/20'"
            :title="videoEnabled ? 'Turn off camera' : 'Turn on camera'"
            @click="handleToggleVideo"
          >
            <svg v-if="videoEnabled && !screenSharing" class="h-5 w-5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
              <path stroke-linecap="round" stroke-linejoin="round" d="M15 10l4.553-2.276A1 1 0 0121 8.618v6.764a1 1 0 01-1.447.894L15 14M5 18h8a2 2 0 002-2V8a2 2 0 00-2-2H5a2 2 0 00-2 2v8a2 2 0 002 2z" />
            </svg>
            <svg v-else class="h-5 w-5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
              <path stroke-linecap="round" stroke-linejoin="round" d="M15.75 10.5l4.72-4.72a.75.75 0 011.28.53v11.38a.75.75 0 01-1.28.53l-4.72-4.72M12 18.75H4.5a2.25 2.25 0 01-2.25-2.25V9m12.841 9.091L16.5 19.5m-1.409-1.409c.407-.407.659-.97.659-1.591v-9a2.25 2.25 0 00-2.25-2.25h-9c-.621 0-1.184.252-1.591.659m12.182 12.182L2.909 5.909M1.5 4.5l1.409 1.409" />
            </svg>
          </button>

          <!-- Screen share toggle -->
          <button
            class="flex h-11 w-11 items-center justify-center rounded-full border transition-all"
            :class="screenSharing
              ? 'border-primary/50 bg-primary/10 text-primary hover:bg-primary/20'
              : 'border-border bg-muted text-foreground hover:bg-muted/80'"
            :title="screenSharing ? 'Stop screen sharing' : 'Share screen'"
            @click="handleToggleScreenShare"
          >
            <svg class="h-5 w-5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
              <path stroke-linecap="round" stroke-linejoin="round" d="M9 17.25v1.007a3 3 0 01-.879 2.122L7.5 21h9l-.621-.621A3 3 0 0115 18.257V17.25m6-12V15a2.25 2.25 0 01-2.25 2.25H5.25A2.25 2.25 0 013 15V5.25m18 0A2.25 2.25 0 0018.75 3H5.25A2.25 2.25 0 003 5.25m18 0V12a9 9 0 01-9 9m9-15V12a9 9 0 01-9 9m0 0a9 9 0 01-9-9V5.25" />
            </svg>
          </button>

          <!-- Divider -->
          <div class="h-6 w-px bg-border" />

          <!-- Leave button (large, destructive) -->
          <button
            class="flex h-11 items-center gap-2 rounded-full bg-destructive px-5 text-sm font-medium text-destructive-foreground transition-colors hover:bg-destructive/90"
            @click="showLeaveConfirm = true"
          >
            <svg class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
              <path stroke-linecap="round" stroke-linejoin="round" d="M15.75 9V5.25A2.25 2.25 0 0013.5 3h-6a2.25 2.25 0 00-2.25 2.25v13.5A2.25 2.25 0 007.5 21h6a2.25 2.25 0 002.25-2.25V15M12 9l-3 3m0 0l3 3m-3-3h12.75" />
            </svg>
            Leave
          </button>
        </div>
      </div>

      <!-- Chat sidebar -->
      <Transition
        enter-active-class="transition-all duration-200 ease-out"
        enter-from-class="w-0 opacity-0"
        enter-to-class="w-80 opacity-100"
        leave-active-class="transition-all duration-150 ease-in"
        leave-from-class="w-80 opacity-100"
        leave-to-class="w-0 opacity-0"
      >
        <div v-if="showChat && isActive" class="flex w-80 flex-col bg-card shrink-0 overflow-hidden">
          <!-- Chat header -->
          <div class="flex items-center justify-between border-b border-border px-4 py-3 shrink-0">
            <h3 class="text-sm font-semibold text-foreground">Session Chat</h3>
            <button
              class="rounded p-1 text-muted-foreground hover:bg-muted hover:text-foreground transition-colors"
              @click="showChat = false"
            >
              <svg class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                <path stroke-linecap="round" stroke-linejoin="round" d="M6 18L18 6M6 6l12 12" />
              </svg>
            </button>
          </div>

          <!-- Messages -->
          <div ref="chatScrollRef" class="flex-1 overflow-y-auto p-3 space-y-3">
            <div v-if="chatMessages.length === 0" class="flex flex-col items-center justify-center h-full text-center py-8">
              <svg class="h-8 w-8 text-muted-foreground/20 mb-2" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.5">
                <path stroke-linecap="round" stroke-linejoin="round" d="M8.625 9.75a.375.375 0 11-.75 0 .375.375 0 01.75 0zm0 0H8.25m4.125 0a.375.375 0 11-.75 0 .375.375 0 01.75 0zm0 0H12m4.125 0a.375.375 0 11-.75 0 .375.375 0 01.75 0zm0 0h-.375m-13.5 3.01c0 1.6 1.123 2.994 2.707 3.227 1.087.16 2.185.283 3.293.369V21l4.184-4.183a1.14 1.14 0 01.778-.332 48.294 48.294 0 005.83-.498c1.585-.233 2.708-1.626 2.708-3.228V6.741c0-1.602-1.123-2.995-2.707-3.228A48.394 48.394 0 0012 3c-2.392 0-4.744.175-7.043.513C3.373 3.746 2.25 5.14 2.25 6.741v6.018z" />
              </svg>
              <p class="text-xs text-muted-foreground">No messages yet</p>
              <p class="text-[0.65rem] text-muted-foreground/60 mt-0.5">Messages are sent P2P via gossip</p>
            </div>

            <div
              v-for="(msg, i) in chatMessages"
              :key="i"
              class="group"
            >
              <div
                class="rounded-lg px-3 py-2 text-sm"
                :class="msg.sender === 'self'
                  ? 'bg-primary/10 ml-6'
                  : 'bg-muted mr-6'"
              >
                <div class="flex items-center gap-2 mb-0.5">
                  <span class="text-[0.65rem] font-semibold" :class="msg.sender === 'self' ? 'text-primary' : 'text-foreground'">
                    {{ msg.sender_name || shortNodeId(msg.sender) }}
                  </span>
                  <span class="text-[0.6rem] text-muted-foreground/60">
                    {{ formatChatTime(msg.timestamp) }}
                  </span>
                </div>
                <p class="text-xs text-foreground/90 whitespace-pre-wrap break-words">{{ msg.text }}</p>
              </div>
            </div>
          </div>

          <!-- Chat input -->
          <div class="border-t border-border p-3 shrink-0">
            <div class="flex gap-2">
              <input
                v-model="chatInput"
                type="text"
                placeholder="Type a message..."
                class="flex-1 rounded-lg border border-border bg-background px-3 py-2 text-sm text-foreground placeholder-muted-foreground focus:border-primary focus:outline-none focus:ring-1 focus:ring-primary"
                @keydown.enter="handleSendChat"
              />
              <button
                class="flex h-9 w-9 items-center justify-center rounded-lg bg-primary text-primary-foreground transition-colors hover:bg-primary/90 disabled:opacity-40"
                :disabled="!chatInput.trim()"
                @click="handleSendChat"
              >
                <svg class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                  <path stroke-linecap="round" stroke-linejoin="round" d="M6 12L3.269 3.126A59.768 59.768 0 0121.485 12 59.77 59.77 0 013.27 20.876L5.999 12zm0 0h7.5" />
                </svg>
              </button>
            </div>
          </div>
        </div>
      </Transition>
    </div>

    <!-- Error bar -->
    <div v-if="lastError" class="shrink-0 border-t border-destructive/30 bg-destructive/5 px-4 py-2 text-xs text-destructive">
      {{ lastError }}
    </div>

    <!-- Leave confirmation modal -->
    <Teleport to="body">
      <Transition
        enter-active-class="transition-all duration-200"
        enter-from-class="opacity-0"
        enter-to-class="opacity-100"
        leave-active-class="transition-all duration-150"
        leave-from-class="opacity-100"
        leave-to-class="opacity-0"
      >
        <div v-if="showLeaveConfirm" class="fixed inset-0 z-50 flex items-center justify-center bg-black/50 backdrop-blur-sm" @click.self="showLeaveConfirm = false">
          <div class="w-full max-w-sm rounded-xl border border-border bg-card p-6 shadow-xl mx-4">
            <h2 class="text-lg font-semibold text-foreground">Leave Session?</h2>
            <p class="mt-2 text-sm text-muted-foreground">Your camera and microphone will stop broadcasting. Other participants will see you leave.</p>
            <div class="mt-6 flex justify-end gap-2">
              <button
                class="rounded-lg border border-border px-4 py-2 text-sm font-medium text-foreground transition-colors hover:bg-muted"
                @click="showLeaveConfirm = false"
              >
                Stay
              </button>
              <button
                class="rounded-lg bg-destructive px-4 py-2 text-sm font-medium text-destructive-foreground transition-colors hover:bg-destructive/90"
                @click="handleLeave"
              >
                Leave Session
              </button>
            </div>
          </div>
        </div>
      </Transition>
    </Teleport>
  </div>
</template>
