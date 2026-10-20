<script lang="ts">
import { ref as moduleRef } from 'vue'

/** Hidden with "Not now". Module scope so it lasts the session, not the visit. */
const notNow = moduleRef<Set<string>>(new Set())
</script>

<script setup lang="ts">
/**
 * Everything organisations have asked of the learner, in one inbox.
 *
 * Four kinds arrive through Alexandria Cloud — requests to see skills,
 * assessment requests, interview invitations and offers — and from the
 * learner's side they are one question: "someone wants something from me".
 * Splitting them by kind made the learner check four lists to learn that
 * nothing was waiting.
 *
 * Every card leads with who sent it, as the sender's own domain vouches for
 * it (OrgBadge). A sender that does not verify is still shown, so the learner
 * knows it arrived, but nothing on its card can be answered: the backend
 * refuses anyway, and a button that only fails would look like a fault.
 *
 * There is still no decline for an access request. "Not now" hides it here
 * for this session and tells nobody; a request left alone expires. A refusal
 * button would turn consent into a record of who said no to whom.
 */
import { computed, onMounted, onUnmounted, ref } from 'vue'
import { useRouter } from 'vue-router'
import { useI18n } from 'vue-i18n'
import { useLocalApi } from '@/composables/useLocalApi'
import { AppButton } from '@/components/ui'
import OrgBadge from './OrgBadge.vue'
import { bloomName } from '@/utils/bloom'
import type {
  AccessRequestItem,
  CloudPull,
  GrantView,
  HiringRecord,
  OrgCheck,
  OrgCredentialRequest,
  OrgInterview,
  OrgOffer,
  ShareableCredential,
  SignedCredentialShare,
  SkillChoice,
} from '@/types'

const emit = defineEmits<{ shared: [grant: GrantView] }>()

/** An Alexandria-hosted interview can be joined from this long before it starts. */
const JOIN_EARLY_MS = 15 * 60 * 1000
/** …and is still listed as upcoming for this long after its start time. */
const JOIN_LATE_MS = 3 * 60 * 60 * 1000

type Entry =
  | { kind: 'access'; key: string; created: number; expires: number; org: OrgCheck; item: AccessRequestItem }
  | { kind: 'assessment'; key: string; created: number; expires: number; org: OrgCheck; item: OrgCredentialRequest }
  | { kind: 'interview'; key: string; created: number; expires: number; org: OrgCheck; item: OrgInterview }
  | { kind: 'offer'; key: string; created: number; expires: number; org: OrgCheck; item: OrgOffer }

const { invoke } = useLocalApi()
const { t, locale } = useI18n()
const router = useRouter()

const access = ref<AccessRequestItem[]>([])
const assessments = ref<OrgCredentialRequest[]>([])
const interviews = ref<OrgInterview[]>([])
const offers = ref<OrgOffer[]>([])
const history = ref<HiringRecord[]>([])
const problems = ref<string[]>([])
const loading = ref(false)
const busy = ref('')
const notice = ref('')
/** Failures belong on the card that caused them, not at the top of a long list. */
const cardError = ref<Record<string, string>>({})
const picked = ref<Record<string, string[]>>({})
const skillNames = ref<Record<string, string>>({})

// Assessment request being answered with an existing credential.
const selected = ref('')
const credentials = ref<ShareableCredential[]>([])
const credentialId = ref('')
const preview = ref<SignedCredentialShare | null>(null)
const receipts = ref<Record<string, string>>({})

// Interview / offer answers.
const slot = ref<Record<string, number | ''>>({})
const notes = ref<Record<string, string>>({})

const now = ref(Date.now())
let clock: ReturnType<typeof setInterval> | undefined
/** Guards against a late answer landing after a newer one. Separate counters, so
 *  picking a credential cannot strand a refresh half-way (or the reverse). */
let loadRevision = 0
let pickRevision = 0

