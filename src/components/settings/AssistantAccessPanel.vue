<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useLocalApi } from '@/composables/useLocalApi'
import { AppButton, AppInput } from '@/components/ui'
import type { StudioAssistantAccess, StudioAssistantConnection } from '@/types'

const { t } = useI18n()
const { invoke } = useLocalApi()
const access = ref<StudioAssistantAccess | null>(null)
const name = ref('')
const learning = ref(true)
const drafts = ref(false)
const propose = ref(false)
const connection = ref<StudioAssistantConnection | null>(null)
const saving = ref(false)
const error = ref('')

// Proposing a draft needs reading it first.
watch(propose, (on) => { if (on) drafts.value = true })

const scopes = computed(() => [
  ...(learning.value ? ['learning:read'] : []),
  ...(drafts.value ? ['drafts:read'] : []),
  ...(propose.value ? ['drafts:propose'] : []),
])

async function load() {
  try {
    access.value = await invoke<StudioAssistantAccess>('studio_assistant_access')
  } catch (e) { error.value = String(e) }
}

async function grant() {
  saving.value = true; error.value = ''
  try {
    connection.value = await invoke<StudioAssistantConnection>('studio_grant_assistant', { clientName: name.value, scopes: scopes.value })
    name.value = ''; learning.value = true; drafts.value = false; propose.value = false
    await load()
  } catch (e) { error.value = String(e) } finally { saving.value = false }
}

async function revoke(id: string) {
  saving.value = true; error.value = ''
  try {
    await invoke('studio_revoke_assistant', { grantId: id })
    if (connection.value?.grant.id === id) connection.value = null
    await load()
  } catch (e) { error.value = String(e) } finally { saving.value = false }
}

function scopeLabel(scope: string) {
  if (scope === 'learning:read') return t('settings.assistants.scopeLearning')
  if (scope === 'drafts:propose') return t('settings.assistants.scopePropose')
  return t('settings.assistants.scopeDrafts')
}

onMounted(load)
</script>

<template>
  <section class="space-y-5">
    <div>
      <h2 class="font-semibold">{{ t('settings.assistants.title') }}</h2>
      <p class="mt-2 text-sm text-muted-foreground">{{ t('settings.assistants.intro') }}</p>
    </div>
    <p v-if="error" role="alert" class="text-sm text-error">{{ error }}</p>
    <p v-if="access && !access.available" role="status" class="rounded-xl border border-border p-5 text-sm text-muted-foreground">{{ t('settings.assistants.unavailable') }}</p>
    <form v-else-if="access" class="space-y-4 rounded-xl border border-border bg-card p-5" @submit.prevent="grant">
      <AppInput v-model="name" :label="t('settings.assistants.name')" required :maxlength="100" />
      <fieldset class="space-y-3">
        <legend class="mb-2 text-sm font-medium">{{ t('settings.assistants.permissions') }}</legend>
        <label class="flex items-start gap-2 text-sm"><input v-model="learning" type="checkbox" class="mt-1">{{ t('settings.assistants.learningScope') }}</label>
        <label class="flex items-start gap-2 text-sm"><input v-model="drafts" type="checkbox" class="mt-1" :disabled="propose">{{ t('settings.assistants.draftsScope') }}</label>
        <label class="flex items-start gap-2 text-sm"><input v-model="propose" type="checkbox" class="mt-1">{{ t('settings.assistants.proposeScope') }}</label>
      </fieldset>
      <p class="text-xs text-muted-foreground">{{ t('settings.assistants.expiry') }}</p>
      <AppButton type="submit" :loading="saving" :disabled="!name.trim() || !scopes.length">{{ t('settings.assistants.grant') }}</AppButton>
    </form>
    <div v-if="connection" class="space-y-3 rounded-xl border border-primary/30 bg-primary/5 p-5" role="status">
      <h3 class="font-semibold">{{ t('settings.assistants.ready') }}</h3>
      <p class="text-sm">{{ t('settings.assistants.setup') }}</p>
      <pre class="overflow-x-auto rounded-lg bg-background p-3 text-xs">{{ JSON.stringify({ command: 'alexandria-mcp', env: { ALEXANDRIA_MCP_CONNECTION_FILE: connection.connection_file } }, null, 2) }}</pre>
      <p class="text-xs text-muted-foreground">{{ t('settings.assistants.setupHelp') }}</p>
    </div>
    <div class="divide-y divide-border rounded-xl border border-border bg-card">
      <div v-for="item in access?.grants" :key="item.id" class="flex flex-wrap items-center gap-4 p-5">
        <div class="min-w-0 flex-1">
          <h3 class="font-semibold">{{ item.client_name }}</h3>
          <p class="mt-1 text-xs text-muted-foreground">{{ item.scopes.map(scopeLabel).join(' · ') }} · {{ t('settings.assistants.expiresAt', { time: new Date(item.expires_at * 1000).toLocaleTimeString() }) }}</p>
        </div>
        <AppButton type="button" variant="secondary" :loading="saving" @click="revoke(item.id)">{{ t('settings.assistants.revoke') }}</AppButton>
      </div>
      <p v-if="!access?.grants.length" class="p-5 text-sm text-muted-foreground">{{ t('settings.assistants.none') }}</p>
    </div>
  </section>
</template>
