<script setup lang="ts">
// Manage user-added (community) relays. These extend the built-in
// operator relays for connectivity only — circuit relay + DHT. They do
// NOT gain username-receipt trust (that stays with the on-chain
// registry), so adding a relay here is safe: a malicious one can carry
// traffic but cannot forge handle ownership.
import { ref, onMounted } from 'vue'
import { useI18n } from 'vue-i18n'
import { invoke } from '@tauri-apps/api/core'
import { AppButton, AppInput } from '@/components/ui'

const { t } = useI18n()

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
    status.value = t('settings.relays.loadFailed', { msg: String(e) })
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
    status.value = t('settings.relays.saved')
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
      <p class="text-sm font-medium text-foreground">{{ $t('settings.relays.title') }}</p>
      <AppButton variant="outline" size="sm" @click="addRow">{{ $t('settings.relays.add') }}</AppButton>
    </div>
    <p class="text-xs text-muted-foreground mb-3">
      {{ $t('settings.relays.hint') }}
    </p>

    <p v-if="loading" class="text-xs text-muted-foreground">{{ $t('settings.relays.loading') }}</p>

    <p v-else-if="rows.length === 0" class="text-xs text-muted-foreground italic">
      {{ $t('settings.relays.empty') }}
    </p>

    <div v-else class="space-y-2">
      <div
        v-for="(row, i) in rows"
        :key="i"
        class="flex flex-wrap items-center gap-2"
      >
        <AppInput
          v-model="row.peer_id"
          :placeholder="$t('settings.relays.deviceIdPlaceholder')"
          class="flex-1 min-w-[16rem] font-mono text-xs"
        />
        <AppInput v-model="row.host" :placeholder="$t('settings.relays.hostPlaceholder')" class="w-40" />
        <AppInput v-model="row.port" type="number" :placeholder="$t('settings.relays.portPlaceholder')" class="w-20" />
        <AppButton variant="ghost" size="sm" @click="removeRow(i)">{{ $t('settings.relays.remove') }}</AppButton>
      </div>
    </div>

    <div class="flex items-center gap-3 mt-3">
      <AppButton size="sm" :disabled="saving || loading" @click="save">
        {{ saving ? $t('settings.relays.saving') : $t('settings.relays.save') }}
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
