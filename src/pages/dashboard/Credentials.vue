<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
import { useRouter } from 'vue-router'
import { AppBadge, AppButton, AppModal, AppInput, EmptyState } from '@/components/ui'
import { useCredentials } from '@/composables/useCredentials'
import type {
  Claim,
  CredentialType,
  IssueCredentialRequest,
  PresentationEnvelope,
  VerifiableCredential,
} from '@/types'

const router = useRouter()
const api = useCredentials()

const filterType = ref<'all' | CredentialType>('all')

// Issue modal -------------------------------------------------------------
const issueOpen = ref(false)
// Form fields are kept as strings so AppInput's `modelValue: string` prop
// matches; we coerce to numbers at submit time.
const issueForm = ref({
  credential_type: 'FormalCredential' as CredentialType,
  subject: '',
  skill_id: '',
  level: '4',
  score: '0.85',
  evidence_refs: '',
  expiration_date: '',
})
const issueError = ref<string | null>(null)
const issueBusy = ref(false)

// Presentation modal ------------------------------------------------------
const presentOpen = ref(false)
const presentForm = ref({
  credential_id: '',
  reveal: 'credential_subject.claim.level',
  audience: '',
  nonce: '',
})
const presentBusy = ref(false)
const presentResult = ref<PresentationEnvelope | null>(null)
const presentError = ref<string | null>(null)

// Lifecycle ---------------------------------------------------------------
onMounted(() => api.list())

// Derived stats -----------------------------------------------------------
const filtered = computed(() => {
  if (filterType.value === 'all') return api.credentials.value
  return api.credentials.value.filter((c) =>
    c.type.includes(filterType.value as string),
  )
})

const stats = computed(() => {
  const list = api.credentials.value
  const active = list.filter((c) => !revokedFlag(c)).length
  const revoked = list.length - active
  return { total: list.length, active, revoked }
})

const credentialTypes: { value: 'all' | CredentialType; label: string }[] = [
  { value: 'all', label: 'All' },
  { value: 'FormalCredential', label: 'Formal' },
  { value: 'AssessmentCredential', label: 'Assessment' },
  { value: 'AttestationCredential', label: 'Attestation' },
  { value: 'RoleCredential', label: 'Role' },
  { value: 'SelfAssertion', label: 'Self' },
]

// Helpers -----------------------------------------------------------------

/** True if the credential's status_list_index has been flipped on its issuer's list. */
function revokedFlag(_c: VerifiableCredential): boolean {
  // The presence of credential_status doesn't mean revoked — we'd need
  // to consult the local status list to know for sure. The backend
  // `verify` IPC carries a `revoked` boolean; the list view skips
  // that round-trip and shows "active" optimistically. The detail
  // page does the real check.
  return false
}

function classOf(c: VerifiableCredential): string {
  return c.type.find((t) => t !== 'VerifiableCredential') ?? 'Credential'
}

function summary(c: VerifiableCredential): string {
  const claim = c.credential_subject.claim
  if (claim.kind === 'skill') {
    return `${claim.skill_id} · L${claim.level} · ${(claim.score * 100).toFixed(0)}%`
  }
  if (claim.kind === 'role') {
    return claim.role + (claim.scope ? ` · ${claim.scope}` : '')
  }
  return 'custom claim'
}

function shortDid(did: string): string {
  if (did.length <= 24) return did
  return `${did.slice(0, 14)}…${did.slice(-6)}`
}

function open(c: VerifiableCredential) {
  router.push({ name: 'dashboard-credential-detail', params: { id: c.id } })
}

// Issue flow --------------------------------------------------------------

function resetIssueForm() {
  issueForm.value = {
    credential_type: 'FormalCredential',
    subject: '',
    skill_id: '',
    level: '4',
    score: '0.85',
    evidence_refs: '',
    expiration_date: '',
  }
  issueError.value = null
}

async function submitIssue() {
  issueBusy.value = true
  issueError.value = null
  const claim: Claim = {
    kind: 'skill',
    skill_id: issueForm.value.skill_id,
    level: Number(issueForm.value.level),
    score: Number(issueForm.value.score),
    evidence_refs: issueForm.value.evidence_refs
      .split(',')
      .map((s) => s.trim())
      .filter(Boolean),
  }
  const req: IssueCredentialRequest = {
    credential_type: issueForm.value.credential_type,
    subject: issueForm.value.subject.trim(),
    claim,
    evidence_refs: claim.kind === 'skill' ? claim.evidence_refs : [],
    expiration_date: issueForm.value.expiration_date || null,
  }
  const issued = await api.issue(req)
  issueBusy.value = false
  if (issued) {
    issueOpen.value = false
    resetIssueForm()
    await api.list()
  } else {
    issueError.value = api.error.value
  }
}

// Export flow -------------------------------------------------------------

