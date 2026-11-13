<script setup lang="ts">
// Sponsor-side choice of the model connection that writes a role's
// generated follow-up probes (`llm` probe policy). The model is the user's
// own saved connection, shown by the name they gave it; an empty value
// turns generated follow-ups off.
import { onMounted, ref } from 'vue'
import { useLocalApi } from '@/composables/useLocalApi'
import type { StudioConnection, StudioDocument } from '@/types'

const model = defineModel<string>({ required: true })

const { invoke } = useLocalApi()
const connections = ref<StudioConnection[]>([])

onMounted(async () => {
  try {
    const docs = await invoke<StudioDocument<StudioConnection>[]>('studio_list_connections')
    connections.value = docs.map(d => d.value).filter(c => c.enabled && c.capability === 'text')
  } catch (e) {
    console.warn('Model connections are unavailable', e)
  }
})
</script>

<template>
  <div class="space-y-1" data-testid="generated-probe-connection">
    <label class="text-xs text-muted-foreground">{{ $t('dashboard.sponsor.roleModal.generatedProbeLabel') }}
      <select v-model="model" class="mt-1 w-full rounded-md border border-border bg-background p-2 text-sm">
        <option value="">{{ $t('dashboard.sponsor.roleModal.generatedProbeOff') }}</option>
        <option v-for="c in connections" :key="c.id" :value="c.id">{{ c.name }}</option>
      </select>
    </label>
    <p v-if="connections.length === 0" class="text-xs text-muted-foreground">{{ $t('dashboard.sponsor.roleModal.generatedProbeNone') }}</p>
    <p class="text-xs text-muted-foreground">{{ $t('dashboard.sponsor.roleModal.generatedProbeNote') }}</p>
  </div>
</template>
