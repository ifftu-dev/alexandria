<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
import { useRouter } from 'vue-router'
import { useTutoringRoom } from '@/composables/useTutoringRoom'

const router = useRouter()
const {
  sessions,
  lastError,
  loading,
  refreshSessions,
  createRoom,
  joinRoom,
} = useTutoringRoom()

const showCreateModal = ref(false)
const showJoinModal = ref(false)
const newRoomTitle = ref('')
const createDisplayName = ref('')
const joinTicket = ref('')
const joinTitle = ref('')
const joinDisplayName = ref('')

onMounted(() => {
  refreshSessions()
})

const pastSessions = computed(() =>
  sessions.value.filter(s => s.status !== 'active')
)

const activeSession = computed(() =>
  sessions.value.find(s => s.status === 'active')
)

async function handleCreate() {
  if (!newRoomTitle.value.trim()) return
  try {
    const session = await createRoom(
      newRoomTitle.value.trim(),
      createDisplayName.value.trim() || undefined,
    )
    showCreateModal.value = false
    newRoomTitle.value = ''
    createDisplayName.value = ''
    router.push(`/tutoring/${session.id}`)
  } catch {
    // error is in lastError
  }
}

async function handleJoin() {
  if (!joinTicket.value.trim()) return
  try {
    const session = await joinRoom(
      joinTicket.value.trim(),
      joinTitle.value.trim() || undefined,
      joinDisplayName.value.trim() || undefined,
    )
    showJoinModal.value = false
    joinTicket.value = ''
    joinTitle.value = ''
    joinDisplayName.value = ''
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
        class="group rounded-xl border border-border bg-card p-6 text-left transition-all hover:border-primary/40 hover:shadow-md"
        @click="showCreateModal = true"
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
        class="group rounded-xl border border-border bg-card p-6 text-left transition-all hover:border-primary/40 hover:shadow-md"
        @click="showJoinModal = true"
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
        <div v-if="showCreateModal" class="fixed inset-0 z-50 flex items-center justify-center bg-black/50 backdrop-blur-sm" @click.self="showCreateModal = false">
          <div class="w-full max-w-md rounded-xl border border-border bg-card p-6 shadow-xl mx-4">
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
                  @keydown.enter="handleCreate"
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
                @click="showCreateModal = false"
              >
                Cancel
              </button>
              <button
                class="rounded-lg bg-primary px-4 py-2 text-sm font-medium text-primary-foreground transition-colors hover:bg-primary/90 disabled:opacity-50"
                :disabled="!newRoomTitle.trim() || loading"
                @click="handleCreate"
              >
                {{ loading ? 'Starting...' : 'Start Session' }}
              </button>
            </div>
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
        <div v-if="showJoinModal" class="fixed inset-0 z-50 flex items-center justify-center bg-black/50 backdrop-blur-sm" @click.self="showJoinModal = false">
          <div class="w-full max-w-md rounded-xl border border-border bg-card p-6 shadow-xl mx-4">
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
                @click="showJoinModal = false"
              >
                Cancel
              </button>
              <button
                class="rounded-lg bg-primary px-4 py-2 text-sm font-medium text-primary-foreground transition-colors hover:bg-primary/90 disabled:opacity-50"
                :disabled="!joinTicket.trim() || loading"
                @click="handleJoin"
              >
                {{ loading ? 'Joining...' : 'Join Session' }}
              </button>
            </div>
          </div>
        </div>
      </Transition>
    </Teleport>
  </div>
</template>
