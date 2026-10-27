<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { AppButton } from '@/components/ui'
import { useLocalApi } from '@/composables/useLocalApi'
import { useGuardian } from '@/composables/useGuardian'
import type { GuardianAiRequest } from '@/types'

/**
 * A ward's requests to use remote AI models. Approving lets the ward send
 * chat messages to that provider at that address; the decision reaches the
 * ward on the next guardian sync, and can be withdrawn later.
 */
const props = defineProps<{ linkId: string }>()
const { t } = useI18n()
const { invoke } = useLocalApi()
const { syncNow } = useGuardian()
const requests = ref<GuardianAiRequest[]>([])
const busy = ref<string | null>(null)
const error = ref('')

const mine = computed(() => requests.value.filter((r) => r.link_id === props.linkId))

async function load() {
  try {
    requests.value = await invoke<GuardianAiRequest[]>('guardian_list_ai_requests')
  } catch (e) {
    error.value = String(e)
  }
}

async function decide(request: GuardianAiRequest, approve: boolean) {
  busy.value = request.id
  error.value = ''
  try {
    await invoke('guardian_decide_ai_request', { requestId: request.id, linkId: request.link_id, approve })
    await load()
    // Deliver now if the child is reachable; otherwise the next sync will.
    void syncNow().then(load).catch(() => {})
  } catch (e) {
    error.value = String(e)
  } finally {
    busy.value = null
  }
}

function host(endpoint: string) {
  try {
    return new URL(endpoint).host
  } catch {
    return endpoint
  }
}

onMounted(load)
</script>

<template>
  <div class="rounded-xl border border-border bg-card p-5">
    <h2 class="text-sm font-semibold text-foreground">{{ t('guardian.aiRequests.title') }}</h2>
    <p class="mt-1 text-xs text-muted-foreground">{{ t('guardian.aiRequests.intro') }}</p>
    <p v-if="error" class="mt-2 text-xs text-error" role="alert">{{ error }}</p>
    <p v-if="!mine.length" class="mt-3 text-sm text-muted-foreground">{{ t('guardian.aiRequests.none') }}</p>
    <ul v-else class="mt-3 space-y-2">
      <li v-for="r in mine" :key="r.id" class="flex flex-wrap items-center gap-3 rounded-lg border border-border p-3 text-sm">
        <div class="min-w-0 flex-1">
          <p class="font-medium text-foreground">{{ r.connection_name }}</p>
          <p class="text-xs text-muted-foreground">{{ host(r.endpoint) }} · {{ r.model }}</p>
          <p class="mt-0.5 text-xs" :class="r.status === 'approved' ? 'text-success' : r.status === 'declined' ? 'text-muted-foreground' : 'text-warning'">
            {{ t(`guardian.aiRequests.status.${r.status}`) }}<template v-if="r.status !== 'pending' && !r.delivered"> · {{ t('guardian.aiRequests.notDelivered') }}</template>
          </p>
        </div>
        <template v-if="r.status === 'pending'">
          <AppButton size="sm" :loading="busy === r.id" @click="decide(r, true)">{{ t('guardian.aiRequests.approve') }}</AppButton>
          <AppButton size="sm" variant="outline" :disabled="busy === r.id" @click="decide(r, false)">{{ t('guardian.aiRequests.decline') }}</AppButton>
        </template>
        <AppButton v-else-if="r.status === 'approved'" size="sm" variant="outline" :loading="busy === r.id" @click="decide(r, false)">{{ t('guardian.aiRequests.withdraw') }}</AppButton>
        <AppButton v-else size="sm" variant="ghost" :loading="busy === r.id" @click="decide(r, true)">{{ t('guardian.aiRequests.approve') }}</AppButton>
      </li>
    </ul>
  </div>
</template>
