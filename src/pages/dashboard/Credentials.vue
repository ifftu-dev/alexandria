<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
import { useI18n } from 'vue-i18n'
import { useRoute, useRouter } from 'vue-router'
import { AppButton, AppModal, AppInput, EmptyState } from '@/components/ui'
import { useCredentials } from '@/composables/useCredentials'
import { useLocalApi } from '@/composables/useLocalApi'
import SourceCredentialsModal from '@/components/credential/SourceCredentialsModal.vue'
import {
  classNameOf,
  CREDENTIAL_KINDS,
  type CredentialClass,
} from '@/components/credential/credentialKind'
import {
  type CredentialType,
  type DerivedSkillState,
  type IssueClaimRequest,
  type IssueCredentialRequest,
  type PresentationEnvelope,
  type SkillInfo,
  type VerifiableCredential,
} from '@/types'

const { t } = useI18n()
const route = useRoute()
const router = useRouter()
const api = useCredentials()
const { invoke } = useLocalApi()

// Core data ---------------------------------------------------------------
const derived = ref<DerivedSkillState[]>([])
const localDid = ref<string | null>(null)
const skillNames = ref<Map<string, string>>(new Map())
// id → input credential, for resolving each derived state's `sources`.
const credById = ref<Map<string, VerifiableCredential>>(new Map())
const initialLoading = ref(true)
const recomputing = ref(false)

// Optional skill pre-filter, set via `?skill=` (e.g. the "View credential"
// link from My Learning lands here filtered to a course's skill).
const skillFilter = ref<string | null>(
  typeof route.query.skill === 'string' ? route.query.skill : null,
)

// Search + sort -----------------------------------------------------------
const search = ref('')
type SortKey = 'level' | 'confidence' | 'trust' | 'evidence' | 'name' | 'recent'
const sortKey = ref<SortKey>('level')

// Drill-down --------------------------------------------------------------
const sourceOpen = ref(false)
const activeState = ref<DerivedSkillState | null>(null)

// Lifecycle ---------------------------------------------------------------
onMounted(async () => {
  await refresh()
  initialLoading.value = false
})

async function refresh() {
  localDid.value = (await invoke<string | null>('get_local_did').catch(() => null)) ?? null

  const skills = (await invoke<SkillInfo[]>('list_skills', {}).catch(() => [])) ?? []
  skillNames.value = new Map(skills.map((s) => [s.id, s.name]))

  // All credentials — used to resolve each derived state's source ids.
  await api.list()
  const map = new Map<string, VerifiableCredential>()
  for (const c of api.credentials.value) if (c.id) map.set(c.id, c)
  credById.value = map

  let states = localDid.value
    ? (await api.listDerivedStates(localDid.value)) ?? []
    : []
  // Nothing cached yet → compute once, then re-read.
  if (states.length === 0) {
    await api.recomputeAll()
    states = localDid.value ? (await api.listDerivedStates(localDid.value)) ?? [] : []
  }
  derived.value = states
}

async function recompute() {
  recomputing.value = true
  await api.recomputeAll()
  await refresh()
  recomputing.value = false
}

// Helpers -----------------------------------------------------------------
function skillLabel(id: string): string {
  return skillNames.value.get(id) ?? id
}

const LEVEL_KEYS = ['none', 'novice', 'beginner', 'competent', 'proficient', 'expert']
function levelLabel(n: number): string {
  const key = LEVEL_KEYS[n]
  return key ? t(`credentials.levels.${key}`) : t('credentials.card.levelShort', { level: n })
}

/** Resolve a derived state's source ids to loaded input credentials. */
function resolvedSources(s: DerivedSkillState): VerifiableCredential[] {
  return s.sources.map((id) => credById.value.get(id)).filter((c): c is VerifiableCredential => !!c)
}

/** Distinct input classes feeding a derived state (for the card icon row). */
function sourceClasses(s: DerivedSkillState): CredentialClass[] {
  const set = new Set<string>()
  for (const c of resolvedSources(s)) set.add(classNameOf(c.type))
  return [...set].filter((k): k is CredentialClass => k in CREDENTIAL_KINDS)
}

// Filter + sort -----------------------------------------------------------
const filtered = computed(() => {
  const q = search.value.trim().toLowerCase()
  let list = derived.value.filter((s) => {
    if (skillFilter.value && s.skill_id !== skillFilter.value) return false
    if (!q) return true
    return skillLabel(s.skill_id).toLowerCase().includes(q) || s.skill_id.toLowerCase().includes(q)
  })

  list = [...list].sort((a, b) => {
    switch (sortKey.value) {
      case 'level':
        return b.level - a.level || b.confidence - a.confidence
      case 'confidence':
        return b.confidence - a.confidence
      case 'trust':
        return b.trust_score - a.trust_score
      case 'evidence':
        return b.active_evidence_count - a.active_evidence_count
      case 'recent':
        return (b.computed_at ?? '').localeCompare(a.computed_at ?? '')
      case 'name':
        return skillLabel(a.skill_id).localeCompare(skillLabel(b.skill_id))
      default:
        return 0
    }
  })
  return list
})