const entries = computed<Entry[]>(() => {
  const all: Entry[] = [
    ...access.value
      .filter((item) => !notNow.value.has(`access:${item.request.id}`))
      .map((item) => ({
        kind: 'access' as const, key: `access:${item.request.id}`, created: item.request.created_at,
        expires: item.request.expires_at, org: item.org, item,
      })),
    ...assessments.value.map((item) => ({
      kind: 'assessment' as const, key: `assessment:${item.request.id}`, created: item.request.created_at,
      expires: item.request.expires_at, org: item.org, item,
    })),
    ...interviews.value.map((item) => ({
      kind: 'interview' as const, key: `interview:${item.invite.id}`, created: item.invite.created_at,
      expires: item.invite.expires_at, org: item.org, item,
    })),
    ...offers.value.map((item) => ({
      kind: 'offer' as const, key: `offer:${item.offer.id}`, created: item.offer.created_at,
      expires: item.offer.expires_at, org: item.org, item,
    })),
  ]
  return all.sort((a, b) => b.created - a.created)
})

function alexandriaHosted(record: HiringRecord): boolean {
  try {
    return JSON.parse(record.payload_json)?.response?.invite?.mode === 'alexandria'
  } catch {
    return false
  }
}

/** Accepted interviews held inside Alexandria, soonest first. */
const upcoming = computed(() =>
  history.value
    .filter((r) => r.kind === 'interview' && r.decision === 'accept' && r.chosen_slot !== null)
    .filter((r) => r.chosen_slot! * 1000 + JOIN_LATE_MS > now.value)
    .filter(alexandriaHosted)
    .sort((a, b) => a.chosen_slot! - b.chosen_slot!),
)

const canJoin = (record: HiringRecord) => now.value >= record.chosen_slot! * 1000 - JOIN_EARLY_MS

function join(record: HiringRecord) {
  void router.push({ path: `/interview-room/${record.id}`, query: { cloud: record.directory_url } })
}

function when(unix: number): string {
  return new Intl.DateTimeFormat(locale.value, { dateStyle: 'medium', timeStyle: 'short' }).format(new Date(unix * 1000))
}

function whenIso(iso: string): string {
  const parsed = new Date(iso)
  if (Number.isNaN(parsed.getTime())) return iso
  return new Intl.DateTimeFormat(locale.value, { dateStyle: 'medium', timeStyle: 'short' }).format(parsed)
}

const bloomLabel = (rank: number) => t(`common.bloom.${bloomName(rank)}`)

function skillDetail(skill: SkillChoice): string {
  if (!skill.hasEvidence) return t('organisations.requests.access.noEvidence')
  const parts: string[] = []
  if (skill.bloomLevel !== null) parts.push(bloomLabel(skill.bloomLevel))
  if (skill.level !== null) parts.push(t('organisations.requests.access.score', { level: skill.level }))
  return parts.join(' · ')
}

function modeLabel(mode: string): string {
  switch (mode) {
    case 'call': return t('organisations.requests.interview.mode.call')
    case 'in_person': return t('organisations.requests.interview.mode.inPerson')
    case 'alexandria': return t('organisations.requests.interview.mode.alexandria')
    default: return t('organisations.requests.interview.mode.video')
  }
}

function setError(key: string, error: unknown) {
  cardError.value = { ...cardError.value, [key]: String(error) }
}

/**
 * Ask for all four kinds at once. Each is settled on its own: one that fails
 * becomes a line under the inbox instead of hiding the three that answered.
 * Problems are deduplicated, since one unreachable Cloud fails every call.
 */
