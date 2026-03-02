import { ref, readonly } from 'vue'
import type { P2PStatus, PeerInfo } from '@/types'
import { useLocalApi } from './useLocalApi'

const { invoke } = useLocalApi()

// Module-level singleton
const status = ref<P2PStatus | null>(null)
const lastError = ref<string | null>(null)
const polling = ref(false)
let pollInterval: ReturnType<typeof setInterval> | null = null
let rapidPollTimer: ReturnType<typeof setInterval> | null = null

async function refreshStatus(): Promise<void> {
  try {
    status.value = await invoke<P2PStatus>('p2p_status')
    lastError.value = null
  } catch (e: unknown) {
    lastError.value = e instanceof Error ? e.message : String(e)
    status.value = null
  }
}

async function start(): Promise<void> {
  // Skip if we already know the node is running
  if (status.value?.is_running) return
  try {
    await invoke('p2p_start')
  } catch {
    // Fire-and-forget: the backend spawns startup in the background.
    // Any error here (e.g. wallet locked) is non-fatal.
  }
  await refreshStatus()
}

async function stop(): Promise<void> {
  await invoke('p2p_stop')
  await refreshStatus()
}

async function peers(): Promise<PeerInfo[]> {
  return invoke<string[]>('p2p_peers')
}

function startPolling(intervalMs = 10000) {
  if (polling.value) return
  polling.value = true

  // Immediate first check
  refreshStatus()

  // Rapid polling every 2s for the first 30s so we pick up the
  // P2P node coming online quickly after auto-start.
  rapidPollTimer = setInterval(async () => {
    await refreshStatus()
    // Once the node is running, stop rapid polling — the regular
    // interval will keep things updated.
    if (status.value?.is_running) {
      if (rapidPollTimer) {
        clearInterval(rapidPollTimer)
        rapidPollTimer = null
      }
    }
  }, 2000)

  // After 30s, clear rapid polling regardless and rely on the
  // normal interval.
  setTimeout(() => {
    if (rapidPollTimer) {
      clearInterval(rapidPollTimer)
      rapidPollTimer = null
    }
  }, 30000)

  // Normal interval for ongoing status updates
  pollInterval = setInterval(refreshStatus, intervalMs)
}

function stopPolling() {
  polling.value = false
  if (pollInterval) {
    clearInterval(pollInterval)
    pollInterval = null
  }
  if (rapidPollTimer) {
    clearInterval(rapidPollTimer)
    rapidPollTimer = null
  }
}

export function useP2P() {
  return {
    status: readonly(status),
    lastError: readonly(lastError),
    refreshStatus,
    start,
    stop,
    peers,
    startPolling,
    stopPolling,
  }
}
