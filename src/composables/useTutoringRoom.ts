import { ref, readonly } from 'vue'
import type {
  TutoringSessionInfo,
  TutoringSessionStatus,
  TutoringPeer,
  TutoringVideoFrame,
  TutoringChatMessage,
  DeviceCheckResult,
  DeviceList,
  AudioLevelEvent,
} from '@/types'
import { useLocalApi } from './useLocalApi'
import { onProfileLocked } from './useProfiles'

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

/** Current mic input level (0.0–1.0) for the VU meter. Updated ~20x/s via Tauri events. */
const micLevel = ref(0)

/** Current output level (0.0–1.0) from remote audio playback. Updated ~20x/s via Tauri events. */
const outputLevel = ref(0)

let pollInterval: ReturnType<typeof setInterval> | null = null
let pollSubscribers = 0

// ── Tauri event listeners (set up once globally) ───────────────────

let videoUnlisten: (() => void) | null = null
let chatUnlisten: (() => void) | null = null
let peerEndedUnlisten: (() => void) | null = null
let peerNameUnlisten: (() => void) | null = null
let audioLevelUnlisten: (() => void) | null = null
let listenerSetup: Promise<void> | null = null
let profileGeneration = 0

function isCurrentProfile(generation: number): boolean {
  return generation === profileGeneration
}

/** Pending video frames batched via rAF to avoid overwhelming Vue reactivity. */
const pendingFrames: Record<string, string> = {}
let rafId: number | null = null

function flushVideoFrames() {
  rafId = null
  const keys = Object.keys(pendingFrames)
  if (keys.length === 0) return
  const updated = { ...videoFrames.value }
  for (const key of keys) {
    updated[key] = pendingFrames[key]!
    delete pendingFrames[key]
  }
  videoFrames.value = updated
}