async function refresh() {
  const generation = ++loadRevision
  loading.value = true
  const [a, r, i, o, h] = await Promise.allSettled([
    invoke<CloudPull<AccessRequestItem>>('org_access_requests'),
    invoke<CloudPull<OrgCredentialRequest>>('exchange_requests'),
    invoke<CloudPull<OrgInterview>>('hiring_interviews'),
    invoke<CloudPull<OrgOffer>>('hiring_offers'),
    invoke<HiringRecord[]>('hiring_history'),
  ])
  if (generation !== loadRevision) return
  const found: string[] = []
  const take = <T,>(result: PromiseSettledResult<CloudPull<T>>): T[] => {
    if (result.status === 'rejected') {
      found.push(String(result.reason))
      return []
    }
    found.push(...result.value.problems.map((p) => p.detail))
    return result.value.items
  }
  access.value = take(a)
  assessments.value = take(r)
  interviews.value = take(i)
  offers.value = take(o)
  if (h.status === 'fulfilled') history.value = h.value
  else found.push(String(h.reason))
  problems.value = [...new Set(found)]

  for (const item of access.value) {
    const key = `access:${item.request.id}`
    if (!picked.value[key]) picked.value[key] = item.skills.filter((s) => s.hasEvidence).map((s) => s.skillId)
  }
  void nameSkills()
  loading.value = false
}

/** Assessment requests carry only a skill id. Best effort: the id stands in. */
async function nameSkills() {
  for (const id of new Set(assessments.value.map((item) => item.request.skill_id))) {
    if (skillNames.value[id]) continue
    try {
      const detail = await invoke<{ skill: { name: string } } | null>('get_skill', { skillId: id })
      if (detail?.skill.name) skillNames.value = { ...skillNames.value, [id]: detail.skill.name }
    } catch {
      /* the id is shown instead */
    }
  }
}

const skillName = (id: string) => skillNames.value[id] ?? id

function togglePick(key: string, skillId: string) {
  const current = picked.value[key] ?? []
  picked.value = {
    ...picked.value,
    [key]: current.includes(skillId) ? current.filter((s) => s !== skillId) : [...current, skillId],
  }
}

async function share(entry: Extract<Entry, { kind: 'access' }>, sync: boolean) {
  if (busy.value) return
  busy.value = entry.key
  setError(entry.key, '')
  notice.value = ''
  try {
    const grant = await invoke<GrantView>('org_grant', {
      directoryUrl: entry.item.directoryUrl,
      request: entry.item.request,
      orgProof: entry.item.orgProof,
      // Only what is ticked. The backend checks it is a subset of what was
      // asked, so the organisation cannot widen the answer by asking for more.
      skillIds: picked.value[entry.key] ?? [],
      sync,
    })
    notice.value = sync
      ? t('organisations.requests.access.sharedSync', { org: grant.orgName })
      : t('organisations.requests.access.shared', { org: grant.orgName })
    emit('shared', grant)
    await refresh()
  } catch (e) {
    setError(entry.key, e)
  } finally {
    busy.value = ''
  }
}

function hide(entry: Entry) {
  notNow.value = new Set([...notNow.value, entry.key])
}

function startAssessment(item: OrgCredentialRequest) {
  void router.push({
    path: `/assessment/${encodeURIComponent(item.request.skill_id)}`,
    query: { request: item.request.id, directory: item.directory_url },
  })
}

async function chooseCredential(entry: Extract<Entry, { kind: 'assessment' }>) {
  const generation = ++pickRevision
  selected.value = entry.key
  credentials.value = []
  credentialId.value = ''
  preview.value = null
  setError(entry.key, '')
  try {
    const found = await invoke<ShareableCredential[]>('exchange_credentials', { request: entry.item.request })
    if (generation === pickRevision) credentials.value = found
  } catch (e) {
    if (generation === pickRevision) setError(entry.key, e)
  }
}

async function showPreview(entry: Extract<Entry, { kind: 'assessment' }>) {
  if (!credentialId.value) return
  const generation = ++pickRevision
  busy.value = entry.key
  preview.value = null
  setError(entry.key, '')
  try {
    const result = await invoke<SignedCredentialShare>('exchange_preview', {
      request: entry.item.request,
      orgProof: entry.item.orgProof,
      credentialId: credentialId.value,
    })
    if (generation === pickRevision) preview.value = result
  } catch (e) {
    setError(entry.key, e)
  } finally {
    busy.value = ''
  }
}

