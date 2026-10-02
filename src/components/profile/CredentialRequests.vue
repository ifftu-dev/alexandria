<script setup lang="ts">
import { onMounted, onUnmounted, ref } from 'vue'
import { useRouter } from 'vue-router'
import { useI18n } from 'vue-i18n'
import { useLocalApi } from '@/composables/useLocalApi'
import { AppButton } from '@/components/ui'
import type { DirectoryCredentialRequest, ShareableCredential, SignedCredentialShare } from '@/types'

const { invoke } = useLocalApi()
const { t } = useI18n()
const router = useRouter()
const requests = ref<DirectoryCredentialRequest[]>([])
const errors = ref<string[]>([])
const busy = ref(false)
const selected = ref<DirectoryCredentialRequest | null>(null)
const credentials = ref<ShareableCredential[]>([])
const credentialId = ref('')
const preview = ref<SignedCredentialShare | null>(null)
const receipt = ref('')
let revision = 0

async function refresh() {
  busy.value = true
  errors.value = []
  revision++
  selected.value = null
  preview.value = null
  try {
    const result = await invoke<{ items: DirectoryCredentialRequest[]; problems: { directory: string; detail: string }[] }>('exchange_requests')
    requests.value = result.items
    errors.value = result.problems.map(p => `${p.directory}: ${p.detail}`)
  } catch (error) { errors.value = [String(error)] }
  finally { busy.value = false }
}

async function choose(item: DirectoryCredentialRequest) {
  const generation = ++revision
  selected.value = item
  preview.value = null
  receipt.value = ''
  credentials.value = []
  credentialId.value = ''
  try {
    const result = await invoke<ShareableCredential[]>('exchange_credentials', { request: item.request })
    if (generation === revision) credentials.value = result
  } catch (error) { if (generation === revision) errors.value = [String(error)] }
}

async function showPreview() {
  if (!selected.value || !credentialId.value) return
  const generation = ++revision
  busy.value = true
  preview.value = null
  try {
    const result = await invoke<SignedCredentialShare>('exchange_preview', { request: selected.value.request, credentialId: credentialId.value })
    if (generation === revision) preview.value = result
  } catch (error) { errors.value = [String(error)] }
  finally { busy.value = false }
}

async function share() {
  if (!preview.value || !selected.value || busy.value) return
  busy.value = true
  errors.value = []
  try {
    const result = await invoke<{ received: boolean; verification: { acceptanceDecision: string } }>('exchange_send', {
      directoryUrl: selected.value.directory_url, signed: preview.value,
    })
    if (result.received) receipt.value = result.verification.acceptanceDecision
  } catch (error) { errors.value = [String(error)] }
  finally { busy.value = false }
}

function start(item: DirectoryCredentialRequest) {
  void router.push({ path: `/assessment/${encodeURIComponent(item.request.skill_id)}`, query: {
    request: item.request.id, directory: item.directory_url,
  } })
}

onMounted(refresh)
onUnmounted(() => { revision++ })
</script>

<template>
  <section class="space-y-4 rounded-xl border border-border p-5">
    <div class="flex items-center justify-between gap-3">
      <h2 class="text-lg font-semibold">{{ t('profile.exchange.title') }}</h2>
      <AppButton variant="outline" :disabled="busy" @click="refresh">{{ t('common.actions.refresh') }}</AppButton>
    </div>
    <p class="text-sm text-muted-foreground">{{ t('profile.exchange.note') }}</p>
    <p v-for="error in errors" :key="error" role="alert" class="text-sm text-error">{{ error }}</p>
    <p v-if="!busy && !requests.length" class="text-sm">{{ t('profile.exchange.empty') }}</p>
    <article v-for="item in requests" :key="`${item.directory_url}:${item.request.id}`" class="space-y-2 rounded-lg border border-border p-4">
      <h3 class="font-medium">{{ item.request.organization }} · {{ item.request.role_label }}</h3>
      <p class="text-sm">{{ item.request.purpose }}</p>
      <p class="text-sm text-muted-foreground">{{ item.request.skill_id }} · {{ new Date(item.request.expires_at * 1000).toLocaleString() }}</p>
      <div class="flex gap-2">
        <AppButton v-if="item.request.require_new_assessment" :disabled="busy" @click="start(item)">{{ t('profile.exchange.assess') }}</AppButton>
        <AppButton variant="outline" :disabled="busy" @click="choose(item)">{{ t('profile.exchange.choose') }}</AppButton>
      </div>
    </article>
    <div v-if="selected" class="space-y-3 rounded-lg border border-border p-4">
      <p class="font-medium">{{ selected.request.organization }} · {{ t('profile.exchange.choose') }}</p>
      <p v-if="!credentials.length" class="text-sm">{{ t('profile.exchange.noCredentials') }}</p>
      <select v-else v-model="credentialId" class="w-full rounded border border-border bg-background p-2" :disabled="busy" @change="preview = null; receipt = ''">
        <option value="">{{ t('profile.exchange.choose') }}</option>
        <option v-for="credential in credentials" :key="credential.id" :value="credential.id">{{ credential.id }} · {{ credential.issued_at }}</option>
      </select>
      <AppButton :disabled="!credentialId || busy" @click="showPreview">{{ t('profile.exchange.preview') }}</AppButton>
      <template v-if="preview">
        <p class="text-sm">{{ t('profile.exchange.disclosure') }}</p>
        <details><summary>{{ t('profile.exchange.exact') }}</summary><pre class="max-h-80 overflow-auto whitespace-pre-wrap break-all text-xs">{{ JSON.stringify(preview.share, null, 2) }}</pre></details>
        <AppButton v-if="!receipt" :disabled="busy" @click="share">{{ t('profile.exchange.share') }}</AppButton>
        <p v-else role="status" class="text-sm">{{ t('profile.exchange.received') }}: {{ receipt }}</p>
      </template>
    </div>
  </section>
</template>
