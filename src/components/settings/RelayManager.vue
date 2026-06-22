<script setup lang="ts">
// Manage user-added (community) relays. These extend the built-in
// operator relays for connectivity only — circuit relay + DHT. They do
// NOT gain username-receipt trust (that stays with the on-chain
// registry), so adding a relay here is safe: a malicious one can carry
// traffic but cannot forge handle ownership.
import { ref, onMounted } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { AppButton, AppInput } from '@/components/ui'

interface ExtraRelay {
  peer_id: string
  host: string
  port: number
}
interface Row {
  peer_id: string
  host: string
  port: string
}

const rows = ref<Row[]>([])
const status = ref('')
const error = ref(false)
const loading = ref(true)
const saving = ref(false)

onMounted(async () => {
  try {
    const relays = await invoke<ExtraRelay[]>('get_extra_relays')
    rows.value = relays.map(r => ({ peer_id: r.peer_id, host: r.host, port: String(r.port) }))
  } catch (e) {
    error.value = true
    status.value = `Failed to load relays: ${e}`
  } finally {
    loading.value = false
  }
})

function addRow() {
  rows.value.push({ peer_id: '', host: '', port: '4001' })
  status.value = ''
}

function removeRow(i: number) {
  rows.value.splice(i, 1)
  status.value = ''
}

async function save() {
  saving.value = true
  status.value = ''
  error.value = false
  const relays: ExtraRelay[] = rows.value.map(r => ({
    peer_id: r.peer_id.trim(),
    host: r.host.trim(),
    port: Number(r.port),
  }))
  try {
    await invoke('save_extra_relays', { relays })
    status.value = 'Saved. New relays take effect on the next node start.'
  } catch (e) {
    error.value = true
    status.value = `${e}`
  } finally {
    saving.value = false
  }
}
</script>

<template>
  <div class="rounded-lg border border-border p-4">
    <div class="flex items-center justify-between gap-4 mb-1">
      <p class="text-sm font-medium text-foreground">Community relays</p>
      <AppButton variant="outline" size="sm" @click="addRow">+ Add relay</AppButton>
    </div>
    <p class="text-xs text-muted-foreground mb-3">
      Extra relays to bootstrap and route through. They help connectivity (NAT
      traversal + directory) only — they cannot vouch for usernames, so adding
      one is safe.
    </p>

    <p v-if="loading" class="text-xs text-muted-foreground">Loading…</p>

    <p v-else-if="rows.length === 0" class="text-xs text-muted-foreground italic">
      No community relays added. The built-in operator relays are always used.
    </p>

    <div v-else class="space-y-2">
      <div
        v-for="(row, i) in rows"
        :key="i"
        class="flex flex-wrap items-center gap-2"
      >
        <AppInput
          v-model="row.peer_id"
          placeholder="Peer ID (12D3KooW…)"
          class="flex-1 min-w-[16rem] font-mono text-xs"
        />
        <AppInput v-model="row.host" placeholder="host (dns or ip)" class="w-40" />
        <AppInput v-model="row.port" type="number" placeholder="port" class="w-20" />
        <AppButton variant="ghost" size="sm" @click="removeRow(i)">Remove</AppButton>
      </div>
    </div>

    <div class="flex items-center gap-3 mt-3">
      <AppButton size="sm" :disabled="saving || loading" @click="save">
        {{ saving ? 'Saving…' : 'Save relays' }}
      </AppButton>
      <span
        v-if="status"
        class="text-xs"
        :class="error ? 'text-destructive' : 'text-muted-foreground'"
      >
        {{ status }}
      </span>
    </div>
  </div>
</template>