async function sendCredential(entry: Extract<Entry, { kind: 'assessment' }>) {
  if (!preview.value || busy.value) return
  busy.value = entry.key
  setError(entry.key, '')
  try {
    const result = await invoke<{ received: boolean; verification: { acceptanceDecision: string } }>('exchange_send', {
      directoryUrl: entry.item.directory_url,
      signed: preview.value,
    })
    if (result.received) receipts.value = { ...receipts.value, [entry.key]: result.verification.acceptanceDecision }
  } catch (e) {
    setError(entry.key, e)
  } finally {
    busy.value = ''
  }
}

async function answerInterview(entry: Extract<Entry, { kind: 'interview' }>, decision: 'accept' | 'decline') {
  if (busy.value) return
  const chosen = slot.value[entry.key]
  if (decision === 'accept' && (chosen === undefined || chosen === '')) return
  busy.value = entry.key
  setError(entry.key, '')
  try {
    const result = await invoke<{ received: boolean; status: string }>('hiring_interview_respond', {
      directoryUrl: entry.item.directory_url,
      invite: entry.item.invite,
      orgProof: entry.item.orgProof,
      decision,
      chosenSlot: decision === 'accept' ? chosen : null,
      note: notes.value[entry.key] || null,
    })
    if (result.received) receipts.value = { ...receipts.value, [entry.key]: result.status }
    await refresh()
  } catch (e) {
    setError(entry.key, e)
  } finally {
    busy.value = ''
  }
}

async function answerOffer(entry: Extract<Entry, { kind: 'offer' }>, decision: 'accept' | 'decline') {
  if (busy.value) return
  busy.value = entry.key
  setError(entry.key, '')
  try {
    const result = await invoke<{ received: boolean; status: string }>('hiring_offer_respond', {
      directoryUrl: entry.item.directory_url,
      offer: entry.item.offer,
      orgProof: entry.item.orgProof,
      decision,
      note: notes.value[entry.key] || null,
    })
    if (result.received) receipts.value = { ...receipts.value, [entry.key]: result.status }
    await refresh()
  } catch (e) {
    setError(entry.key, e)
  } finally {
    busy.value = ''
  }
}

defineExpose({ refresh })

onMounted(() => {
  clock = setInterval(() => { now.value = Date.now() }, 30_000)
  void refresh()
})
onUnmounted(() => {
  loadRevision++
  pickRevision++
  if (clock) clearInterval(clock)
})
</script>

