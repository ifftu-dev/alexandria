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
const privateSkills = ref(false)
const credentials = ref(false)
const verify = ref(false)
const drafts = ref(false)
const propose = ref(false)
const connection = ref<StudioAssistantConnection | null>(null)
const saving = ref(false)
const error = ref('')

// Proposing a draft needs reading it first.
watch(propose, (on) => { if (on) drafts.value = true })
// Private skills are part of the learning scope.
watch(learning, (on) => { if (!on) privateSkills.value = false })

const scopes = computed(() => [
  ...(learning.value ? ['learning:read'] : []),
  ...(learning.value && privateSkills.value ? ['learning:private_skills'] : []),
  ...(credentials.value ? ['credentials:read'] : []),
  ...(verify.value ? ['credentials:verify'] : []),
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
    name.value = ''; learning.value = true; privateSkills.value = false; credentials.value = false; verify.value = false; drafts.value = false; propose.value = false
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
  if (scope === 'learning:private_skills') return t('settings.assistants.scopePrivateSkills')
  if (scope === 'credentials:read') return t('settings.assistants.scopeCredentials')
  if (scope === 'credentials:verify') return t('settings.assistants.scopeVerify')
  if (scope === 'drafts:propose') return t('settings.assistants.scopePropose')
  return t('settings.assistants.scopeDrafts')
}

function outcomeLabel(outcome: string) {
  if (outcome === 'returned') return t('settings.assistants.outcomeReturned')
  if (outcome === 'withheld') return t('settings.assistants.outcomeWithheld')
  if (outcome === 'unavailable') return t('settings.assistants.outcomeUnavailable')
  if (outcome === 'assessment_active') return t('settings.assistants.outcomePaused')
  return t('settings.assistants.outcomeRefused')
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
      <div class="rounded-lg border border-warning/40 bg-warning/10 p-3 text-sm" role="note">
        <p class="font-semibold">{{ t('settings.assistants.disclosureTitle') }}</p>
        <p class="mt-1 text-muted-foreground">{{ t('settings.assistants.disclosureBody') }}</p>
      </div>
      <AppInput v-model="name" :label="t('settings.assistants.name')" required :maxlength="100" />
      <fieldset class="space-y-3">
        <legend class="mb-2 text-sm font-medium">{{ t('settings.assistants.permissions') }}</legend>
        <label class="flex items-start gap-2 text-sm"><input v-model="learning" type="checkbox" class="mt-1">{{ t('settings.assistants.learningContent') }}</label>
        <label class="ms-6 flex items-start gap-2 text-sm" :class="{ 'opacity-60': !learning }"><input v-model="privateSkills" type="checkbox" class="mt-1" :disabled="!learning">{{ t('settings.assistants.privateSkillsScope') }}</label>
        <label class="flex items-start gap-2 text-sm"><input v-model="credentials" type="checkbox" class="mt-1">{{ t('settings.assistants.credentialsScope') }}</label>
        <label class="flex items-start gap-2 text-sm"><input v-model="verify" type="checkbox" class="mt-1">{{ t('settings.assistants.verifyScope') }}</label>
        <label class="flex items-start gap-2 text-sm"><input v-model="drafts" type="checkbox" class="mt-1" :disabled="propose">{{ t('settings.assistants.draftsScope') }}</label>
        <label class="flex items-start gap-2 text-sm"><input v-model="propose" type="checkbox" class="mt-1">{{ t('settings.assistants.proposeScope') }}</label>
      </fieldset>
      <p class="text-xs text-muted-foreground">{{ t('settings.assistants.expiry') }}</p>
      <p class="text-xs text-muted-foreground">{{ t('settings.assistants.pauses') }}</p>
      <AppButton type="submit" :loading="saving" :disabled="!name.trim() || !scopes.length">{{ t('settings.assistants.grant') }}</AppButton>
    </form>
    <div v-if="connection" class="space-y-3 rounded-xl border border-primary/30 bg-primary/5 p-5" role="status">
      <h3 class="font-semibold">{{ t('settings.assistants.ready') }}</h3>
      <p class="text-sm">{{ t('settings.assistants.setup') }}</p>
      <pre class="overflow-x-auto rounded-lg bg-background p-3 text-xs">{{ JSON.stringify({ command: 'alexandria-mcp', env: { ALEXANDRIA_MCP_CONNECTION_FILE: connection.connection_file } }, null, 2) }}</pre>
      <p class="text-xs text-muted-foreground">{{ t('settings.assistants.setupHelp') }}</p>
    </div>
    <div class="divide-y divide-border rounded-xl border border-border bg-card">
      <div v-for="item in access?.grants" :key="item.id" class="p-5">
        <div class="flex flex-wrap items-center gap-4">
          <div class="min-w-0 flex-1">
            <h3 class="font-semibold">{{ item.client_name }}</h3>
            <p class="mt-1 text-xs text-muted-foreground">{{ item.scopes.map(scopeLabel).join(' · ') }} · {{ t('settings.assistants.expiresAt', { time: new Date(item.expires_at * 1000).toLocaleTimeString() }) }}</p>
          </div>
          <AppButton type="button" variant="secondary" :loading="saving" @click="revoke(item.id)">{{ t('settings.assistants.revoke') }}</AppButton>
        </div>
        <details class="mt-3 text-sm" @toggle="load">
          <summary class="cursor-pointer text-xs font-medium text-muted-foreground">{{ t('settings.assistants.activity', { count: access?.activity[item.id]?.length ?? 0 }) }}</summary>
          <p v-if="!access?.activity[item.id]?.length" class="mt-2 text-xs text-muted-foreground">{{ t('settings.assistants.activityEmpty') }}</p>
          <table v-else class="mt-2 w-full text-xs">
            <tbody>
              <tr v-for="(entry, index) in access?.activity[item.id]" :key="index" class="border-t border-border">
                <td class="py-1.5 pe-3 text-muted-foreground">{{ new Date(entry.at * 1000).toLocaleTimeString() }}</td>
                <td class="py-1.5 pe-3"><code class="font-mono">{{ entry.tool }}</code> <span class="text-muted-foreground">{{ entry.detail }}</span></td>
                <td class="py-1.5" :class="entry.outcome === 'returned' ? 'text-success' : entry.outcome === 'assessment_active' ? 'text-error' : 'text-muted-foreground'">{{ outcomeLabel(entry.outcome) }}</td>
              </tr>
            </tbody>
          </table>
        </details>
      </div>
      <p v-if="!access?.grants.length" class="p-5 text-sm text-muted-foreground">{{ t('settings.assistants.none') }}</p>
    </div>
  </section>
</template>
