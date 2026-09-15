<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { onBeforeRouteLeave } from 'vue-router'
import { useI18n } from 'vue-i18n'
import { useLocalApi } from '@/composables/useLocalApi'
import { useStudio } from '@/composables/useStudio'
import { AppButton, AppInput, AppTextarea, AppTabs, ConfirmDialog } from '@/components/ui'
import type { StudioConnection, StudioDocument, StudioSettings, StudioAssistantAccess, StudioAssistantConnection } from '@/types'

const { t } = useI18n()
const { invoke } = useLocalApi()
const studio = useStudio()
const tab = ref('roles')
const assistantAccess = ref<StudioAssistantAccess | null>(null)
const assistantName = ref('')
const allowProposals = ref(false)
const assistantConnection = ref<StudioAssistantConnection | null>(null)
async function loadAssistantAccess() {
  assistantAccess.value = await invoke<StudioAssistantAccess>('studio_assistant_access')
}
async function grantAssistant() {
  saving.value = true; error.value = ''
  try {
    assistantConnection.value = await invoke<StudioAssistantConnection>('studio_grant_assistant', { clientName: assistantName.value, scopes: allowProposals.value ? ['drafts:read', 'drafts:propose'] : ['drafts:read'] })
    assistantName.value = ''; allowProposals.value = false
    await loadAssistantAccess()
  } catch (e) { error.value = String(e) } finally { saving.value = false }
}
async function revokeAssistant(id: string) {
  saving.value = true; error.value = ''
  try {
    await invoke('studio_revoke_assistant', { grantId: id })
    if (assistantConnection.value?.grant.id === id) assistantConnection.value = null
    await loadAssistantAccess()
  } catch (e) { error.value = String(e) } finally { saving.value = false }
}
const draft = ref<StudioDocument<StudioSettings> | null>(null)
const connection = ref<StudioDocument<StudioConnection> | null>(null)
const apiKey = ref('')
const error = ref('')
const saving = ref(false)
const saved = ref('')
const selectedRole = ref('draft')
const baseline = ref('')
const dirty = computed(() => draft.value !== null && JSON.stringify(draft.value) !== baseline.value)
const currentRole = computed(() => draft.value?.value.roles.find(r => r.role === selectedRole.value))
const leave = ref(false)
let resolveLeave: ((allow: boolean) => void) | null = null
onBeforeRouteLeave(() => {
  if (!dirty.value && !connection.value) return true
  leave.value = true
  return new Promise<boolean>(resolve => { resolveLeave = resolve })
})
function answerLeave(allow: boolean) { leave.value = false; resolveLeave?.(allow); resolveLeave = null }
async function load() {
  try {
    await Promise.all([studio.refresh(), loadAssistantAccess()])
    if (studio.settings.value) draft.value = JSON.parse(JSON.stringify(studio.settings.value)) as StudioDocument<StudioSettings>
    baseline.value = JSON.stringify(draft.value)
  } catch (e) { error.value = String(e) }
}
onMounted(load)
async function saveRoles() {
  if (!draft.value) return
  saving.value = true; error.value = ''
  try {
    draft.value = await invoke<StudioDocument<StudioSettings>>('studio_save_settings', { document: draft.value })
    baseline.value = JSON.stringify(draft.value)
    saved.value = t('instructor.studio.saved')
    await studio.refresh()
  } catch (e) { error.value = String(e) } finally { saving.value = false }
}
function editConnection(value?: StudioDocument<StudioConnection>) {
  const id = crypto.randomUUID()
  connection.value = value ? structuredClone(value) : { id, revision: 0, value: { id, name: '', endpoint: 'http://localhost:11434/v1', model: '', location: 'local', capability: 'text', enabled: true, has_key: false } }
  apiKey.value = ''; saved.value = ''
}
async function saveConnection() {
  if (!connection.value) return
  saving.value = true; error.value = ''
  try {
    await invoke('studio_save_connection', { document: connection.value, apiKey: apiKey.value || null })
    apiKey.value = ''; connection.value = null; saved.value = t('instructor.studio.saved')
    await studio.refresh()
  } catch (e) { error.value = String(e) } finally { saving.value = false }
}
</script>

