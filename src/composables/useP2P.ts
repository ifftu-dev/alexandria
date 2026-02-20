import { ref, readonly } from 'vue'
import type { P2PStatus, PeerInfo } from '@/types'
import { useLocalApi } from './useLocalApi'

const { invoke } = useLocalApi()

// Module-level singleton
const status = ref<P2PStatus | null>(null)
const polling = ref(false)
let pollInterval: ReturnType<typeof setInterval> | null = null

async function refreshStatus(): Promise<void> {
  try {
    status.value = await invoke<P2PStatus>('p2p_status')
  } catch {
    status.value = null
  }
}

async function start(): Promise<void> {
  await invoke('p2p_start')
  await refreshStatus()
}

async function stop(): Promise<void> {
  await invoke('p2p_stop')
  await refreshStatus()
}

async function peers(): Promise<PeerInfo[]> {
  return invoke<PeerInfo[]>('p2p_peers')
}

function startPolling(intervalMs = 10000) {
  if (polling.value) return
  polling.value = true
  refreshStatus()
  pollInterval = setInterval(refreshStatus, intervalMs)
}

function stopPolling() {
  polling.value = false
  if (pollInterval) {
    clearInterval(pollInterval)
    pollInterval = null
  }
}

export function useP2P() {
  return {
    status: readonly(status),
    refreshStatus,
    start,
    stop,
    peers,
    startPolling,
    stopPolling,
  }
}
