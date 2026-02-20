<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { useP2P } from '@/composables/useP2P'
import { AppButton, AppSpinner, EmptyState, StatusBadge, DataRow } from '@/components/ui'
import type { PeerInfo } from '@/types'

const { status: p2pStatus, start, stop, peers: fetchPeers, refreshStatus } = useP2P()

const peerList = ref<PeerInfo[]>([])
const loading = ref(true)
const toggling = ref(false)

onMounted(async () => {
  await refreshStatus()
  if (p2pStatus.value?.running) {
    try {
      peerList.value = await fetchPeers()
    } catch { /* no peers */ }
  }
  loading.value = false
})

async function toggle() {
  toggling.value = true
  try {
    if (p2pStatus.value?.running) {
      await stop()
      peerList.value = []
    } else {
      await start()
      peerList.value = await fetchPeers().catch(() => [])
    }
  } catch (e) {
    console.error('Failed to toggle P2P:', e)
  } finally {
    toggling.value = false
  }
}

async function refreshPeers() {
  peerList.value = await fetchPeers().catch(() => [])
}
</script>

<template>
  <div>
    <div class="flex items-center justify-between mb-6">
      <div>
        <h1 class="text-xl font-bold">P2P Network</h1>
        <p class="text-sm text-[rgb(var(--color-muted-foreground))]">
          Manage your node's peer-to-peer network connection.
        </p>
      </div>
      <AppButton
        :variant="p2pStatus?.running ? 'outline' : 'primary'"
        :loading="toggling"
        @click="toggle"
      >
        {{ p2pStatus?.running ? 'Stop' : 'Start' }}
      </AppButton>
    </div>

    <AppSpinner v-if="loading" label="Loading network status..." />

    <template v-else>
      <!-- Status card -->
      <div class="card p-5 mb-6">
        <h2 class="text-base font-semibold mb-3">Node Status</h2>
        <div class="space-y-2">
          <DataRow label="Status">
            <StatusBadge :status="p2pStatus?.running ? 'online' : 'offline'" />
          </DataRow>
          <DataRow v-if="p2pStatus?.peer_id" label="Peer ID" mono>{{ p2pStatus.peer_id }}</DataRow>
          <DataRow label="Connected Peers">{{ p2pStatus?.connected_peers ?? 0 }}</DataRow>
          <DataRow v-if="p2pStatus?.gossipsub_topics?.length" label="Topics">
            {{ p2pStatus.gossipsub_topics.join(', ') }}
          </DataRow>
        </div>

        <div v-if="p2pStatus?.listening_addresses?.length" class="mt-3 pt-3 border-t border-[rgb(var(--color-border))]">
          <div class="text-xs font-medium text-[rgb(var(--color-muted-foreground))] mb-1">Listening Addresses</div>
          <div v-for="addr in p2pStatus.listening_addresses" :key="addr" class="text-xs font-mono text-[rgb(var(--color-muted-foreground))] break-all">
            {{ addr }}
          </div>
        </div>
      </div>

      <!-- Peers -->
      <div class="card p-5">
        <div class="flex items-center justify-between mb-3">
          <h2 class="text-base font-semibold">Connected Peers</h2>
          <AppButton variant="ghost" size="sm" @click="refreshPeers">
            Refresh
          </AppButton>
        </div>

        <EmptyState
          v-if="peerList.length === 0"
          title="No peers connected"
          :description="p2pStatus?.running ? 'Waiting for peer discovery via mDNS and DHT...' : 'Start the P2P network to discover peers.'"
        />

        <div v-else class="space-y-2">
          <div v-for="peer in peerList" :key="peer.peer_id" class="p-3 rounded bg-[rgb(var(--color-muted)/0.3)]">
            <div class="text-sm font-mono break-all">{{ peer.peer_id }}</div>
            <div v-if="peer.addresses.length" class="text-xs text-[rgb(var(--color-muted-foreground))] mt-1">
              {{ peer.addresses[0] }}
              <span v-if="peer.addresses.length > 1"> +{{ peer.addresses.length - 1 }} more</span>
            </div>
          </div>
        </div>
      </div>
    </template>
  </div>
</template>
