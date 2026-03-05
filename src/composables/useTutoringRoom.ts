import { ref, readonly } from 'vue'
import type {
  TutoringSessionInfo,
  TutoringSessionStatus,
  TutoringPeer,
  TutoringVideoFrame,
  TutoringChatMessage,
  DeviceCheckResult,
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

/** Map of node_id → display name (learned via gossip /names topic). */
const peerNames = ref<Record<string, string>>({})

/** Number of chat messages received while the chat panel was closed. */
const unreadChatCount = ref(0)

/** Whether the chat panel is currently visible (set by the session page). */
const chatOpen = ref(false)

let pollInterval: ReturnType<typeof setInterval> | null = null

// ── Tauri event listeners (set up once globally) ───────────────────

let videoUnlisten: (() => void) | null = null
let chatUnlisten: (() => void) | null = null
let peerEndedUnlisten: (() => void) | null = null
let peerNameUnlisten: (() => void) | null = null

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
      if (!chatOpen.value) {
        unreadChatCount.value++
      }
    })

    peerEndedUnlisten = await listen<{ node_id: string }>('tutoring:peer-video-ended', (event) => {
      const { node_id } = event.payload
      const updated = { ...videoFrames.value }
      delete updated[node_id]
      videoFrames.value = updated
    })

    peerNameUnlisten = await listen<{ node_id: string; display_name: string }>('tutoring:peer-name', (event) => {
      const { node_id, display_name } = event.payload
      peerNames.value = {
        ...peerNames.value,
        [node_id]: display_name,
      }
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
  if (peerEndedUnlisten) {
    peerEndedUnlisten()
    peerEndedUnlisten = null
  }
  if (peerNameUnlisten) {
    peerNameUnlisten()
    peerNameUnlisten = null
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

async function createRoom(title: string, displayName?: string): Promise<TutoringSessionInfo> {
  loading.value = true
  lastError.value = null
  try {
    const session = await invoke<TutoringSessionInfo>('tutoring_create_room', {
      title,
      displayName: displayName || null,
    })
    await setupEventListeners()
    chatMessages.value = []
    videoFrames.value = {}
    peerNames.value = {}
    unreadChatCount.value = 0
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

async function joinRoom(ticket: string, title?: string, displayName?: string): Promise<TutoringSessionInfo> {
  loading.value = true
  lastError.value = null
  try {
    const session = await invoke<TutoringSessionInfo>('tutoring_join_room', {
      ticket,
      title: title || null,
      displayName: displayName || null,
    })
    await setupEventListeners()
    chatMessages.value = []
    videoFrames.value = {}
    peerNames.value = {}
    unreadChatCount.value = 0
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
    peerNames.value = {}
    unreadChatCount.value = 0
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

/** Check device availability (camera + mic) before joining a session. */
async function checkDevices(): Promise<DeviceCheckResult> {
  try {
    return await invoke<DeviceCheckResult>('tutoring_check_devices')
  } catch (e: unknown) {
    return {
      has_camera: false,
      camera_name: null,
      has_audio: false,
      error: e instanceof Error ? e.message : String(e),
    }
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

/** Set the chat panel open state (controls unread counter). */
function setChatOpen(open: boolean) {
  chatOpen.value = open
  if (open) {
    unreadChatCount.value = 0
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
    peerNames: readonly(peerNames),
    unreadChatCount: readonly(unreadChatCount),
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
    checkDevices,
    startPolling,
    stopPolling,
    setupEventListeners,
    teardownEventListeners,
    setChatOpen,
  }
}