const stats = computed(() => {
  const list = derived.value
  const count = list.length
  const avgLevel = count ? list.reduce((s, d) => s + d.level, 0) / count : 0
  const avgConf = count ? list.reduce((s, d) => s + d.confidence, 0) / count : 0
  return { count, avgLevel, avgConf }
})

function clearSkillFilter() {
  skillFilter.value = null
  void router.replace({ name: 'credentials' })
}

function openSources(s: DerivedSkillState) {
  activeState.value = s
  sourceOpen.value = true
}

function openCredential(id: string) {
  sourceOpen.value = false
  router.push({ name: 'credential-detail', params: { id } })
}

// Issue modal -------------------------------------------------------------
const issueOpen = ref(false)
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

function openIssue() {
  resetIssueForm()
  issueOpen.value = true
}

function resetIssueForm() {
  issueForm.value = {
    credential_type: 'FormalCredential',
    // Default to self-issuance: a credential about your own DID feeds
    // your derived skill state and shows up on this page immediately.
    subject: localDid.value ?? '',
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
  const evidenceRefs = issueForm.value.evidence_refs
    .split(',')
    .map((s) => s.trim())
    .filter(Boolean)
  const claim: IssueClaimRequest = {
    kind: 'skill',
    skillId: issueForm.value.skill_id,
    level: Number(issueForm.value.level),
    score: Number(issueForm.value.score),
    evidenceRefs,
  }
  const req: IssueCredentialRequest = {
    credential_type: issueForm.value.credential_type,
    subject: issueForm.value.subject.trim(),
    claim,
    evidence_refs: evidenceRefs,
    expiration_date: issueForm.value.expiration_date || null,
  }
  const issued = await api.issue(req)
  issueBusy.value = false
  if (issued) {
    issueOpen.value = false
    resetIssueForm()
    await recompute()
  } else {
    issueError.value = api.error.value
  }
}

// Present modal -----------------------------------------------------------
const presentOpen = ref(false)
const presentForm = ref({
  credential_id: '',
  reveal: 'credentialSubject.level',
  audience: '',
  nonce: '',
})
const presentBusy = ref(false)
const presentResult = ref<PresentationEnvelope | null>(null)
const presentError = ref<string | null>(null)

function openPresent() {
  presentForm.value = {
    credential_id: '',
    reveal: 'credentialSubject.level',
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
  if (env) presentResult.value = env
  else presentError.value = api.error.value
}

async function copyEnvelope() {
  if (!presentResult.value) return
  await navigator.clipboard.writeText(JSON.stringify(presentResult.value, null, 2))
}

// Export ------------------------------------------------------------------
const exporting = ref(false)
async function exportBundle() {
  exporting.value = true
  const json = await api.exportBundle()
  if (json) {
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
</script>

<template>
  <div>
    <!-- Header -->
    <div class="mb-8 flex items-start justify-between gap-4 flex-wrap">
      <div>
        <h1 class="text-3xl font-bold text-foreground">{{ $t('credentials.page.title') }}</h1>
        <p class="mt-2 max-w-2xl text-muted-foreground">
          {{ $t('credentials.page.intro') }}
        </p>
      </div>
      <div class="flex gap-2 flex-wrap">
        <AppButton variant="outline" :loading="recomputing" @click="recompute">
          {{ $t('credentials.page.refresh') }}
        </AppButton>
        <AppButton variant="outline" :loading="exporting" @click="exportBundle">
          {{ $t('credentials.page.exportAll') }}
        </AppButton>
        <AppButton variant="ghost" @click="openPresent">{{ $t('credentials.page.share') }}</AppButton>
        <AppButton @click="openIssue">{{ $t('credentials.page.add') }}</AppButton>
      </div>
    </div>

    <!-- Skeleton -->
    <div v-if="initialLoading" class="space-y-6">
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
          <p class="text-sm text-muted-foreground">{{ $t('credentials.stats.skills') }}</p>
          <p class="mt-2 text-3xl font-bold text-foreground">{{ stats.count }}</p>
        </div>
        <div class="rounded-xl bg-card shadow-sm p-6">
          <p class="text-sm text-muted-foreground">{{ $t('credentials.stats.avgLevel') }}</p>
          <p class="mt-2 text-3xl font-bold text-primary">{{ stats.avgLevel.toFixed(1) }}<span class="text-base text-muted-foreground">/5</span></p>
        </div>
        <div class="rounded-xl bg-card shadow-sm p-6">
          <p class="text-sm text-muted-foreground">{{ $t('credentials.stats.avgConfidence') }}</p>
          <p class="mt-2 text-3xl font-bold text-foreground">{{ (stats.avgConf * 100).toFixed(0) }}<span class="text-base text-muted-foreground">%</span></p>
        </div>
      </div>

      <!-- Search + sort -->
      <div class="mb-4 flex flex-wrap items-center gap-2">
        <div class="min-w-[14rem] flex-1">
          <AppInput v-model="search" :placeholder="$t('credentials.search.placeholder')" />
        </div>
        <select v-model="sortKey" class="input w-auto text-sm">
          <option value="level">{{ $t('credentials.sort.level') }}</option>
          <option value="confidence">{{ $t('credentials.sort.confidence') }}</option>
          <option value="trust">{{ $t('credentials.sort.trust') }}</option>
          <option value="evidence">{{ $t('credentials.sort.evidence') }}</option>
          <option value="recent">{{ $t('credentials.sort.recent') }}</option>
          <option value="name">{{ $t('credentials.sort.name') }}</option>
        </select>
      </div>

      <!-- Active skill filter chip -->
      <div v-if="skillFilter" class="mb-4 flex items-center gap-2">
        <span class="inline-flex items-center gap-2 rounded-full bg-primary/10 px-3 py-1 text-xs font-medium text-primary">
          {{ $t('credentials.filter.skill', { name: skillLabel(skillFilter) }) }}
          <button
            type="button"
            class="text-primary/70 transition-colors hover:text-primary"
            :aria-label="$t('credentials.filter.clear')"
            @click="clearSkillFilter"
          >
            <svg class="h-3 w-3" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2.5">
              <path stroke-linecap="round" stroke-linejoin="round" d="M6 18L18 6M6 6l12 12" />
            </svg>
          </button>
        </span>
      </div>

      <!-- Empty state -->
      <EmptyState
        v-if="filtered.length === 0"
        :title="$t('credentials.empty.title')"
        :description="derived.length === 0
          ? $t('credentials.empty.noneYet')
          : $t('credentials.empty.noMatch')"
      >
        <template #action>
          <AppButton @click="openIssue">{{ $t('credentials.page.add') }}</AppButton>
        </template>
      </EmptyState>

      <!-- Derived credential grid -->
      <div v-else class="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
        <article
          v-for="s in filtered"
          :key="s.skill_id"
          class="group rounded-xl bg-card shadow-sm p-5 transition-shadow hover:shadow-md cursor-pointer"
          @click="openSources(s)"
        >
          <div class="mb-3 flex items-start justify-between gap-2">
            <span class="inline-flex items-center gap-1.5 rounded-full px-2 py-0.5 text-[10px] font-semibold" :class="CREDENTIAL_KINDS.DerivedCredential.badge">
              <span class="h-1.5 w-1.5 rounded-full" :class="CREDENTIAL_KINDS.DerivedCredential.dot" />
              {{ $t('credentials.kind.derived.short') }}
            </span>
            <span class="text-xs font-semibold text-foreground">{{ $t('credentials.card.levelShort', { level: s.level }) }}<span class="text-muted-foreground">/5</span></span>
          </div>

          <p class="truncate text-sm font-semibold text-foreground" :title="skillLabel(s.skill_id)">
            {{ skillLabel(s.skill_id) }}
          </p>
          <p class="text-xs text-muted-foreground">{{ levelLabel(s.level) }}</p>

          <!-- Confidence bar -->
          <div class="mt-3">
            <div class="mb-1 flex justify-between text-[11px] text-muted-foreground">
              <span>{{ $t('credentials.card.confidence') }}</span>
              <span>{{ (s.confidence * 100).toFixed(0) }}%</span>
            </div>
            <div class="h-1.5 w-full overflow-hidden rounded-full bg-muted">
              <div class="h-full rounded-full bg-violet-500" :style="{ width: `${Math.round(s.confidence * 100)}%` }" />
            </div>
          </div>

          <dl class="mt-3 grid grid-cols-2 gap-x-3 gap-y-1 text-[11px]">
            <div class="flex justify-between">
              <dt class="text-muted-foreground">{{ $t('credentials.card.trust') }}</dt>
              <dd class="font-medium">{{ (s.trust_score * 100).toFixed(0) }}%</dd>
            </div>
            <div class="flex justify-between">
              <dt class="text-muted-foreground">{{ $t('credentials.card.sources') }}</dt>
              <dd class="font-medium">{{ s.unique_issuer_clusters }}</dd>
            </div>
          </dl>

          <!-- Evidence footer -->
          <div class="mt-4 flex items-center justify-between border-t border-border pt-3">
            <div class="flex items-center gap-1">
              <span
                v-for="k in sourceClasses(s)"
                :key="k"
                class="flex h-5 w-5 items-center justify-center rounded text-white"
                :class="CREDENTIAL_KINDS[k].dot"
                :title="$t(CREDENTIAL_KINDS[k].label)"
              >
                <svg viewBox="0 0 24 24" class="h-3 w-3" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                  <path :d="CREDENTIAL_KINDS[k].icon" />
                </svg>
              </span>
            </div>
            <span class="text-[11px] text-primary opacity-0 transition-opacity group-hover:opacity-100">
              {{ $t('credentials.card.evidence', { count: s.active_evidence_count }, s.active_evidence_count) }}
            </span>
          </div>
        </article>
      </div>
    </template>

    <!-- Source / evidence drill-down -->
    <SourceCredentialsModal
      :open="sourceOpen"
      :skill-name="activeState ? skillLabel(activeState.skill_id) : ''"
      :source-count="activeState?.active_evidence_count ?? 0"
      :sources="activeState ? resolvedSources(activeState) : []"
      @close="sourceOpen = false"
      @open-credential="openCredential"
    />

    <!-- Issue modal -->
    <AppModal :open="issueOpen" :title="$t('credentials.issue.title')" max-width="32rem" @close="issueOpen = false">
      <form class="space-y-4" @submit.prevent="submitIssue">
        <div>
          <label class="label text-xs text-muted-foreground">{{ $t('credentials.issue.type') }}</label>
          <select v-model="issueForm.credential_type" class="input">
            <option value="FormalCredential">{{ $t('credentials.issue.typeFormal') }}</option>
            <option value="AssessmentCredential">{{ $t('credentials.issue.typeAssessment') }}</option>
            <option value="AttestationCredential">{{ $t('credentials.issue.typeAttestation') }}</option>
            <option value="RoleCredential">{{ $t('credentials.issue.typeRole') }}</option>
            <option value="SelfAssertion">{{ $t('credentials.issue.typeSelf') }}</option>
          </select>
        </div>
        <AppInput v-model="issueForm.subject" :label="$t('credentials.issue.subject')" placeholder="did:key:z…" />
        <AppInput v-model="issueForm.skill_id" :label="$t('credentials.issue.skill')" placeholder="skill_x" />
        <div class="grid grid-cols-2 gap-3">
          <AppInput v-model="issueForm.level" :label="$t('credentials.issue.level')" type="number" />
          <AppInput v-model="issueForm.score" :label="$t('credentials.issue.score')" type="number" />
        </div>
        <AppInput
          v-model="issueForm.evidence_refs"
          :label="$t('credentials.issue.evidence')"
          placeholder="urn:uuid:e1, urn:uuid:e2"
        />
        <AppInput
          v-model="issueForm.expiration_date"
          :label="$t('credentials.issue.expiration')"
          placeholder="2028-04-13T00:00:00Z"
        />
        <p v-if="issueError" class="text-xs text-error">{{ issueError }}</p>
      </form>
      <template #footer>
        <div class="flex justify-end gap-2">
          <AppButton variant="ghost" @click="issueOpen = false">{{ $t('common.actions.cancel') }}</AppButton>
          <AppButton :loading="issueBusy" @click="submitIssue">{{ $t('credentials.issue.submit') }}</AppButton>
        </div>
      </template>
    </AppModal>

    <!-- Presentation modal -->
    <AppModal
      :open="presentOpen"
      :title="$t('credentials.present.title')"
      max-width="36rem"
      @close="presentOpen = false"
    >
      <div v-if="!presentResult" class="space-y-4">
        <AppInput v-model="presentForm.credential_id" :label="$t('credentials.present.credentialId')" placeholder="urn:uuid:…" />
        <AppInput
          v-model="presentForm.reveal"
          :label="$t('credentials.present.reveal')"
          placeholder="credentialSubject.level"
        />
        <AppInput v-model="presentForm.audience" :label="$t('credentials.present.audience')" placeholder="did:web:hirer.example" />
        <AppInput v-model="presentForm.nonce" :label="$t('credentials.present.nonce')" />
        <p v-if="presentError" class="text-xs text-error">{{ presentError }}</p>
      </div>
      <div v-else class="space-y-3">
        <p class="text-sm text-foreground">{{ $t('credentials.present.ready') }}</p>
        <pre class="max-h-64 overflow-auto rounded-md bg-muted/30 p-3 text-xs font-mono">{{ JSON.stringify(presentResult, null, 2) }}</pre>
      </div>
      <template #footer>
        <div class="flex justify-end gap-2">
          <AppButton variant="ghost" @click="presentOpen = false">{{ $t('common.actions.close') }}</AppButton>
          <AppButton v-if="!presentResult" :loading="presentBusy" @click="submitPresent">{{ $t('credentials.present.create') }}</AppButton>
          <AppButton v-else variant="outline" @click="copyEnvelope">{{ $t('credentials.present.copy') }}</AppButton>
        </div>
      </template>
    </AppModal>
  </div>
</template>