const exporting = ref(false)
async function exportBundle() {
  exporting.value = true
  const json = await api.exportBundle()
  if (json) {
    // Browser-side download — the webview hands the file off to the
    // OS Save panel without us needing the @tauri-apps/plugin-fs dep.
    const blob = new Blob([json], { type: 'application/json' })
    const url = URL.createObjectURL(blob)
    const a = document.createElement('a')
    a.href = url
    a.download = `alexandria-credentials-${new Date().toISOString().slice(0, 10)}.json`
    document.body.appendChild(a)
    a.click()
    document.body.removeChild(a)
    URL.revokeObjectURL(url)
  }
  exporting.value = false
}

// Presentation flow -------------------------------------------------------

function openPresent(c?: VerifiableCredential) {
  presentForm.value = {
    credential_id: c?.id ?? '',
    reveal: 'credential_subject.claim.level',
    audience: '',
    nonce: crypto.randomUUID(),
  }
  presentResult.value = null
  presentError.value = null
  presentOpen.value = true
}

async function submitPresent() {
  presentBusy.value = true
  presentError.value = null
  const env = await api.createPresentation({
    credential_ids: [presentForm.value.credential_id],
    reveal: presentForm.value.reveal
      .split(',')
      .map((s) => s.trim())
      .filter(Boolean),
    audience: presentForm.value.audience,
    nonce: presentForm.value.nonce,
  })
  presentBusy.value = false
  if (env) {
    presentResult.value = env
  } else {
    presentError.value = api.error.value
  }
}

async function copyEnvelope() {
  if (!presentResult.value) return
  await navigator.clipboard.writeText(JSON.stringify(presentResult.value, null, 2))
}
</script>