<template>
  <section class="space-y-4 rounded-xl border border-border bg-card p-5" data-testid="org-requests">
    <header class="flex items-start justify-between gap-3">
      <div>
        <h2 class="text-base font-semibold text-foreground">{{ t('organisations.requests.title') }}</h2>
        <p class="mt-1 text-sm text-muted-foreground">{{ t('organisations.requests.note') }}</p>
      </div>
      <AppButton variant="outline" size="sm" class="shrink-0 whitespace-nowrap" :disabled="loading" @click="refresh">
        {{ loading ? t('organisations.requests.checking') : t('organisations.requests.refresh') }}
      </AppButton>
    </header>

    <p v-if="notice" role="status" class="text-sm text-success">{{ notice }}</p>

    <!-- Interviews held inside Alexandria that the learner has accepted. -->
    <div v-if="upcoming.length" class="space-y-2" data-testid="upcoming">
      <h3 class="text-sm font-medium text-foreground">{{ t('organisations.requests.upcoming.title') }}</h3>
      <div
        v-for="record in upcoming"
        :key="record.id"
        class="flex flex-wrap items-center gap-3 rounded-lg border border-primary/30 bg-primary/5 p-3"
        data-testid="upcoming-interview"
      >
        <span class="min-w-0 flex-1">
          <span class="block truncate text-sm font-medium text-foreground">{{ record.organization }} · {{ record.role_label }}</span>
          <span class="block text-xs text-muted-foreground">{{ when(record.chosen_slot!) }}</span>
        </span>
        <AppButton v-if="canJoin(record)" size="sm" @click="join(record)">
          {{ t('organisations.requests.upcoming.join') }}
        </AppButton>
        <span v-else class="text-xs text-muted-foreground">{{ t('organisations.requests.upcoming.opensSoon') }}</span>
      </div>
    </div>

    <p v-if="!loading && !entries.length" class="text-sm text-muted-foreground">
      {{ t('organisations.requests.empty') }}
    </p>

    <article
      v-for="entry in entries"
      :key="entry.key"
      class="space-y-3 rounded-lg border border-border p-4"
      :data-testid="`request-${entry.kind}`"
    >
      <div class="flex flex-wrap items-start justify-between gap-2">
        <OrgBadge :org="entry.org" />
        <span class="shrink-0 rounded-full bg-muted px-2 py-0.5 text-xs text-muted-foreground">
          {{ t(`organisations.requests.kind.${entry.kind}`) }}
        </span>
      </div>

      <!-- ── Access request ── -->
      <template v-if="entry.kind === 'access'">
        <p class="text-sm text-foreground">{{ t('organisations.requests.access.purpose', { purpose: entry.item.request.purpose }) }}</p>
        <fieldset class="space-y-2">
          <legend class="text-xs font-medium text-muted-foreground">{{ t('organisations.requests.access.skillsHeading') }}</legend>
          <label
            v-for="skill in entry.item.skills"
            :key="skill.skillId"
            class="flex items-center gap-3 rounded-lg border border-border p-2"
            :class="skill.hasEvidence && entry.org.verified ? 'cursor-pointer' : 'opacity-60'"
          >
            <input
              type="checkbox"
              class="h-4 w-4 shrink-0"
              :checked="(picked[entry.key] ?? []).includes(skill.skillId)"
              :disabled="!skill.hasEvidence || !entry.org.verified || !!busy"
              @change="togglePick(entry.key, skill.skillId)"
            />
            <span class="min-w-0 flex-1">
              <span class="block truncate text-sm text-foreground">{{ skill.name }}</span>
              <span class="block text-xs text-muted-foreground">{{ skillDetail(skill) }}</span>
            </span>
          </label>
        </fieldset>
        <div class="flex flex-wrap gap-2">
          <AppButton
            size="sm"
            :disabled="!entry.org.verified || !!busy || !(picked[entry.key] ?? []).length"
            @click="share(entry, false)"
          >
            {{ busy === entry.key ? t('organisations.requests.access.sharing') : t('organisations.requests.access.share') }}
          </AppButton>
          <AppButton
            size="sm"
            variant="outline"
            :disabled="!entry.org.verified || !!busy || !(picked[entry.key] ?? []).length"
            @click="share(entry, true)"
          >
            {{ t('organisations.requests.access.shareSync') }}
          </AppButton>
          <AppButton size="sm" variant="ghost" :disabled="!!busy" @click="hide(entry)">
            {{ t('organisations.requests.access.notNow') }}
          </AppButton>
        </div>
        <p class="text-xs text-muted-foreground">{{ t('organisations.requests.access.syncNote') }}</p>
        <p class="text-xs text-muted-foreground">{{ t('organisations.requests.access.notNowNote') }}</p>
      </template>

      <!-- ── Assessment request ── -->
      <template v-else-if="entry.kind === 'assessment'">
        <p class="text-sm font-medium text-foreground">{{ entry.item.request.role_label }}</p>
        <p class="text-sm text-foreground">{{ entry.item.request.purpose }}</p>
        <p class="text-xs text-muted-foreground">{{ t('organisations.requests.assessment.skill', { skill: skillName(entry.item.request.skill_id) }) }}</p>
        <div class="flex flex-wrap gap-2">
          <AppButton
            v-if="entry.item.request.require_new_assessment"
            size="sm"
            :disabled="!entry.org.verified || !!busy"
            @click="startAssessment(entry.item)"
          >
            {{ t('organisations.requests.assessment.assess') }}
          </AppButton>
          <AppButton size="sm" variant="outline" :disabled="!entry.org.verified || !!busy" @click="chooseCredential(entry)">
            {{ t('organisations.requests.assessment.choose') }}
          </AppButton>
        </div>
        <div v-if="selected === entry.key" class="space-y-3 rounded-lg bg-muted/40 p-3" data-testid="credential-picker">
          <p v-if="!credentials.length" class="text-sm text-muted-foreground">{{ t('organisations.requests.assessment.noCredentials') }}</p>
          <select
            v-else
            v-model="credentialId"
            class="w-full rounded border border-border bg-background p-2 text-sm"
            :aria-label="t('organisations.requests.assessment.choose')"
            :disabled="!!busy"
            @change="preview = null"
          >
            <option value="">{{ t('organisations.requests.assessment.choose') }}</option>
            <option v-for="credential in credentials" :key="credential.id" :value="credential.id">{{ credential.id }} · {{ whenIso(credential.issued_at) }}</option>
          </select>
          <AppButton size="sm" :disabled="!credentialId || !!busy || !entry.org.verified" @click="showPreview(entry)">
            {{ t('organisations.requests.assessment.preview') }}
          </AppButton>
          <template v-if="preview">
            <p class="text-sm text-foreground">{{ t('organisations.requests.assessment.disclosure') }}</p>
            <details>
              <summary class="cursor-pointer text-xs text-muted-foreground">{{ t('organisations.requests.assessment.exact') }}</summary>
              <pre class="max-h-80 overflow-auto whitespace-pre-wrap break-all text-xs">{{ JSON.stringify(preview, null, 2) }}</pre>
            </details>
            <AppButton v-if="!receipts[entry.key]" size="sm" :disabled="!!busy || !entry.org.verified" @click="sendCredential(entry)">
              {{ t('organisations.requests.assessment.share') }}
            </AppButton>
            <p v-else role="status" class="text-sm text-foreground">
              {{ t('organisations.requests.assessment.received', { outcome: receipts[entry.key] }) }}
            </p>
          </template>
        </div>
      </template>

      <!-- ── Interview invitation ── -->
      <template v-else-if="entry.kind === 'interview'">
        <p class="text-sm font-medium text-foreground">{{ entry.item.invite.role_label }} · {{ modeLabel(entry.item.invite.mode) }}</p>
        <p class="whitespace-pre-wrap text-sm text-foreground">{{ entry.item.invite.message }}</p>
        <p class="text-xs text-muted-foreground">
          <span>{{ t('organisations.requests.interview.where') }}: </span>
          <span v-if="entry.item.invite.mode === 'alexandria'">{{ t('organisations.requests.interview.inAlexandria') }}</span>
          <a
            v-else-if="entry.item.invite.meeting_url"
            :href="entry.item.invite.meeting_url"
            target="_blank"
            rel="noopener"
            class="text-primary underline"
          >{{ entry.item.invite.meeting_url }}</a>
          <span v-else>{{ t('organisations.requests.interview.toBeArranged') }}</span>
          <span v-if="entry.item.invite.run_id"> · {{ t('organisations.requests.interview.followsRun') }}</span>
        </p>
        <p v-if="entry.item.invite.integrity_check" class="text-xs text-muted-foreground" data-testid="invite-integrity">
          {{ t('organisations.requests.interview.integrityAsked') }}
        </p>
        <template v-if="!receipts[entry.key]">
          <label class="block text-sm text-foreground">{{ t('organisations.requests.interview.chooseTime') }}
            <select
              v-model="slot[entry.key]"
              class="mt-1 w-full rounded border border-border bg-background p-2"
              :disabled="!!busy || !entry.org.verified"
            >
              <option value="">{{ t('organisations.requests.interview.chooseTime') }}</option>
              <option v-for="s in entry.item.invite.proposed_slots" :key="s" :value="s">{{ when(s) }}</option>
            </select>
          </label>
          <label class="block text-sm text-foreground">{{ t('organisations.requests.noteLabel') }}
            <input
              v-model="notes[entry.key]"
              class="mt-1 w-full rounded border border-border bg-background p-2"
              maxlength="2000"
              :disabled="!!busy || !entry.org.verified"
            >
          </label>
          <div class="flex flex-wrap gap-2">
            <AppButton
              size="sm"
              :disabled="!!busy || !entry.org.verified || slot[entry.key] === undefined || slot[entry.key] === ''"
              @click="answerInterview(entry, 'accept')"
            >
              {{ t('organisations.requests.interview.accept') }}
            </AppButton>
            <AppButton size="sm" variant="outline" :disabled="!!busy || !entry.org.verified" @click="answerInterview(entry, 'decline')">
              {{ t('organisations.requests.interview.decline') }}
            </AppButton>
          </div>
        </template>
        <p v-else role="status" class="text-sm text-foreground">{{ t('organisations.requests.answered', { status: receipts[entry.key] }) }}</p>
      </template>

      <!-- ── Offer ── -->
      <template v-else-if="entry.kind === 'offer'">
        <p class="text-sm font-medium text-foreground">{{ entry.item.offer.role_label }}</p>
        <p class="whitespace-pre-wrap text-sm text-foreground">{{ entry.item.offer.terms }}</p>
        <p v-if="entry.item.offer.start_date" class="text-xs text-muted-foreground">
          {{ t('organisations.requests.offer.starts', { date: entry.item.offer.start_date }) }}
        </p>
        <template v-if="!receipts[entry.key]">
          <label class="block text-sm text-foreground">{{ t('organisations.requests.noteLabel') }}
            <input
              v-model="notes[entry.key]"
              class="mt-1 w-full rounded border border-border bg-background p-2"
              maxlength="2000"
              :disabled="!!busy || !entry.org.verified"
            >
          </label>
          <div class="flex flex-wrap gap-2">
            <AppButton size="sm" :disabled="!!busy || !entry.org.verified" @click="answerOffer(entry, 'accept')">
              {{ t('organisations.requests.offer.accept') }}
            </AppButton>
            <AppButton size="sm" variant="outline" :disabled="!!busy || !entry.org.verified" @click="answerOffer(entry, 'decline')">
              {{ t('organisations.requests.offer.decline') }}
            </AppButton>
          </div>
        </template>
        <p v-else role="status" class="text-sm text-foreground">{{ t('organisations.requests.answered', { status: receipts[entry.key] }) }}</p>
      </template>

      <p class="text-xs text-muted-foreground">{{ t('organisations.requests.expires', { when: when(entry.expires) }) }}</p>
      <p v-if="cardError[entry.key]" role="alert" class="text-sm text-destructive">{{ cardError[entry.key] }}</p>
    </article>

    <details v-if="history.length" data-testid="history">
      <summary class="cursor-pointer text-sm font-medium text-foreground">
        {{ t('organisations.requests.history.title', { count: history.length }) }}
      </summary>
      <ul class="mt-2 space-y-2 text-sm">
        <li v-for="record in history" :key="`${record.kind}:${record.id}`" class="rounded border border-border p-3">
          <span class="font-medium">{{ record.organization }} · {{ record.role_label }}</span>
          · {{ record.kind === 'interview' ? t('organisations.requests.kind.interview') : t('organisations.requests.kind.offer') }}
          · {{ record.decision === 'accept' ? t('organisations.requests.history.accepted') : t('organisations.requests.history.declined') }}
          <span v-if="record.chosen_slot"> · {{ when(record.chosen_slot) }}</span>
          <a
            v-if="record.meeting_url && record.decision === 'accept'"
            :href="record.meeting_url"
            target="_blank"
            rel="noopener"
            class="ms-1 text-primary underline"
          >{{ t('organisations.requests.interview.meetingLink') }}</a>
          <span class="text-muted-foreground"> · {{ whenIso(record.responded_at) }}</span>
        </li>
      </ul>
    </details>

    <div v-if="problems.length" class="space-y-1" data-testid="request-problems">
      <h3 class="text-sm font-medium text-foreground">{{ t('organisations.problems.heading') }}</h3>
      <p v-for="p in problems" :key="p" role="alert" class="text-sm text-muted-foreground">{{ p }}</p>
      <p class="text-xs text-muted-foreground">{{ t('organisations.problems.note') }}</p>
    </div>
  </section>
</template>
