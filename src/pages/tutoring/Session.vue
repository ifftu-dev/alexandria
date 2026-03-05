<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useTutoringRoom } from '@/composables/useTutoringRoom'

const route = useRoute()
const router = useRouter()
const {
  sessionStatus,
  lastError,
  refreshStatus,
  leaveRoom,
  startPolling,
  stopPolling,
} = useTutoringRoom()

const sessionId = computed(() => route.params.id as string)
const ticketCopied = ref(false)
const showLeaveConfirm = ref(false)

onMounted(() => {
  refreshStatus()
  startPolling(2000)
})

onUnmounted(() => {
  stopPolling()
})

const isActive = computed(() => sessionStatus.value?.session_id === sessionId.value)
const peers = computed(() => sessionStatus.value?.peers ?? [])
const peerCount = computed(() => peers.value.length)
const connectedPeerCount = computed(() => peers.value.filter(p => p.connected).length)

async function copyTicket() {
  if (!sessionStatus.value?.ticket) return
  try {
    await navigator.clipboard.writeText(sessionStatus.value.ticket)
    ticketCopied.value = true
    setTimeout(() => { ticketCopied.value = false }, 2000)
  } catch {
    // fallback: select all in a temporary textarea
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
</script>

<template>
  <div class="flex flex-col h-[calc(100vh-4rem)]">
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

    <!-- Main content area -->
    <div class="flex-1 flex flex-col items-center justify-center p-6 overflow-auto">
      <!-- Video grid placeholder -->
      <div v-if="isActive" class="w-full max-w-4xl space-y-6">
        <!-- Self video placeholder -->
        <div class="relative mx-auto aspect-video w-full max-w-2xl overflow-hidden rounded-2xl border border-border bg-card">
          <div class="absolute inset-0 flex flex-col items-center justify-center gap-3 text-muted-foreground">
            <svg class="h-12 w-12 opacity-30" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1">
              <path stroke-linecap="round" stroke-linejoin="round" d="M15 10l4.553-2.276A1 1 0 0121 8.618v6.764a1 1 0 01-1.447.894L15 14M5 18h8a2 2 0 002-2V8a2 2 0 00-2-2H5a2 2 0 00-2 2v8a2 2 0 002 2z" />
            </svg>
            <div class="text-center">
              <p class="text-sm font-medium">Camera active</p>
              <p class="text-xs opacity-70">Video rendering coming in Phase 1.1</p>
            </div>
          </div>

          <!-- Status overlay -->
          <div class="absolute bottom-3 left-3 flex items-center gap-2 rounded-lg bg-black/60 px-3 py-1.5 backdrop-blur-sm">
            <span class="relative flex h-2 w-2">
              <span class="absolute inline-flex h-full w-full animate-ping rounded-full bg-success opacity-75" />
              <span class="relative inline-flex h-2 w-2 rounded-full bg-success" />
            </span>
            <span class="text-xs font-medium text-white">You</span>
          </div>
        </div>

        <!-- Peer grid -->
        <div v-if="peers.length > 0" class="grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-3">
          <div
            v-for="peer in peers"
            :key="peer.node_id"
            class="relative aspect-video overflow-hidden rounded-xl border border-border bg-card"
          >
            <div class="absolute inset-0 flex flex-col items-center justify-center gap-2 text-muted-foreground">
              <div class="flex h-12 w-12 items-center justify-center rounded-full bg-muted text-lg font-bold">
                {{ peer.node_id.slice(0, 2).toUpperCase() }}
              </div>
              <span class="text-xs font-mono opacity-60">{{ peer.node_id.slice(0, 12) }}...</span>
            </div>
            <div class="absolute bottom-2 left-2 flex items-center gap-1.5 rounded bg-black/60 px-2 py-1 backdrop-blur-sm">
              <span
                class="h-1.5 w-1.5 rounded-full"
                :class="peer.connected ? 'bg-success' : 'bg-warning'"
              />
              <span class="text-[0.6rem] font-medium text-white">
                {{ peer.connected ? 'Connected' : 'Connecting...' }}
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

      <!-- Not in active session -->
      <div v-else class="text-center py-16">
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

    <!-- Error -->
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
