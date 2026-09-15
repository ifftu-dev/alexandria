<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { useRoute } from 'vue-router'
import { useI18n } from 'vue-i18n'
import { useLocalApi } from '@/composables/useLocalApi'
import { AppButton, AppInput } from '@/components/ui'
import QrCodeDisplay from '@/components/tutoring/QrCodeDisplay.vue'
import type {
  GovernanceGenesisLocator,
  PinGenesisResponse,
  RetrievedGenesisPreview,
  ReviewedGenesisLocator,
} from '@/types'

const route = useRoute()
const { t } = useI18n()
const { invoke } = useLocalApi()

const locatorInput = ref(typeof route.query.locator === 'string' ? route.query.locator : '')
/** Snapshot of the reviewed, normalized locator. Retrieval and the QR code
 *  use only this, so the facts shown always describe what was fetched. */
const locator = ref<ReviewedGenesisLocator | null>(null)
const retrieved = ref<RetrievedGenesisPreview | null>(null)
const confirmation = ref('')
const error = ref('')
const success = ref('')
const reviewing = ref(false)
const retrieving = ref(false)
const pinning = ref(false)

const canPin = computed(
  () =>
    locator.value !== null &&
    retrieved.value !== null &&
    confirmation.value === retrieved.value.preview.dao_id,
)

function message(reason: unknown): string {
  return reason instanceof Error ? reason.message : String(reason)
}

/** Make control, format, separator and private-use characters visible as
 *  `U+XXXX` so bidi overrides or zero-width characters cannot disguise a
 *  reviewed value. The backend already rejects them; this keeps the display
 *  robust if that ever regresses. */
function visible(value: string): string {
  return value.replace(
    /[\p{Cc}\p{Cf}\p{Co}\p{Zl}\p{Zp}]/gu,
    character =>
      `[U+${(character.codePointAt(0) ?? 0).toString(16).toUpperCase().padStart(4, '0')}]`,
  )
}

function sameLocator(left: GovernanceGenesisLocator, right: GovernanceGenesisLocator): boolean {
  return (
    left.version === right.version &&
    left.dao_id === right.dao_id &&
    left.content_hash === right.content_hash &&
    left.locations.length === right.locations.length &&
    left.locations.every((location, index) => location === right.locations[index])
  )
}

function resetReview(): void {
  locator.value = null
  retrieved.value = null
  confirmation.value = ''
  success.value = ''
}

// Any edit invalidates the review: nothing may be retrieved, shared or
// pinned on the strength of a locator that is no longer the one on screen.
watch(locatorInput, () => {
  resetReview()
  error.value = ''
})

async function reviewLocator(): Promise<void> {
  const requested = locatorInput.value
  error.value = ''
  resetReview()
  reviewing.value = true
  try {
    const reviewed = await invoke<ReviewedGenesisLocator>('governance_preview_genesis_locator', {
      locatorUri: requested,
    })
    if (locatorInput.value === requested) locator.value = reviewed
  } catch (reason) {
    if (locatorInput.value === requested) error.value = message(reason)
  } finally {
    reviewing.value = false
  }
}

async function retrieveGenesis(): Promise<void> {
  const reviewed = locator.value
  if (!reviewed) return
  error.value = ''
  success.value = ''
  retrieved.value = null
  confirmation.value = ''
  retrieving.value = true
  try {
    const result = await invoke<RetrievedGenesisPreview>('governance_retrieve_genesis', {
      locatorUri: reviewed.canonical_uri,
    })
    if (locator.value !== reviewed) return
    if (!sameLocator(result.locator, reviewed) || result.preview.dao_id !== reviewed.dao_id) {
      error.value = t('governanceGenesisImport.retrievedMismatch')
      return
    }
    retrieved.value = result
  } catch (reason) {
    if (locator.value === reviewed) error.value = message(reason)
  } finally {
    retrieving.value = false
  }
}

