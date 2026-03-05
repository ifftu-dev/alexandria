import { ref, readonly } from 'vue'
import type { TutoringSessionInfo, TutoringSessionStatus, TutoringPeer } from '@/types'
import { useLocalApi } from './useLocalApi'

const { invoke } = useLocalApi()

// Module-level singleton state
const sessionStatus = ref<TutoringSessionStatus | null>(null)
const sessions = ref<TutoringSessionInfo[]>([])
const lastError = ref<string | null>(null)
const loading = ref(false)

let pollInterval: ReturnType<typeof setInterval> | null = null

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
    await refreshSessions()
  } catch (e: unknown) {
    lastError.value = e instanceof Error ? e.message : String(e)
  } finally {
    loading.value = false
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
    refreshStatus,
    refreshSessions,
    createRoom,
    joinRoom,
    leaveRoom,
    getPeers,
    startPolling,
    stopPolling,
  }
}
