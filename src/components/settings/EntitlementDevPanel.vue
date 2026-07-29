<script setup lang="ts">
/**
 * Dev-only entitlement harness.
 *
 * Exists to exercise the entitlement chain end to end by hand — mint or
 * install a credential, then watch what the resolver makes of it. It is NOT a
 * shipped surface: the strings are deliberately English-only and this file is
 * listed in `FILE_SKIP` in `scripts/i18n/check-no-raw-text.mjs`, following the
 * `SentinelDebugPip.vue` precedent for dev instrumentation.
 *
 * It renders nothing in a community build. `@ee` resolves to the MIT stub
 * there, so there is no entitlement to inspect and no reason to occupy space
 * in Settings.
 *
 * Everything here goes through the ordinary MIT commands. Nothing bypasses
 * verification or the holder-binding check, so a credential that unlocks in
 * this panel would unlock the same way if it arrived from a billing service.
 */

import { ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'

import { AppButton, AppTextarea } from '@/components/ui'
import { useEntitlements } from '@/composables/useEntitlements'

interface ImportOutcome {
  credentialId: string
  stored: boolean
}

const { entitlements, refresh } = useEntitlements()

const localDid = ref<string>('')
const busy = ref(false)
const result = ref<string>('')
const failed = ref(false)

const credentialJson = ref('')
const ticketProvider = ref('')
const ticketHash = ref('')

/**
 * The community build has nothing to show: `@ee` resolves to the stub, which
 * reports `enterpriseBuild: false` and grants nothing by construction.
 */
const isEnterpriseBuild = () => entitlements.value.enterpriseBuild

function report(message: string, isError = false) {
  result.value = message
  failed.value = isError
}

/** Run an action, refresh the snapshot, and surface whatever happened. */
async function run(label: string, action: () => Promise<string>) {
  busy.value = true
  report('')
  try {
    const message = await action()
    await refresh()
    report(`${label}: ${message}`)
  } catch (e) {
    // Backend errors are strings and already name the failing check
    // ("expired", "signature", ...), so they are shown verbatim rather than
    // flattened into a generic failure.
    report(`${label} failed: ${String(e)}`, true)
  } finally {
    busy.value = false
  }
}

function describe(outcome: ImportOutcome): string {
  return outcome.stored
    ? `stored ${outcome.credentialId}`
    : `${outcome.credentialId} was already installed`
}

async function loadIdentity() {
  try {
    localDid.value = (await invoke<string | null>('get_local_did')) ?? '(none)'
  } catch {
    localDid.value = '(unavailable)'
  }
}

async function onRefresh() {
  await loadIdentity()
  await run('Refresh', async () => 'snapshot reloaded')
}

async function onImportJson() {
  await run('Import JSON', async () => {
    const credential = JSON.parse(credentialJson.value)
    const outcome = await invoke<ImportOutcome>('import_credential', { credential })
    return describe(outcome)
  })
}

async function onImportTicket() {
  await run('Fetch from peer', async () => {
    const outcome = await invoke<ImportOutcome>('import_credential_from_peer', {
      ticket: { provider: ticketProvider.value, hash: ticketHash.value },
    })
    return describe(outcome)
  })
}

/**
 * Only registered under `--features ee-staging`. In any other build the invoke
 * rejects with "command not found", which is the honest outcome — the staging
 * issuer genuinely does not exist there.
 */
async function onMintStaging() {
  await run('Mint staging', async () => {
    const outcome = await invoke<ImportOutcome>('mint_staging_entitlement', {
      features: ['talent_index', 'employer_console'],
    })
    return describe(outcome)
  })
}

void loadIdentity()
</script>

<template>
  <div v-if="isEnterpriseBuild()" class="space-y-4 rounded-xl border border-border bg-card p-4">
    <div>
      <h3 class="text-base font-semibold text-foreground">Entitlement (dev)</h3>
      <p class="mt-1 text-sm text-muted-foreground">
        Untranslated test harness. Not a shipped surface.
      </p>
    </div>

    <dl class="space-y-1 text-sm">
      <div class="flex gap-2">
        <dt class="w-24 shrink-0 text-muted-foreground">holder</dt>
        <dd class="min-w-0 flex-1 truncate font-mono text-xs text-foreground">{{ localDid }}</dd>
      </div>
      <div class="flex gap-2">
        <dt class="w-24 shrink-0 text-muted-foreground">org</dt>
        <dd class="min-w-0 flex-1 text-foreground">{{ entitlements.orgId ?? '—' }}</dd>
      </div>
      <div class="flex gap-2">
        <dt class="w-24 shrink-0 text-muted-foreground">plan</dt>
        <dd class="min-w-0 flex-1 text-foreground">{{ entitlements.plan ?? '—' }}</dd>
      </div>
      <div class="flex gap-2">
        <dt class="w-24 shrink-0 text-muted-foreground">features</dt>
        <dd class="min-w-0 flex-1 text-foreground">
          <span v-if="entitlements.features.length === 0">none</span>
          <span v-else class="font-mono text-xs">{{ entitlements.features.join(', ') }}</span>
        </dd>
      </div>
    </dl>

    <div class="space-y-2">
      <label class="block text-xs font-medium text-muted-foreground" for="ent-dev-json">
        Credential JSON
      </label>
      <AppTextarea
        id="ent-dev-json"
        v-model="credentialJson"
        :rows="4"
        class="font-mono text-xs"
        placeholder='{"@context": ..., "proof": ...}'
      />
      <AppButton :disabled="busy || !credentialJson.trim()" @click="onImportJson">
        Import JSON
      </AppButton>
    </div>

    <div class="space-y-2">
      <label class="block text-xs font-medium text-muted-foreground" for="ent-dev-provider">
        iroh ticket
      </label>
      <input
        id="ent-dev-provider"
        v-model="ticketProvider"
        class="w-full rounded-lg border border-border bg-background px-3 py-2 font-mono text-xs text-foreground"
        placeholder="provider endpoint id"
      />
      <input
        id="ent-dev-hash"
        v-model="ticketHash"
        class="w-full rounded-lg border border-border bg-background px-3 py-2 font-mono text-xs text-foreground"
        placeholder="blake3 hash (hex)"
      />
      <AppButton
        :disabled="busy || !ticketProvider.trim() || !ticketHash.trim()"
        @click="onImportTicket"
      >
        Fetch from peer
      </AppButton>
    </div>

    <div class="flex flex-wrap gap-2 border-t border-border pt-3">
      <AppButton :disabled="busy" @click="onMintStaging">Mint staging entitlement</AppButton>
      <AppButton variant="secondary" :disabled="busy" @click="onRefresh">Refresh</AppButton>
    </div>

    <p
      v-if="result"
      class="break-words font-mono text-xs"
      :class="failed ? 'text-destructive' : 'text-muted-foreground'"
    >
      {{ result }}
    </p>
  </div>
</template>
