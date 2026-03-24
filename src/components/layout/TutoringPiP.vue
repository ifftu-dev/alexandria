<script setup lang="ts">
import { computed, onMounted, onUnmounted } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { usePlatform } from '@/composables/usePlatform'
import { useTutoringRoom } from '@/composables/useTutoringRoom'

const route = useRoute()
const router = useRouter()
const { isMobilePlatform } = usePlatform()
const {
  sessionStatus,
  videoFrames,
  micLevel,
  outputLevel,
  refreshStatus,
  setupEventListeners,
  startPolling,
  stopPolling,
  toggleAudio,
  leaveRoom,
} = useTutoringRoom()

onMounted(async () => {
  await setupEventListeners()
  await refreshStatus()
  startPolling(2000)
})

onUnmounted(() => {
  stopPolling()
})

const isVisible = computed(() => {
  return Boolean(sessionStatus.value) && route.name !== 'tutoring-session'
})

const sessionTitle = computed(() => sessionStatus.value?.session_title ?? 'Live Tutoring')
const sessionId = computed(() => sessionStatus.value?.session_id ?? '')
const audioEnabled = computed(() => sessionStatus.value?.audio_enabled ?? false)
const peerCount = computed(() => sessionStatus.value?.peers.length ?? 0)
const connectedPeerCount = computed(() => {
  return sessionStatus.value?.peers.filter(peer => peer.connected).length ?? 0
})

const previewSrc = computed(() => {
  const session = sessionStatus.value
  if (!session) return null
  for (const peer of session.peers) {
    const frame = videoFrames.value[peer.node_id]
    if (frame) return frame
  }
  return videoFrames.value.self ?? null
})

async function openSession() {
  if (!sessionId.value) return
  await router.push(`/tutoring/${sessionId.value}`)
}

async function handleToggleAudio() {
  try {
    await toggleAudio(!audioEnabled.value)
  } catch {
    // Error is surfaced through the shared tutoring state.
  }
}

async function handleLeave() {
  try {
    await leaveRoom()
    if (route.path.startsWith('/tutoring')) {
      await router.push('/tutoring')
    }
  } catch {
    // Error is surfaced through the shared tutoring state.
  }
}
</script>

<template>
  <div
    v-if="isVisible"
    class="fixed z-50"
    :class="isMobilePlatform ? 'bottom-24 right-3 left-3' : 'bottom-6 right-6 w-[21rem]'"
  >
    <div class="overflow-hidden rounded-2xl border border-border/80 bg-card/96 shadow-2xl ring-1 ring-black/5 backdrop-blur-md">
      <button
        class="relative block w-full overflow-hidden text-left"
        :class="isMobilePlatform ? 'aspect-[16/7]' : 'aspect-[16/9]'"
        @click="openSession"
      >
        <img
          v-if="previewSrc"
          :src="previewSrc"
          class="absolute inset-0 h-full w-full object-cover"
          alt="Tutoring preview"
        />
        <div v-else class="absolute inset-0 bg-[radial-gradient(circle_at_top,_rgba(34,197,94,0.22),_transparent_48%),linear-gradient(135deg,_rgba(15,23,42,0.96),_rgba(30,41,59,0.88))]" />
        <div class="absolute inset-0 bg-gradient-to-t from-black/75 via-black/20 to-transparent" />
        <div class="absolute left-3 top-3 flex items-center gap-2 rounded-full bg-black/60 px-2.5 py-1 text-[0.65rem] font-semibold uppercase tracking-[0.18em] text-white backdrop-blur-sm">
          <span class="relative flex h-2 w-2">
            <span class="absolute inline-flex h-full w-full animate-ping rounded-full bg-emerald-400 opacity-80" />
            <span class="relative inline-flex h-2 w-2 rounded-full bg-emerald-400" />
          </span>
          <span>Live</span>
        </div>
        <div class="absolute inset-x-0 bottom-0 p-3">
          <div class="flex items-end justify-between gap-3">
            <div class="min-w-0">
              <p class="truncate text-sm font-semibold text-white">{{ sessionTitle }}</p>
              <p class="mt-0.5 text-xs text-white/75">
                {{ connectedPeerCount }}/{{ peerCount }} peers connected
              </p>
            </div>
            <div class="flex items-center gap-2 rounded-full bg-black/55 px-2.5 py-1 text-[0.7rem] font-medium text-white backdrop-blur-sm">
              <span>Mic {{ audioEnabled ? 'On' : 'Off' }}</span>
              <span class="h-1.5 w-1.5 rounded-full" :class="outputLevel > 0.04 ? 'bg-sky-400' : micLevel > 0.04 ? 'bg-emerald-400' : 'bg-white/30'" />
            </div>
          </div>
        </div>
      </button>

      <div class="flex items-center justify-between gap-2 border-t border-border/70 px-3 py-2.5">
        <button
          class="inline-flex items-center gap-2 rounded-lg px-2.5 py-1.5 text-xs font-medium text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
          @click="handleToggleAudio"
        >
          <svg class="h-3.5 w-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
            <path stroke-linecap="round" stroke-linejoin="round" d="M12 18.75a6 6 0 006-6v-1.5m-6 7.5a6 6 0 01-6-6v-1.5m6 7.5v3.75m-3.75 0h7.5M12 15.75a3 3 0 01-3-3V4.5a3 3 0 116 0v8.25a3 3 0 01-3 3z" />
          </svg>
          <span>{{ audioEnabled ? 'Mute' : 'Unmute' }}</span>
        </button>

        <div class="flex items-center gap-2">
          <button
            class="inline-flex items-center gap-2 rounded-lg border border-border px-2.5 py-1.5 text-xs font-medium text-foreground transition-colors hover:bg-muted"
            @click="openSession"
          >
            <span>Open</span>
          </button>
          <button
            class="inline-flex items-center gap-2 rounded-lg bg-destructive px-2.5 py-1.5 text-xs font-medium text-destructive-foreground transition-colors hover:bg-destructive/90"
            @click="handleLeave"
          >
            <span>Leave</span>
          </button>
        </div>
      </div>
    </div>
  </div>
</template>