<template>
  <div class="mx-auto max-w-5xl space-y-6">
    <header><h1 class="text-2xl font-bold">{{ t('instructor.studio.aiSettings') }}</h1><p class="mt-2 text-sm text-muted-foreground">{{ t('instructor.studio.aiIntro') }}</p></header>
    <AppTabs v-model="tab" :tabs="[{ key: 'roles', label: t('instructor.studio.roles') }, { key: 'connections', label: t('instructor.studio.connections') }, { key: 'assistants', label: t('instructor.studio.assistantAccess') }]" />
    <p v-if="error" role="alert" class="text-sm text-error">{{ error }}</p>
    <p v-if="saved" role="status" class="text-sm text-success">{{ saved }}</p>
    <div v-if="tab === 'roles' && draft" class="grid gap-5 md:grid-cols-[220px_minmax(0,1fr)]">
      <nav :aria-label="t('instructor.studio.roles')" class="space-y-1">
        <button v-for="role in draft.value.roles" :key="role.role" type="button" :aria-current="selectedRole === role.role ? 'true' : undefined" class="w-full rounded-lg px-4 py-3 text-start text-sm hover:bg-muted" :class="selectedRole === role.role ? 'bg-primary/10 text-primary font-semibold' : 'text-muted-foreground'" @click="selectedRole = role.role">{{ t(`instructor.studio.roleNames.${role.role}`) }}</button>
      </nav>
      <form v-if="currentRole" class="rounded-xl border border-border bg-card p-6 space-y-5" @submit.prevent="saveRoles">
        <h2 class="text-lg font-semibold">{{ t(`instructor.studio.roleNames.${currentRole.role}`) }}</h2>
        <label class="block text-sm"><span class="mb-2 block">{{ t('instructor.studio.defaultModel') }}</span><select v-model="currentRole.connection_id" class="w-full rounded-lg border border-border bg-background p-2.5"><option :value="null">{{ t('instructor.studio.unassigned') }}</option><option v-for="item in studio.connections.value" :key="item.id" :value="item.id">{{ item.value.name }} · {{ item.value.location }}</option></select></label>
        <AppTextarea v-model="currentRole.initial_prompt" :label="t('instructor.studio.initialPrompt')" :rows="9" :maxlength="16000" />
        <p class="text-xs text-muted-foreground">{{ t('instructor.studio.rolePromptHelp') }}</p>
        <div class="flex flex-wrap items-center gap-3"><AppButton type="submit" :loading="saving" :disabled="!dirty">{{ t('common.actions.save') }}</AppButton><span v-if="dirty" class="text-xs text-warning">{{ t('instructor.studio.unsaved') }}</span></div>
      </form>
    </div>
    <div v-if="tab === 'connections'" class="space-y-5">
      <div class="flex items-center justify-between gap-3"><h2 class="font-semibold">{{ t('instructor.studio.connections') }}</h2><AppButton type="button" :disabled="connection !== null" @click="editConnection()">{{ t('instructor.studio.addConnection') }}</AppButton></div>
      <p class="text-sm text-muted-foreground">{{ t('instructor.studio.compatibility') }}</p>
      <form v-if="connection" class="rounded-xl border border-border bg-card p-6 space-y-4" @submit.prevent="saveConnection">
        <AppInput v-model="connection.value.name" :label="t('instructor.studio.displayName')" required :maxlength="200" />
        <label class="block text-sm"><span class="mb-2 block">{{ t('instructor.studio.location') }}</span><select v-model="connection.value.location" class="w-full rounded-lg border border-border bg-background p-2.5"><option value="local">{{ t('instructor.studio.local') }}</option><option value="cloud">{{ t('instructor.studio.cloud') }}</option></select></label>
        <AppInput v-model="connection.value.endpoint" :label="t('instructor.studio.endpoint')" required />
        <AppInput v-model="connection.value.model" :label="t('instructor.studio.modelId')" required :maxlength="200" />
        <AppInput v-model="apiKey" type="password" :label="t('instructor.studio.apiKey')" autocomplete="new-password" :placeholder="connection.value.has_key ? t('instructor.studio.keepKey') : ''" />
        <p class="text-xs text-muted-foreground">{{ t('instructor.studio.keyPrivacy') }}</p>
        <label class="flex items-center gap-2 text-sm"><input v-model="connection.value.enabled" type="checkbox">{{ t('instructor.studio.enabled') }}</label>
        <div class="flex gap-2"><AppButton type="submit" :loading="saving">{{ t('common.actions.save') }}</AppButton><AppButton type="button" variant="ghost" @click="connection = null; apiKey = ''">{{ t('common.actions.cancel') }}</AppButton></div>
      </form>
      <div class="divide-y divide-border rounded-xl border border-border bg-card">
        <div v-for="item in studio.connections.value" :key="item.id" class="flex flex-wrap items-center gap-4 p-5"><div class="min-w-0 flex-1"><h3 class="font-semibold">{{ item.value.name }}</h3><p class="break-all text-xs text-muted-foreground">{{ item.value.model }} · {{ item.value.location }}</p></div><span class="text-xs text-muted-foreground">{{ t(item.value.enabled ? 'instructor.studio.enabled' : 'instructor.studio.disabled') }}</span><AppButton type="button" variant="secondary" :disabled="connection !== null" @click="editConnection(JSON.parse(JSON.stringify(item)) as StudioDocument<StudioConnection>)">{{ t('common.actions.edit') }}</AppButton></div>
        <p v-if="!studio.connections.value.length" class="p-6 text-sm text-muted-foreground">{{ t('instructor.studio.noConnections') }}</p>
      </div>
    </div>
    <section v-if="tab === 'assistants'" class="space-y-5">
      <div><h2 class="font-semibold">{{ t('instructor.studio.assistantAccess') }}</h2><p class="mt-2 text-sm text-muted-foreground">{{ t('instructor.studio.assistantIntro') }}</p></div>
      <p v-if="assistantAccess && !assistantAccess.available" role="status" class="rounded-xl border border-border p-5 text-sm text-muted-foreground">{{ t('instructor.studio.assistantUnavailable') }}</p>
      <form v-else-if="assistantAccess" class="space-y-4 rounded-xl border border-border bg-card p-5" @submit.prevent="grantAssistant">
        <AppInput v-model="assistantName" :label="t('instructor.studio.assistantName')" required :maxlength="100" />
        <p class="text-sm">{{ t('instructor.studio.assistantReadScope') }}</p>
        <label class="flex items-start gap-2 text-sm"><input v-model="allowProposals" type="checkbox" class="mt-1">{{ t('instructor.studio.assistantProposeScope') }}</label>
        <p class="text-xs text-muted-foreground">{{ t('instructor.studio.assistantExpiry') }}</p>
        <AppButton type="submit" :loading="saving" :disabled="!assistantName.trim()">{{ t('instructor.studio.grantAccess') }}</AppButton>
      </form>
      <div v-if="assistantConnection" class="space-y-3 rounded-xl border border-primary/30 bg-primary/5 p-5" role="status">
        <h3 class="font-semibold">{{ t('instructor.studio.assistantReady') }}</h3>
        <p class="text-sm">{{ t('instructor.studio.assistantSetup') }}</p>
        <pre class="overflow-x-auto rounded-lg bg-background p-3 text-xs">{{ JSON.stringify({ command: 'alexandria-mcp', env: { ALEXANDRIA_MCP_CONNECTION_FILE: assistantConnection.connection_file } }, null, 2) }}</pre>
        <p class="text-xs text-muted-foreground">{{ t('instructor.studio.assistantSetupHelp') }}</p>
      </div>
      <div class="divide-y divide-border rounded-xl border border-border bg-card">
        <div v-for="grant in assistantAccess?.grants" :key="grant.id" class="flex flex-wrap items-center gap-4 p-5"><div class="min-w-0 flex-1"><h3 class="font-semibold">{{ grant.client_name }}</h3><p class="mt-1 text-xs text-muted-foreground">{{ t(grant.scopes.includes('drafts:propose') ? 'instructor.studio.readAndPropose' : 'instructor.studio.readOnly') }} · {{ t('instructor.studio.expiresAt', { time: new Date(grant.expires_at * 1000).toLocaleTimeString() }) }}</p></div><AppButton type="button" variant="secondary" :loading="saving" @click="revokeAssistant(grant.id)">{{ t('instructor.studio.revokeAccess') }}</AppButton></div>
        <p v-if="!assistantAccess?.grants.length" class="p-5 text-sm text-muted-foreground">{{ t('instructor.studio.noAssistants') }}</p>
      </div>
    </section>
    <ConfirmDialog :open="leave" :title="t('instructor.studio.unsaved')" :message="t('instructor.studio.discardMessage')" :confirm-label="t('instructor.studio.discard')" @confirm="answerLeave(true)" @cancel="answerLeave(false)" />
  </div>
</template>
