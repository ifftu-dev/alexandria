import { ref, readonly, onUnmounted, getCurrentInstance } from 'vue'
import type {
  TutoringSessionInfo,
  TutoringSessionStatus,
  TutoringPeer,
  TutoringVideoFrame,
  TutoringChatMessage,
} from '@/types'
import { useLocalApi } from './useLocalApi'

const { invoke } = useLocalApi()

// ── Module-level singleton state ───────────────────────────────────

const sessionStatus = ref<TutoringSessionStatus | null>(null)
const sessions = ref<TutoringSessionInfo[]>([])
const lastError = ref<string | null>(null)
const loading = ref(false)

/** Map of node_id → latest JPEG data URL for rendering video frames. */
const videoFrames = ref<Record<string, string>>({})

/** Chat message history for the current session. */
const chatMessages = ref<TutoringChatMessage[]>([])

let pollInterval: ReturnType<typeof setInterval> | null = null

// ── Tauri event listeners (set up once globally) ───────────────────

let videoUnlisten: (() => void) | null = null
let chatUnlisten: (() => void) | null = null

async function setupEventListeners() {
  if (videoUnlisten) return // already set up

  try {
    // Dynamic import to avoid SSR issues and to get the listen function
    const { listen } = await import('@tauri-apps/api/event')

    videoUnlisten = await listen<TutoringVideoFrame>('tutoring:video-frame', (event) => {
      const { node_id, jpeg_b64 } = event.payload
      videoFrames.value = {
        ...videoFrames.value,
        [node_id]: `data:image/jpeg;base64,${jpeg_b64}`,
      }
    })

    chatUnlisten = await listen<TutoringChatMessage>('tutoring:chat', (event) => {
      chatMessages.value = [...chatMessages.value, event.payload]
    })
  } catch (e) {
    console.warn('Failed to set up Tauri event listeners:', e)
  }
}

function teardownEventListeners() {
  if (videoUnlisten) {
    videoUnlisten()
    videoUnlisten = null
  }
  if (chatUnlisten) {
    chatUnlisten()
    chatUnlisten = null
  }
}

// ── API functions ──────────────────────────────────────────────────

async function refreshStatus(): Promise<void> {
  try {
    sessionStatus.value = await invoke<TutoringSessionStatus | null>('tutoring_status')
    lastError.value = null
  } catch (e: unknown) {
    lastError.value = e instanceof Error ? e.message : String(e)
  }
}

async function refreshSessions(): Promise<void> {
  try {
    sessions.value = await invoke<TutoringSessionInfo[]>('tutoring_list_sessions')
  } catch (e: unknown) {
    lastError.value = e instanceof Error ? e.message : String(e)
  }
}

async function createRoom(title: string): Promise<TutoringSessionInfo> {
  loading.value = true
  lastError.value = null
  try {
    const session = await invoke<TutoringSessionInfo>('tutoring_create_room', { title })
    await setupEventListeners()
    chatMessages.value = []
    videoFrames.value = {}
    await refreshStatus()
    await refreshSessions()
    return session
  } catch (e: unknown) {
    const msg = e instanceof Error ? e.message : String(e)
    lastError.value = msg
    throw new Error(msg)
  } finally {
    loading.value = false
  }
}

async function joinRoom(ticket: string, title?: string): Promise<TutoringSessionInfo> {
  loading.value = true
  lastError.value = null
  try {
    const session = await invoke<TutoringSessionInfo>('tutoring_join_room', { ticket, title })
    await setupEventListeners()
    chatMessages.value = []
    videoFrames.value = {}
    await refreshStatus()
    await refreshSessions()
    return session
  } catch (e: unknown) {
    const msg = e instanceof Error ? e.message : String(e)
    lastError.value = msg
    throw new Error(msg)
  } finally {
    loading.value = false
  }
}

async function leaveRoom(): Promise<void> {
  loading.value = true
  lastError.value = null
  try {
    await invoke('tutoring_leave_room')
    sessionStatus.value = null
    videoFrames.value = {}
    chatMessages.value = []
    teardownEventListeners()
    await refreshSessions()
  } catch (e: unknown) {
    lastError.value = e instanceof Error ? e.message : String(e)
  } finally {
    loading.value = false
  }
}

async function toggleVideo(enable: boolean): Promise<boolean> {
  try {
    const result = await invoke<boolean>('tutoring_toggle_video', { enable })
    await refreshStatus()
    return result
  } catch (e: unknown) {
    lastError.value = e instanceof Error ? e.message : String(e)
    throw e
  }
}

async function toggleAudio(enable: boolean): Promise<boolean> {
  try {
    const result = await invoke<boolean>('tutoring_toggle_audio', { enable })
    await refreshStatus()
    return result
  } catch (e: unknown) {
    lastError.value = e instanceof Error ? e.message : String(e)
    throw e
  }
}

async function toggleScreenShare(enable: boolean): Promise<boolean> {
  try {
    const result = await invoke<boolean>('tutoring_toggle_screen_share', { enable })
    await refreshStatus()
    return result
  } catch (e: unknown) {
    lastError.value = e instanceof Error ? e.message : String(e)
    throw e
  }
}

async function sendChat(text: string): Promise<void> {
  try {
    await invoke('tutoring_send_chat', { text })
    // Add our own message to the local list (server doesn't echo it back)
    chatMessages.value = [
      ...chatMessages.value,
      {
        sender: 'self',
        sender_name: null,
        text,
        timestamp: Date.now(),
      },
    ]
  } catch (e: unknown) {
    lastError.value = e instanceof Error ? e.message : String(e)
    throw e
  }
}

async function getPeers(): Promise<TutoringPeer[]> {
  try {
    return await invoke<TutoringPeer[]>('tutoring_peers')
  } catch {
    return []
  }
}

function startPolling(intervalMs = 3000) {
  if (pollInterval) return
  refreshStatus()
  pollInterval = setInterval(refreshStatus, intervalMs)
}

function stopPolling() {
  if (pollInterval) {
    clearInterval(pollInterval)
    pollInterval = null
  }
}

export function useTutoringRoom() {
  return {
    sessionStatus: readonly(sessionStatus),
    sessions: readonly(sessions),
    lastError: readonly(lastError),
    loading: readonly(loading),
    videoFrames: readonly(videoFrames),
    chatMessages: readonly(chatMessages),
    refreshStatus,
    refreshSessions,
    createRoom,
    joinRoom,
    leaveRoom,
    toggleVideo,
    toggleAudio,
    toggleScreenShare,
    sendChat,
    getPeers,
    startPolling,
    stopPolling,
    setupEventListeners,
    teardownEventListeners,
  }
}