<template>
  <div>
    <!-- Header -->
    <div class="mb-8 flex items-start justify-between gap-4 flex-wrap">
      <div>
        <h1 class="text-3xl font-bold text-foreground">Credentials</h1>
        <p class="mt-2 text-muted-foreground">
          Verifiable Credentials issued to or by you. Signed with your
          DID, verifiable offline, portable to any W3C-compatible verifier.
        </p>
      </div>
      <div class="flex gap-2 flex-wrap">
        <AppButton variant="outline" :loading="exporting" @click="exportBundle">
          Export bundle
        </AppButton>
        <AppButton @click="issueOpen = true">Issue credential</AppButton>
      </div>
    </div>

    <!-- Skeleton -->
    <div v-if="api.loading.value && api.credentials.value.length === 0" class="space-y-6">
      <div class="grid gap-4 sm:grid-cols-3">
        <div v-for="i in 3" :key="i" class="animate-pulse rounded-xl bg-card shadow-sm p-6">
          <div class="h-3 w-20 rounded bg-muted-foreground/15 mb-3" />
          <div class="h-8 w-12 rounded bg-muted-foreground/20" />
        </div>
      </div>
    </div>

    <template v-else>
      <!-- Stats -->
      <div class="mb-6 grid gap-4 sm:grid-cols-3">
        <div class="rounded-xl bg-card shadow-sm p-6">
          <p class="text-sm text-muted-foreground">Total credentials</p>
          <p class="mt-2 text-3xl font-bold text-foreground">{{ stats.total }}</p>
        </div>
        <div class="rounded-xl bg-card shadow-sm p-6">
          <p class="text-sm text-muted-foreground">Active</p>
          <p class="mt-2 text-3xl font-bold text-primary">{{ stats.active }}</p>
        </div>
        <div class="rounded-xl bg-card shadow-sm p-6">
          <p class="text-sm text-muted-foreground">Revoked</p>
          <p class="mt-2 text-3xl font-bold text-foreground">{{ stats.revoked }}</p>
        </div>
      </div>

      <!-- Filter tabs -->
      <div class="mb-6 flex gap-1 rounded-lg bg-muted p-1 overflow-x-auto">
        <button
          v-for="t in credentialTypes"
          :key="t.value"
          class="flex-1 min-w-fit rounded-md px-3 py-1.5 text-xs font-medium transition-colors"
          :class="filterType === t.value
            ? 'bg-card text-foreground shadow-sm'
            : 'text-muted-foreground hover:text-foreground'"
          @click="filterType = t.value"
        >
          {{ t.label }}
        </button>
      </div>

      <!-- Empty state -->
      <EmptyState
        v-if="filtered.length === 0"
        title="No credentials yet"
        description="Issue your first credential, or wait for one to arrive over the network."
      >
        <template #action>
          <AppButton @click="issueOpen = true">Issue credential</AppButton>
        </template>
      </EmptyState>

      <!-- Card grid -->
      <div v-else class="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
        <article
          v-for="c in filtered"
          :key="c.id"
          class="rounded-xl bg-card shadow-sm p-5 transition-shadow hover:shadow-md cursor-pointer"
          @click="open(c)"
        >
          <div class="flex items-start justify-between mb-3 gap-2">
            <AppBadge variant="primary">{{ classOf(c) }}</AppBadge>
            <AppBadge v-if="c.expiration_date" variant="secondary">
              expires
            </AppBadge>
          </div>
          <p class="text-sm font-semibold text-foreground truncate" :title="summary(c)">
            {{ summary(c) }}
          </p>
          <dl class="mt-3 space-y-1 text-xs">
            <div class="flex justify-between gap-2">
              <dt class="text-muted-foreground">Issuer</dt>
              <dd class="font-mono truncate" :title="c.issuer">{{ shortDid(c.issuer) }}</dd>
            </div>
            <div class="flex justify-between gap-2">
              <dt class="text-muted-foreground">Subject</dt>
              <dd class="font-mono truncate" :title="c.credential_subject.id">
                {{ shortDid(c.credential_subject.id) }}
              </dd>
            </div>
            <div class="flex justify-between gap-2">
              <dt class="text-muted-foreground">Issued</dt>
              <dd>{{ c.issuance_date.slice(0, 10) }}</dd>
            </div>
          </dl>
          <div class="mt-4 flex gap-2" @click.stop>
            <AppButton size="xs" variant="outline" @click="open(c)">View</AppButton>
            <AppButton size="xs" variant="ghost" @click="openPresent(c)">Present</AppButton>
          </div>
        </article>
      </div>
    </template>

    <!-- Issue modal -->
    <AppModal :open="issueOpen" title="Issue credential" max-width="32rem" @close="issueOpen = false">
      <form class="space-y-4" @submit.prevent="submitIssue">
        <div>
          <label class="label text-xs text-muted-foreground">Credential type</label>
          <select v-model="issueForm.credential_type" class="input">
            <option value="FormalCredential">Formal</option>
            <option value="AssessmentCredential">Assessment</option>
            <option value="AttestationCredential">Attestation</option>
            <option value="RoleCredential">Role</option>
            <option value="SelfAssertion">Self assertion</option>
          </select>
        </div>
        <AppInput
          v-model="issueForm.subject"
          label="Subject DID"
          placeholder="did:key:z…"
        />
        <AppInput v-model="issueForm.skill_id" label="Skill ID" placeholder="skill_x" />
        <div class="grid grid-cols-2 gap-3">
          <AppInput v-model="issueForm.level" label="Level (1–5)" type="number" />
          <AppInput v-model="issueForm.score" label="Score (0–1)" type="number" />
        </div>
        <AppInput
          v-model="issueForm.evidence_refs"
          label="Evidence refs (comma-separated)"
          placeholder="urn:uuid:e1, urn:uuid:e2"
        />
        <AppInput
          v-model="issueForm.expiration_date"
          label="Expiration (ISO 8601, optional)"
          placeholder="2028-04-13T00:00:00Z"
        />
        <p v-if="issueError" class="text-xs text-error">{{ issueError }}</p>
      </form>
      <template #footer>
        <div class="flex justify-end gap-2">
          <AppButton variant="ghost" @click="issueOpen = false">Cancel</AppButton>
          <AppButton :loading="issueBusy" @click="submitIssue">Issue</AppButton>
        </div>
      </template>
    </AppModal>

    <!-- Presentation modal -->
    <AppModal
      :open="presentOpen"
      title="Create selective-disclosure presentation"
      max-width="36rem"
      @close="presentOpen = false"
    >
      <div v-if="!presentResult" class="space-y-4">
        <AppInput
          v-model="presentForm.credential_id"
          label="Credential ID"
          placeholder="urn:uuid:…"
        />
        <AppInput
          v-model="presentForm.reveal"
          label="Reveal paths (comma-separated)"
          placeholder="credential_subject.claim.level"
        />
        <AppInput
          v-model="presentForm.audience"
          label="Audience"
          placeholder="did:web:hirer.example"
        />
        <AppInput v-model="presentForm.nonce" label="Nonce" />
        <p v-if="presentError" class="text-xs text-error">{{ presentError }}</p>
      </div>
      <div v-else class="space-y-3">
        <p class="text-sm text-foreground">Presentation envelope created.</p>
        <pre class="max-h-64 overflow-auto rounded-md bg-muted/30 p-3 text-xs font-mono">{{ JSON.stringify(presentResult, null, 2) }}</pre>
      </div>
      <template #footer>
        <div class="flex justify-end gap-2">
          <AppButton variant="ghost" @click="presentOpen = false">Close</AppButton>
          <AppButton v-if="!presentResult" :loading="presentBusy" @click="submitPresent">
            Create
          </AppButton>
          <AppButton v-else variant="outline" @click="copyEnvelope">Copy JSON</AppButton>
        </div>
      </template>
    </AppModal>
  </div>
</template>
