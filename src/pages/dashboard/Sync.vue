<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { useLocalApi } from '@/composables/useLocalApi'
import { AppSpinner, EmptyState, AppButton, StatusBadge, DataRow } from '@/components/ui'
import type { SyncStatus, DeviceInfo, SyncHistoryEntry, SyncResult } from '@/types'

const { invoke } = useLocalApi()

const status = ref<SyncStatus | null>(null)
const device = ref<DeviceInfo | null>(null)
const history = ref<SyncHistoryEntry[]>([])
const loading = ref(true)
const syncing = ref(false)
const syncResult = ref<SyncResult | null>(null)

onMounted(async () => {
  try {
    const [s, d, h] = await Promise.all([
      invoke<SyncStatus>('sync_status'),
      invoke<DeviceInfo>('sync_get_device_info'),
      invoke<SyncHistoryEntry[]>('sync_history'),
    ])
    status.value = s
    device.value = d
    history.value = h
  } catch (e) {
    console.error('Failed to load sync data:', e)
  } finally {
    loading.value = false
  }
})

async function syncNow() {
  syncing.value = true
  syncResult.value = null
  try {
    syncResult.value = await invoke<SyncResult>('sync_now')
    status.value = await invoke<SyncStatus>('sync_status')
    history.value = await invoke<SyncHistoryEntry[]>('sync_history')
  } catch (e) {
    console.error('Sync failed:', e)
  } finally {
    syncing.value = false
  }
}

async function toggleAutoSync() {
  if (!status.value) return
  try {
    await invoke('sync_set_auto', { enabled: !status.value.auto_sync })
    status.value = await invoke<SyncStatus>('sync_status')
  } catch (e) {
    console.error('Failed to toggle auto-sync:', e)
  }
}
</script>

<template>
  <div>
    <div class="flex items-center justify-between mb-6">
      <div>
        <h1 class="text-xl font-bold">Cross-Device Sync</h1>
        <p class="text-sm text-[rgb(var(--color-muted-foreground))]">
          Sync your data across devices paired with the same recovery phrase.
        </p>
      </div>
      <AppButton :loading="syncing" @click="syncNow">
        Sync Now
      </AppButton>
    </div>

    <AppSpinner v-if="loading" label="Loading sync status..." />

    <template v-else>
      <!-- Sync result -->
      <div v-if="syncResult" class="alert alert-success mb-6">
        Synced {{ syncResult.rows_merged }} rows in {{ syncResult.duration_ms }}ms
        (sent {{ syncResult.rows_sent }}, received {{ syncResult.rows_received }})
      </div>

      <!-- Status -->
      <div class="card p-5 mb-6">
        <h2 class="text-base font-semibold mb-3">Status</h2>
        <div class="space-y-2">
          <DataRow label="This Device" mono>{{ device?.id ?? '...' }}</DataRow>
          <DataRow v-if="device?.device_name" label="Name">{{ device.device_name }}</DataRow>
          <DataRow label="Platform">{{ device?.platform ?? 'Unknown' }}</DataRow>
          <DataRow label="Devices">{{ status?.device_count ?? 0 }}</DataRow>
          <DataRow label="Queue">{{ status?.queue_length ?? 0 }} pending</DataRow>
          <DataRow label="Auto-sync">
            <AppButton variant="ghost" size="xs" @click="toggleAutoSync">
              {{ status?.auto_sync ? 'On' : 'Off' }}
            </AppButton>
          </DataRow>
          <DataRow v-if="status?.last_sync" label="Last sync">{{ status.last_sync }}</DataRow>
        </div>
      </div>

      <!-- Paired devices -->
      <div class="card p-5 mb-6">
        <h2 class="text-base font-semibold mb-3">Paired Devices</h2>
        <EmptyState
          v-if="!status?.devices?.length"
          title="No other devices"
          description="Import the same recovery phrase on another device to pair."
        />
        <div v-else class="space-y-2">
          <div
            v-for="d in status.devices"
            :key="d.device_id"
            class="flex items-center justify-between p-3 rounded bg-[rgb(var(--color-muted)/0.3)]"
          >
            <div>
              <div class="text-sm font-medium">{{ d.device_name ?? d.device_id }}</div>
              <div class="text-xs text-[rgb(var(--color-muted-foreground))]">
                {{ d.tables_synced }} tables synced
              </div>
            </div>
            <StatusBadge :status="d.is_online ? 'online' : 'offline'" />
          </div>
        </div>
      </div>

      <!-- Sync history -->
      <div class="card p-5">
        <h2 class="text-base font-semibold mb-3">History</h2>
        <EmptyState
          v-if="history.length === 0"
          title="No sync history"
          description="Sync history will appear after your first sync."
        />
        <div v-else class="space-y-2">
          <div
            v-for="(entry, i) in history.slice(0, 20)"
            :key="i"
            class="flex items-center justify-between text-sm p-2 rounded bg-[rgb(var(--color-muted)/0.2)]"
          >
            <div>
              <span class="font-medium">{{ entry.device_name ?? entry.device_id }}</span>
              <span class="text-xs text-[rgb(var(--color-muted-foreground))] ml-2">{{ entry.direction }}</span>
            </div>
            <div class="text-xs text-[rgb(var(--color-muted-foreground))]">
              {{ entry.rows_sent }}↑ {{ entry.rows_received }}↓ &middot; {{ entry.synced_at }}
            </div>
          </div>
        </div>
      </div>
    </template>
  </div>
</template>
