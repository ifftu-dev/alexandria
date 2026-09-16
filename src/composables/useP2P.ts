import { ref, readonly } from 'vue'
import type { P2PStatus, PeerInfo } from '@/types'
import { useLocalApi } from './useLocalApi'
import { onProfileLocked } from './useProfiles'

const { invoke } = useLocalApi()

// Module-level singleton
const status = ref<P2PStatus | null>(null)
const lastError = ref<string | null>(null)
const polling = ref(false)
let pollInterval: ReturnType<typeof setInterval> | null = null
let rapidPollTimer: ReturnType<typeof setInterval> | null = null
let rapidPollDeadline: ReturnType<typeof setTimeout> | null = null
let profileGeneration = 0

async function refreshStatus(): Promise<void> {
  const generation = profileGeneration
  try {
    const refreshed = await invoke<P2PStatus>('p2p_status')
    if (generation === profileGeneration) {
      status.value = refreshed
      lastError.value = null
    }
  } catch (e: unknown) {
    if (generation === profileGeneration) {
      lastError.value = e instanceof Error ? e.message : String(e)
      status.value = null
    }
  }
}

async function start(): Promise<void> {
  const generation = profileGeneration
  // Skip if we already know the node is running
  if (status.value?.is_running) return
  try {
    await invoke('p2p_start')
    if (generation !== profileGeneration) return
    // (Re)publish this profile's username claim — kad records expire,
    // and claims made while offline defer their DHT publish to here.
    void invoke('claim_username').catch(() => {})
  } catch {
    // Fire-and-forget: the backend spawns startup in the background.
    // Any error here (e.g. wallet locked) is non-fatal.
  }
  if (generation === profileGeneration) await refreshStatus()
}

async function stop(): Promise<void> {
  const generation = profileGeneration
  await invoke('p2p_stop')
  if (generation === profileGeneration) await refreshStatus()
}

async function peers(): Promise<PeerInfo[]> {
  const generation = profileGeneration
  const connected = await invoke<string[]>('p2p_peers')
  return generation === profileGeneration ? connected : []
}

function startPolling(intervalMs = 10000) {
  if (polling.value) return
  polling.value = true
  const generation = profileGeneration

  // Immediate first check
  void refreshStatus()

  // Rapid polling every 2s for the first 30s so we pick up the
  // P2P node coming online quickly after auto-start.
  rapidPollTimer = setInterval(async () => {
    await refreshStatus()
    if (generation !== profileGeneration) return
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
  rapidPollDeadline = setTimeout(() => {
    if (generation !== profileGeneration) return
    if (rapidPollTimer) {
      clearInterval(rapidPollTimer)
      rapidPollTimer = null
    }
    rapidPollDeadline = null
  }, 30000)

  // Normal interval for ongoing status updates
  pollInterval = setInterval(() => void refreshStatus(), intervalMs)
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
  if (rapidPollDeadline) {
    clearTimeout(rapidPollDeadline)
    rapidPollDeadline = null
  }
}

onProfileLocked(() => {
  profileGeneration += 1
  stopPolling()
  status.value = null
  lastError.value = null
})

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