async function pinGenesis(): Promise<void> {
  const current = retrieved.value
  if (!current || !canPin.value) return
  error.value = ''
  success.value = ''
  pinning.value = true
  try {
    const result = await invoke<PinGenesisResponse>('governance_pin_genesis', {
      genesisJson: current.genesis_json,
      expectedDaoId: confirmation.value,
    })
    if (retrieved.value !== current) return
    success.value = result.newly_pinned
      ? t('governanceGenesisImport.pinned')
      : result.stored_envelope_differs
        ? t('governanceGenesisImport.equivalentAlreadyPinned')
        : t('governanceGenesisImport.alreadyPinned')
  } catch (reason) {
    error.value = message(reason)
  } finally {
    pinning.value = false
  }
}

onMounted(() => {
  if (locatorInput.value) void reviewLocator()
})
</script>

<template>
  <div class="mx-auto max-w-4xl space-y-6">
    <div>
      <h1 class="text-2xl font-bold text-foreground">
        {{ $t('governanceGenesisImport.title') }}
      </h1>
      <p class="mt-2 text-sm text-muted-foreground">
        {{ $t('governanceGenesisImport.subtitle') }}
      </p>
    </div>

    <section class="rounded-xl bg-card p-5 shadow-sm">
      <label class="text-sm font-medium text-foreground" for="genesis-locator">
        {{ $t('governanceGenesisImport.locatorLabel') }}
      </label>
      <textarea
        id="genesis-locator"
        v-model="locatorInput"
        class="input mt-2 min-h-28 w-full resize-y font-mono text-xs"
        dir="ltr"
        :placeholder="$t('governanceGenesisImport.locatorPlaceholder')"
      />
      <p class="mt-2 text-xs text-muted-foreground">
        {{ $t('governanceGenesisImport.locatorHint') }}
      </p>
      <AppButton class="mt-4" :loading="reviewing" @click="reviewLocator">
        {{ $t('governanceGenesisImport.reviewLocator') }}
      </AppButton>
    </section>

    <p v-if="error" class="rounded-lg border border-red-500/30 bg-red-500/10 p-3 text-sm text-red-600 dark:text-red-400">
      {{ error }}
    </p>
    <p v-if="success" class="rounded-lg border border-emerald-500/30 bg-emerald-500/10 p-3 text-sm text-emerald-700 dark:text-emerald-400">
      {{ success }}
    </p>

    <section v-if="locator" class="rounded-xl bg-card p-5 shadow-sm" data-testid="locator-facts">
      <h2 class="text-base font-semibold text-foreground">
        {{ $t('governanceGenesisImport.locatorFacts') }}
      </h2>
      <dl class="mt-4 grid gap-4 sm:grid-cols-2">
        <div>
          <dt class="text-xs text-muted-foreground">{{ $t('governanceGenesisImport.daoId') }}</dt>
          <dd class="mt-1 break-all font-mono text-xs text-foreground" dir="ltr">{{ visible(locator.dao_id) }}</dd>
        </div>
        <div>
          <dt class="text-xs text-muted-foreground">{{ $t('governanceGenesisImport.contentHash') }}</dt>
          <dd class="mt-1 break-all font-mono text-xs text-foreground" dir="ltr">{{ visible(locator.content_hash) }}</dd>
        </div>
      </dl>
      <h3 class="mt-5 text-sm font-medium text-foreground">
        {{ $t('governanceGenesisImport.sources') }}
      </h3>
      <ul class="mt-2 space-y-2">
        <li v-for="source in locator.locations" :key="source" class="break-all rounded bg-background p-2 font-mono text-xs text-foreground" dir="ltr">
          {{ visible(source) }}
        </li>
      </ul>
      <h3 class="mt-5 text-sm font-medium text-foreground">
        {{ $t('governanceGenesisImport.canonicalLocator') }}
      </h3>
      <p class="mt-2 break-all rounded bg-background p-2 font-mono text-xs text-foreground" dir="ltr" data-testid="canonical-locator">
        {{ visible(locator.canonical_uri) }}
      </p>
      <details class="mt-4 rounded border border-border p-3">
        <summary class="cursor-pointer text-sm font-medium text-foreground">
          {{ $t('governanceGenesisImport.qr') }}
        </summary>
        <QrCodeDisplay class="mt-3" :value="locator.canonical_uri" :size="240" />
      </details>
      <p class="mt-4 text-xs text-muted-foreground">
        {{ $t('governanceGenesisImport.retrieveHint') }}
      </p>
      <AppButton class="mt-3" :loading="retrieving" @click="retrieveGenesis">
        {{ $t('governanceGenesisImport.retrieve') }}
      </AppButton>
    </section>

    <section v-if="locator && retrieved" class="rounded-xl bg-card p-5 shadow-sm" data-testid="trust-facts">
      <h2 class="text-base font-semibold text-foreground">
        {{ $t('governanceGenesisImport.trustFacts') }}
      </h2>
      <dl class="mt-4 grid gap-4 sm:grid-cols-2">
        <div>
          <dt class="text-xs text-muted-foreground">{{ $t('governanceGenesisImport.name') }}</dt>
          <dd class="mt-1 text-sm text-foreground" data-testid="genesis-name"><bdi>{{ visible(retrieved.preview.name) }}</bdi></dd>
        </div>
        <div>
          <dt class="text-xs text-muted-foreground">{{ $t('governanceGenesisImport.scope') }}</dt>
          <dd class="mt-1 text-sm text-foreground" dir="ltr">{{ visible(retrieved.preview.scope_type) }} / {{ visible(retrieved.preview.scope_id) }}</dd>
        </div>
        <div>
          <dt class="text-xs text-muted-foreground">{{ $t('governanceGenesisImport.daoId') }}</dt>
          <dd class="mt-1 break-all font-mono text-xs text-foreground" dir="ltr">{{ visible(retrieved.preview.dao_id) }}</dd>
        </div>
        <div>
          <dt class="text-xs text-muted-foreground">{{ $t('governanceGenesisImport.coreHash') }}</dt>
          <dd class="mt-1 break-all font-mono text-xs text-foreground" dir="ltr">{{ visible(retrieved.preview.core_hash) }}</dd>
        </div>
        <div>
          <dt class="text-xs text-muted-foreground">{{ $t('governanceGenesisImport.envelopeHash') }}</dt>
          <dd class="mt-1 break-all font-mono text-xs text-foreground" dir="ltr">{{ visible(retrieved.preview.envelope_hash) }}</dd>
        </div>
        <div>
          <dt class="text-xs text-muted-foreground">{{ $t('governanceGenesisImport.rulesHash') }}</dt>
          <dd class="mt-1 break-all font-mono text-xs text-foreground" dir="ltr">{{ visible(retrieved.preview.rules_hash) }}</dd>
        </div>
        <div>
          <dt class="text-xs text-muted-foreground">{{ $t('governanceGenesisImport.versions') }}</dt>
          <dd class="mt-1 text-sm text-foreground" dir="ltr">{{ retrieved.preview.protocol_version }} / {{ visible(retrieved.preview.rules_version) }}</dd>
        </div>
        <div>
          <dt class="text-xs text-muted-foreground">{{ $t('governanceGenesisImport.thresholds') }}</dt>
          <dd class="mt-1 text-sm text-foreground" dir="ltr">
            {{ retrieved.preview.committee_size }} / {{ retrieved.preview.receipt_threshold }} / {{ retrieved.preview.outcome_threshold }}
          </dd>
        </div>
        <div>
          <dt class="text-xs text-muted-foreground">{{ $t('governanceGenesisImport.turnout') }}</dt>
          <dd class="mt-1 text-sm text-foreground" dir="ltr">{{ retrieved.preview.minimum_turnout_count }}</dd>
        </div>
        <div>
          <dt class="text-xs text-muted-foreground">{{ $t('governanceGenesisImport.approvalRule') }}</dt>
          <dd class="mt-1 text-sm text-foreground" dir="ltr">{{ retrieved.preview.proposal_approval_numerator }} / {{ retrieved.preview.proposal_approval_denominator }}</dd>
        </div>
        <div>
          <dt class="text-xs text-muted-foreground">{{ $t('governanceGenesisImport.activation') }}</dt>
          <dd class="mt-1 break-all text-sm text-foreground" dir="ltr">
            {{ visible(retrieved.preview.cometbft_chain_id) }} / {{ retrieved.preview.initial_epoch }} / {{ retrieved.preview.initial_height }}
          </dd>
        </div>
        <div>
          <dt class="text-xs text-muted-foreground">{{ $t('governanceGenesisImport.activationTime') }}</dt>
          <dd class="mt-1 text-sm text-foreground">{{ new Date(retrieved.preview.activation_time_unix * 1000).toLocaleString() }}</dd>
        </div>
        <div>
          <dt class="text-xs text-muted-foreground">{{ $t('governanceGenesisImport.policyVersion') }}</dt>
          <dd class="mt-1 text-sm text-foreground" dir="ltr">{{ visible(retrieved.preview.qualification_policy_version) }}</dd>
        </div>
        <div>
          <dt class="text-xs text-muted-foreground">{{ $t('governanceGenesisImport.resolvedFrom') }}</dt>
          <dd class="mt-1 break-all font-mono text-xs text-foreground" dir="ltr">{{ visible(retrieved.resolved_from) }}</dd>
        </div>
      </dl>

      <div class="mt-5 grid gap-5 sm:grid-cols-2">
        <div>
          <h3 class="text-sm font-medium text-foreground">{{ $t('governanceGenesisImport.issuers') }}</h3>
          <ul class="mt-2 space-y-1">
            <li v-for="issuer in retrieved.preview.accepted_issuers" :key="issuer" class="break-all font-mono text-xs text-muted-foreground" dir="ltr">{{ visible(issuer) }}</li>
          </ul>
        </div>
        <div>
          <h3 class="text-sm font-medium text-foreground">{{ $t('governanceGenesisImport.evidence') }}</h3>
          <ul class="mt-2 space-y-1">
            <li v-for="item in retrieved.preview.accepted_assessment_evidence" :key="item" class="break-all font-mono text-xs text-muted-foreground" dir="ltr">{{ visible(item) }}</li>
          </ul>
        </div>
      </div>

      <h3 class="mt-5 text-sm font-medium text-foreground">
        {{ $t('governanceGenesisImport.founders') }}
      </h3>
      <div class="mt-2 space-y-3">
        <details v-for="member in retrieved.preview.members" :key="member.member_id" class="rounded border border-border p-3">
          <summary class="cursor-pointer break-all font-mono text-sm font-medium text-foreground" dir="ltr">{{ visible(member.member_id) }}</summary>
          <dl class="mt-3 space-y-2 font-mono text-xs text-muted-foreground" dir="ltr">
            <div><dt>{{ $t('governanceGenesisImport.identityKey') }}</dt><dd class="break-all">{{ visible(member.identity_public_key_hex) }}</dd></div>
            <div><dt>{{ $t('governanceGenesisImport.consensusKey') }}</dt><dd class="break-all">{{ visible(member.consensus_public_key_hex) }}</dd></div>
            <div><dt>{{ $t('governanceGenesisImport.governanceKey') }}</dt><dd class="break-all">{{ visible(member.governance_public_key_hex) }}</dd></div>
          </dl>
        </details>
      </div>

      <div class="mt-6 border-t border-border pt-5">
        <AppInput
          v-model="confirmation"
          :label="$t('governanceGenesisImport.confirmLabel')"
          :placeholder="retrieved.preview.dao_id"
        />
        <p class="mt-2 text-xs text-muted-foreground">
          {{ $t('governanceGenesisImport.confirmHint') }}
        </p>
        <AppButton class="mt-4" variant="governance" :loading="pinning" :disabled="!canPin" @click="pinGenesis">
          {{ $t('governanceGenesisImport.pin') }}
        </AppButton>
      </div>
    </section>
  </div>
</template>