async function setupEventListeners() {
  if (
    videoUnlisten
    && chatUnlisten
    && peerEndedUnlisten
    && peerNameUnlisten
    && audioLevelUnlisten
  ) return
  if (listenerSetup) return listenerSetup

  const generation = profileGeneration
  const setup = (async () => {
    const installed: Array<() => void> = []
    const { listen } = await import('@tauri-apps/api/event')
    if (!isCurrentProfile(generation)) return

    try {
      const nextVideoUnlisten = await listen<TutoringVideoFrame>('tutoring:video-frame', (event) => {
        if (!isCurrentProfile(generation)) return
        const { node_id, jpeg_b64 } = event.payload
        pendingFrames[node_id] = `data:image/jpeg;base64,${jpeg_b64}`
        if (rafId === null) {
          rafId = requestAnimationFrame(flushVideoFrames)
        }
      })
      installed.push(nextVideoUnlisten)
      if (!isCurrentProfile(generation)) return

      const nextChatUnlisten = await listen<TutoringChatMessage>('tutoring:chat', (event) => {
        if (!isCurrentProfile(generation)) return
        chatMessages.value = [...chatMessages.value, event.payload]
        if (!chatOpen.value) {
          unreadChatCount.value++
        }
      })
      installed.push(nextChatUnlisten)
      if (!isCurrentProfile(generation)) return

      const nextPeerEndedUnlisten = await listen<{ node_id: string }>(
        'tutoring:peer-video-ended',
        (event) => {
          if (!isCurrentProfile(generation)) return
          const { node_id } = event.payload
          const updated = { ...videoFrames.value }
          delete updated[node_id]
          videoFrames.value = updated
        },
      )
      installed.push(nextPeerEndedUnlisten)
      if (!isCurrentProfile(generation)) return

      const nextPeerNameUnlisten = await listen<{ node_id: string; display_name: string }>(
        'tutoring:peer-name',
        (event) => {
          if (!isCurrentProfile(generation)) return
          const { node_id, display_name } = event.payload
          peerNames.value = {
            ...peerNames.value,
            [node_id]: display_name,
          }
        },
      )
      installed.push(nextPeerNameUnlisten)
      if (!isCurrentProfile(generation)) return

      const nextAudioLevelUnlisten = await listen<AudioLevelEvent>(
        'tutoring:audio-level',
        (event) => {
          if (!isCurrentProfile(generation)) return
          micLevel.value = event.payload.mic_level
          outputLevel.value = event.payload.output_level
        },
      )
      installed.push(nextAudioLevelUnlisten)
      if (!isCurrentProfile(generation)) return

      videoUnlisten = nextVideoUnlisten
      chatUnlisten = nextChatUnlisten
      peerEndedUnlisten = nextPeerEndedUnlisten
      peerNameUnlisten = nextPeerNameUnlisten
      audioLevelUnlisten = nextAudioLevelUnlisten
      installed.length = 0
    } finally {
      for (const unlisten of installed) unlisten()
    }
  })()
  listenerSetup = setup
  try {
    await setup
  } catch (e) {
    console.warn('Failed to set up Tauri event listeners:', e)
  } finally {
    if (listenerSetup === setup) listenerSetup = null
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
  if (audioLevelUnlisten) {
    audioLevelUnlisten()
    audioLevelUnlisten = null
  }
  if (rafId !== null) {
    cancelAnimationFrame(rafId)
    rafId = null
  }
  for (const key of Object.keys(pendingFrames)) {
    delete pendingFrames[key]
  }
  micLevel.value = 0
  outputLevel.value = 0
}

function resetForProfileLock(): void {
  profileGeneration++
  listenerSetup = null
  if (pollInterval) clearInterval(pollInterval)
  pollInterval = null
  pollSubscribers = 0
  sessionStatus.value = null
  sessions.value = []
  lastError.value = null
  loading.value = false
  videoFrames.value = {}
  chatMessages.value = []
  peerNames.value = {}
  unreadChatCount.value = 0
  chatOpen.value = false
  teardownEventListeners()
}

onProfileLocked(resetForProfileLock)

// ── API functions ──────────────────────────────────────────────────

async function refreshStatus(): Promise<void> {
  const generation = profileGeneration
  try {
    const status = await invoke<TutoringSessionStatus | null>('tutoring_status')
    if (isCurrentProfile(generation)) {
      sessionStatus.value = status
      lastError.value = null
    }
  } catch (e: unknown) {
    if (isCurrentProfile(generation)) {
      lastError.value = e instanceof Error ? e.message : String(e)
    }
  }
}

async function refreshSessions(): Promise<void> {
  const generation = profileGeneration
  try {
    const loaded = await invoke<TutoringSessionInfo[]>('tutoring_list_sessions')
    if (isCurrentProfile(generation)) sessions.value = loaded
  } catch (e: unknown) {
    if (isCurrentProfile(generation)) {
      lastError.value = e instanceof Error ? e.message : String(e)
    }
  }
}

async function createRoom(
  title: string,
  displayName?: string,
  cameraId?: string | null,
  micId?: string | null,
  speakerId?: string | null,
): Promise<TutoringSessionInfo> {
  const generation = profileGeneration
  loading.value = true
  lastError.value = null
  try {
    const session = await invoke<TutoringSessionInfo>('tutoring_create_room', {
      title,
      displayName: displayName || null,
      cameraId: cameraId || null,
      micId: micId || null,
      speakerId: speakerId || null,
    })
    if (!isCurrentProfile(generation)) throw new Error('Profile changed while creating room')
    await setupEventListeners()
    if (!isCurrentProfile(generation)) throw new Error('Profile changed while creating room')
    chatMessages.value = []
    videoFrames.value = {}
    peerNames.value = {}
    unreadChatCount.value = 0
    await refreshStatus()
    await refreshSessions()
    return session
  } catch (e: unknown) {
    const msg = e instanceof Error ? e.message : String(e)
    if (isCurrentProfile(generation)) lastError.value = msg
    throw new Error(msg)
  } finally {
    if (isCurrentProfile(generation)) loading.value = false
  }
}

async function joinRoom(
  ticket: string,
  title?: string,
  displayName?: string,
  cameraId?: string | null,
  micId?: string | null,
  speakerId?: string | null,
): Promise<TutoringSessionInfo> {
  const generation = profileGeneration
  loading.value = true
  lastError.value = null
  try {
    const session = await invoke<TutoringSessionInfo>('tutoring_join_room', {
      ticket,
      title: title || null,
      displayName: displayName || null,
      cameraId: cameraId || null,
      micId: micId || null,
      speakerId: speakerId || null,
    })
    if (!isCurrentProfile(generation)) throw new Error('Profile changed while joining room')
    await setupEventListeners()
    if (!isCurrentProfile(generation)) throw new Error('Profile changed while joining room')
    chatMessages.value = []
    videoFrames.value = {}
    peerNames.value = {}
    unreadChatCount.value = 0
    await refreshStatus()
    await refreshSessions()
    return session
  } catch (e: unknown) {
    const msg = e instanceof Error ? e.message : String(e)
    if (isCurrentProfile(generation)) lastError.value = msg
    throw new Error(msg)
  } finally {
    if (isCurrentProfile(generation)) loading.value = false
  }
}

async function leaveRoom(): Promise<void> {
  const generation = profileGeneration
  loading.value = true
  lastError.value = null
  try {
    await invoke('tutoring_leave_room')
    if (!isCurrentProfile(generation)) return
    sessionStatus.value = null
    videoFrames.value = {}
    chatMessages.value = []
    peerNames.value = {}
    unreadChatCount.value = 0
    teardownEventListeners()
    await refreshSessions()
  } catch (e: unknown) {
    if (isCurrentProfile(generation)) {
      lastError.value = e instanceof Error ? e.message : String(e)
    }
  } finally {
    if (isCurrentProfile(generation)) loading.value = false
  }
}

async function toggleVideo(enable: boolean): Promise<boolean> {
  const generation = profileGeneration
  try {
    const result = await invoke<boolean>('tutoring_toggle_video', { enable })
    if (isCurrentProfile(generation)) await refreshStatus()
    return result
  } catch (e: unknown) {
    if (isCurrentProfile(generation)) lastError.value = e instanceof Error ? e.message : String(e)
    throw e
  }
}

async function toggleAudio(enable: boolean): Promise<boolean> {
  const generation = profileGeneration
  try {
    const result = await invoke<boolean>('tutoring_toggle_audio', { enable })
    if (isCurrentProfile(generation)) await refreshStatus()
    return result
  } catch (e: unknown) {
    if (isCurrentProfile(generation)) lastError.value = e instanceof Error ? e.message : String(e)
    throw e
  }
}

async function toggleScreenShare(enable: boolean): Promise<boolean> {
  const generation = profileGeneration
  try {
    const result = await invoke<boolean>('tutoring_toggle_screen_share', { enable })
    if (isCurrentProfile(generation)) await refreshStatus()
    return result
  } catch (e: unknown) {
    if (isCurrentProfile(generation)) lastError.value = e instanceof Error ? e.message : String(e)
    throw e
  }
}

async function sendChat(text: string): Promise<void> {
  const generation = profileGeneration
  try {
    await invoke('tutoring_send_chat', { text })
    if (!isCurrentProfile(generation)) return
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
    if (isCurrentProfile(generation)) lastError.value = e instanceof Error ? e.message : String(e)
    throw e
  }
}

async function getPeers(): Promise<TutoringPeer[]> {
  const generation = profileGeneration
  try {
    const peers = await invoke<TutoringPeer[]>('tutoring_peers')
    return isCurrentProfile(generation) ? peers : []
  } catch {
    return []
  }
}

/** Get diagnostic info about the current A/V pipeline state. */
async function getDiagnostics(): Promise<Record<string, unknown> | null> {
  const generation = profileGeneration
  try {
    const diagnostics = await invoke<Record<string, unknown> | null>('tutoring_diagnostics')
    return isCurrentProfile(generation) ? diagnostics : null
  } catch (e: unknown) {
    if (isCurrentProfile(generation)) console.warn('Failed to get diagnostics:', e)
    return null
  }
}

function emptyDeviceList(): DeviceList {
  return {
    audio_inputs: [],
    audio_outputs: [],
    cameras: [],
    selected_audio_input: null,
    selected_audio_output: null,
  }
}

/** List all available audio and camera devices. */
async function listDevices(): Promise<DeviceList> {
  const generation = profileGeneration
  try {
    const devices = await invoke<DeviceList>('tutoring_list_devices')
    return isCurrentProfile(generation) ? devices : emptyDeviceList()
  } catch (e: unknown) {
    if (isCurrentProfile(generation)) console.warn('Failed to list devices:', e)
    return emptyDeviceList()
  }
}

async function setAudioDevices(micId?: string | null, speakerId?: string | null): Promise<void> {
  const generation = profileGeneration
  try {
    await invoke('tutoring_set_audio_devices', {
      micId: micId || null,
      speakerId: speakerId || null,
    })
    if (isCurrentProfile(generation)) await refreshStatus()
  } catch (e: unknown) {
    if (isCurrentProfile(generation)) lastError.value = e instanceof Error ? e.message : String(e)
    throw e
  }
}

/** Check device availability (camera + mic) before joining a session. */
async function checkDevices(): Promise<DeviceCheckResult> {
  const generation = profileGeneration
  try {
    const result = await invoke<DeviceCheckResult>('tutoring_check_devices')
    return isCurrentProfile(generation)
      ? result
      : {
          has_camera: false,
          camera_name: null,
          has_audio: false,
          error: 'Profile changed while checking devices',
        }
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
  pollSubscribers += 1
  if (pollInterval) return
  void refreshStatus()
  pollInterval = setInterval(() => void refreshStatus(), intervalMs)
}

function stopPolling() {
  if (pollSubscribers > 0) {
    pollSubscribers -= 1
  }
  if (pollSubscribers === 0 && pollInterval) {
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
    micLevel: readonly(micLevel),
    outputLevel: readonly(outputLevel),
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
    listDevices,
    setAudioDevices,
    getDiagnostics,
    startPolling,
    stopPolling,
    setupEventListeners,
    teardownEventListeners,
    setChatOpen,
  }
}
