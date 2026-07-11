<script setup lang="ts">
/**
 * Sponsor dashboard — enterprise organizations + role/JD assessments
 * (productization P2). A sponsor creates an organization, authors
 * role-specific assessments tied to a job description with an issuance
 * policy + required assurance level, and issues gated RoleCredentials to
 * candidates from a completed integrity session.
 */
import { ref, computed, onMounted } from 'vue'
import { useI18n } from 'vue-i18n'
import { AppButton, AppBadge, AppModal, AppInput, AppTextarea, AppTabs, EmptyState, AppAlert } from '@/components/ui'
import { useSponsor } from '@/composables/useSponsor'
import { useAuth } from '@/composables/useAuth'
import type { IssuancePolicy, VerifiableCredential } from '@/types'

const { t } = useI18n()
const sponsor = useSponsor()
const { stakeAddress } = useAuth()

const tab = ref<'orgs' | 'roles' | 'issue'>('orgs')
const tabs = computed(() => [
  { key: 'orgs', label: t('dashboard.sponsor.tabs.orgs') },
  { key: 'roles', label: t('dashboard.sponsor.tabs.roles') },
  { key: 'issue', label: t('dashboard.sponsor.tabs.issue') },
])

const ASSURANCE_LEVELS = ['local', 'anchored', 'high_assurance']

// --- Organizations -------------------------------------------------------
const orgModalOpen = ref(false)
const orgName = ref('')
const orgBusy = ref(false)

async function createOrg() {
  if (!orgName.value.trim() || !stakeAddress.value) return
  orgBusy.value = true
  const org = await sponsor.createOrganization(orgName.value.trim(), stakeAddress.value)
  orgBusy.value = false
  if (org) {
    orgName.value = ''
    orgModalOpen.value = false
  }
}

// --- Role assessments ----------------------------------------------------
const roleModalOpen = ref(false)
const roleForm = ref({
  org_id: '',
  role_title: '',
  job_description: '',
  skill_ids: '',
  min_integrity: '',
  max_critical: '',
  max_warning: '',
  require_clean: false,
  required_assurance_level: '',
})
const roleBusy = ref(false)

function openRoleModal() {
  roleForm.value.org_id = sponsor.organizations.value[0]?.id ?? ''
  roleModalOpen.value = true
}

function buildPolicy(): IssuancePolicy | null {
  const p: IssuancePolicy = {}
  if (roleForm.value.min_integrity !== '') p.min_integrity = Number(roleForm.value.min_integrity)
  if (roleForm.value.max_critical !== '') p.max_critical = Number(roleForm.value.max_critical)
  if (roleForm.value.max_warning !== '') p.max_warning = Number(roleForm.value.max_warning)
  if (roleForm.value.require_clean) p.require_clean = true
  // required_assurance_level is folded in backend-side from the column.
  return Object.keys(p).length > 0 ? p : null
}

async function createRole() {
  const f = roleForm.value
  if (!f.org_id || !f.role_title.trim()) return
  roleBusy.value = true
  const ra = await sponsor.createRoleAssessment({
    org_id: f.org_id,
    role_title: f.role_title.trim(),
    job_description: f.job_description.trim() || null,
    skill_ids: f.skill_ids.split(',').map(s => s.trim()).filter(Boolean),
    issuance_policy: buildPolicy(),
    required_assurance_level: f.required_assurance_level || null,
  })
  roleBusy.value = false
  if (ra) {
    roleForm.value = {
      org_id: '', role_title: '', job_description: '', skill_ids: '',
      min_integrity: '', max_critical: '', max_warning: '',
      require_clean: false, required_assurance_level: '',
    }
    roleModalOpen.value = false
  }
}

function orgName_(id: string) {
  return sponsor.organizations.value.find(o => o.id === id)?.name ?? id.slice(0, 8)
}

// --- Issue ---------------------------------------------------------------
const issueForm = ref({ role_assessment_id: '', subject: '', integrity_session_id: '' })
const issueBusy = ref(false)
const issueResult = ref<VerifiableCredential | null>(null)
const issueError = ref<string | null>(null)

async function issue() {
  const f = issueForm.value
  if (!f.role_assessment_id || !f.subject.trim() || !f.integrity_session_id.trim()) return
  issueBusy.value = true
  issueError.value = null
  issueResult.value = null
  const vc = await sponsor.issueRoleCredential(
    f.role_assessment_id, f.subject.trim(), f.integrity_session_id.trim(),
  )
  issueBusy.value = false
  if (vc) issueResult.value = vc
  else issueError.value = sponsor.error.value
}

const publishedRoles = computed(() => sponsor.roleAssessments.value.filter(r => r.status !== 'archived'))

function assuranceTone(level?: string | null): 'success' | 'accent' | 'secondary' {
  if (level === 'high_assurance') return 'success'
  if (level === 'anchored') return 'accent'
  return 'secondary'
}

onMounted(async () => {
  await sponsor.listOrganizations()
  await sponsor.listRoleAssessments()
})
</script>

<template>
  <div class="mx-auto max-w-4xl p-4">
    <div class="mb-4">
      <h1 class="text-lg font-semibold text-foreground">{{ $t('dashboard.sponsor.title') }}</h1>
      <p class="text-sm text-muted-foreground">
        {{ $t('dashboard.sponsor.subtitle') }}
      </p>
    </div>

    <AppAlert v-if="sponsor.error.value" variant="error" class="mb-3">{{ sponsor.error.value }}</AppAlert>

    <AppTabs :tabs="tabs" :model-value="tab" class="mb-4" @update:model-value="tab = $event as typeof tab" />

    <!-- Organizations ------------------------------------------------- -->
    <section v-if="tab === 'orgs'">
      <div class="mb-3 flex justify-end">
        <AppButton variant="primary" size="sm" @click="orgModalOpen = true">{{ $t('dashboard.sponsor.orgs.new') }}</AppButton>
      </div>
      <EmptyState v-if="!sponsor.organizations.value.length" :title="$t('dashboard.sponsor.orgs.emptyTitle')" :description="$t('dashboard.sponsor.orgs.emptyBody')" />
      <div v-else class="grid gap-3">
        <div v-for="org in sponsor.organizations.value" :key="org.id" class="rounded-lg border border-border bg-card p-3">
          <div class="flex items-center justify-between">
            <span class="font-medium text-foreground">{{ org.name }}</span>
            <span class="font-mono text-xs text-muted-foreground">{{ org.id.slice(0, 12) }}</span>
          </div>
          <p class="mt-1 truncate text-xs text-muted-foreground">{{ $t('dashboard.sponsor.orgs.owner') }}: {{ org.owner_address }}</p>
        </div>
      </div>
    </section>

    <!-- Role assessments ---------------------------------------------- -->
    <section v-else-if="tab === 'roles'">
      <div class="mb-3 flex justify-end">
        <AppButton variant="primary" size="sm" :disabled="!sponsor.organizations.value.length" @click="openRoleModal">
          {{ $t('dashboard.sponsor.roles.new') }}
        </AppButton>
      </div>
      <EmptyState v-if="!sponsor.roleAssessments.value.length" :title="$t('dashboard.sponsor.roles.emptyTitle')" :description="$t('dashboard.sponsor.roles.emptyBody')" />
      <div v-else class="grid gap-3">
        <div v-for="ra in sponsor.roleAssessments.value" :key="ra.id" class="rounded-lg border border-border bg-card p-3">
          <div class="flex items-center justify-between">
            <span class="font-medium text-foreground">{{ ra.role_title }}</span>
            <AppBadge :variant="ra.status === 'published' ? 'success' : 'secondary'">{{ ra.status }}</AppBadge>
          </div>
          <p class="text-xs text-muted-foreground">{{ orgName_(ra.org_id) }}</p>
          <p v-if="ra.job_description" class="mt-1 line-clamp-2 text-sm text-muted-foreground">{{ ra.job_description }}</p>
          <div class="mt-2 flex flex-wrap items-center gap-2 text-xs">
            <AppBadge v-if="ra.required_assurance_level" :variant="assuranceTone(ra.required_assurance_level)">
              {{ ra.required_assurance_level }}
            </AppBadge>
            <span v-if="ra.issuance_policy?.require_clean" class="text-muted-foreground">{{ $t('dashboard.sponsor.roles.requiresClean') }}</span>
            <span v-if="ra.issuance_policy?.min_integrity != null" class="text-muted-foreground">
              {{ $t('dashboard.sponsor.roles.minIntegrity', { value: ra.issuance_policy.min_integrity }) }}
            </span>
            <span v-for="s in ra.skill_ids" :key="s" class="rounded bg-muted px-1.5 py-0.5 text-muted-foreground">{{ s }}</span>
          </div>
          <div class="mt-2 flex gap-2">
            <AppButton v-if="ra.status === 'draft'" variant="ghost" size="sm" @click="sponsor.setRoleAssessmentStatus(ra.id, 'published')">{{ $t('dashboard.sponsor.roles.publish') }}</AppButton>
            <AppButton v-if="ra.status !== 'archived'" variant="ghost" size="sm" @click="sponsor.setRoleAssessmentStatus(ra.id, 'archived')">{{ $t('dashboard.sponsor.roles.archive') }}</AppButton>
          </div>
        </div>
      </div>
    </section>

    <!-- Issue --------------------------------------------------------- -->
    <section v-else>
      <div class="grid gap-3 rounded-lg border border-border bg-card p-4">
        <label class="text-xs text-muted-foreground">{{ $t('dashboard.sponsor.issue.roleLabel') }}
          <select v-model="issueForm.role_assessment_id" class="mt-1 w-full rounded-md border border-border bg-background p-2 text-sm">
            <option value="">{{ $t('dashboard.sponsor.issue.selectPlaceholder') }}</option>
            <option v-for="ra in publishedRoles" :key="ra.id" :value="ra.id">{{ ra.role_title }} — {{ orgName_(ra.org_id) }}</option>
          </select>
        </label>
        <AppInput v-model="issueForm.subject" :label="$t('dashboard.sponsor.issue.candidateLabel')" placeholder="did:key:z6Mk…" />
        <AppInput v-model="issueForm.integrity_session_id" :label="$t('dashboard.sponsor.issue.sessionLabel')" placeholder="isess_…" />
        <div>
          <AppButton variant="primary" size="sm" :disabled="issueBusy" @click="issue">
            {{ issueBusy ? $t('dashboard.sponsor.issue.submitting') : $t('dashboard.sponsor.issue.submit') }}
          </AppButton>
        </div>
        <AppAlert v-if="issueError" variant="error">{{ issueError }}</AppAlert>
        <div v-if="issueResult" class="rounded-md border border-emerald-300 bg-emerald-50 p-3 text-sm dark:border-emerald-800/40 dark:bg-emerald-900/20">
          <p class="font-medium text-emerald-700 dark:text-emerald-400">{{ $t('dashboard.sponsor.issue.issued') }}</p>
          <p class="mt-1 font-mono text-xs break-all text-muted-foreground">{{ issueResult.id }}</p>
          <p v-if="issueResult.integrity" class="mt-1 text-xs text-muted-foreground">
            {{ $t('dashboard.sponsor.issue.assurance') }}
            <AppBadge :variant="assuranceTone(issueResult.integrity.assuranceLevel)">{{ issueResult.integrity.assuranceLevel }}</AppBadge>
            · {{ $t('dashboard.sponsor.issue.integrity', { value: issueResult.integrity.integrityScore ?? 'n/a' }) }}
          </p>
        </div>
      </div>
    </section>

    <!-- New org modal -->
    <AppModal :open="orgModalOpen" :title="$t('dashboard.sponsor.orgModal.title')" @close="orgModalOpen = false">
      <div class="grid gap-3">
        <AppInput v-model="orgName" :label="$t('dashboard.sponsor.orgModal.nameLabel')" :placeholder="$t('dashboard.sponsor.orgModal.namePlaceholder')" />
        <div class="flex justify-end gap-2">
          <AppButton variant="ghost" size="sm" @click="orgModalOpen = false">{{ $t('common.actions.cancel') }}</AppButton>
          <AppButton variant="primary" size="sm" :disabled="orgBusy || !orgName.trim()" @click="createOrg">{{ $t('dashboard.sponsor.actions.create') }}</AppButton>
        </div>
      </div>
    </AppModal>

    <!-- New role assessment modal -->
    <AppModal :open="roleModalOpen" :title="$t('dashboard.sponsor.roleModal.title')" @close="roleModalOpen = false">
      <div class="grid max-h-[70vh] gap-3 overflow-y-auto">
        <label class="text-xs text-muted-foreground">{{ $t('dashboard.sponsor.roleModal.orgLabel') }}
          <select v-model="roleForm.org_id" class="mt-1 w-full rounded-md border border-border bg-background p-2 text-sm">
            <option v-for="o in sponsor.organizations.value" :key="o.id" :value="o.id">{{ o.name }}</option>
          </select>
        </label>
        <AppInput v-model="roleForm.role_title" :label="$t('dashboard.sponsor.roleModal.roleTitleLabel')" :placeholder="$t('dashboard.sponsor.roleModal.roleTitlePlaceholder')" />
        <AppTextarea v-model="roleForm.job_description" :label="$t('dashboard.sponsor.roleModal.jobLabel')" :placeholder="$t('dashboard.sponsor.roleModal.jobPlaceholder')" />
        <AppInput v-model="roleForm.skill_ids" :label="$t('dashboard.sponsor.roleModal.skillsLabel')" placeholder="skill:sre, skill:linux" />
        <p class="text-xs font-medium text-foreground">{{ $t('dashboard.sponsor.roleModal.policy') }}</p>
        <div class="grid grid-cols-3 gap-2">
          <AppInput v-model="roleForm.min_integrity" :label="$t('dashboard.sponsor.roleModal.minIntegrity')" placeholder="0.70" type="number" />
          <AppInput v-model="roleForm.max_critical" :label="$t('dashboard.sponsor.roleModal.maxCritical')" placeholder="0" type="number" />
          <AppInput v-model="roleForm.max_warning" :label="$t('dashboard.sponsor.roleModal.maxWarning')" placeholder="2" type="number" />
        </div>
        <label class="flex items-center gap-2 text-sm text-foreground">
          <input v-model="roleForm.require_clean" type="checkbox" /> {{ $t('dashboard.sponsor.roleModal.requireClean') }}
        </label>
        <label class="text-xs text-muted-foreground">{{ $t('dashboard.sponsor.roleModal.assuranceLabel') }}
          <select v-model="roleForm.required_assurance_level" class="mt-1 w-full rounded-md border border-border bg-background p-2 text-sm">
            <option value="">{{ $t('dashboard.sponsor.roleModal.assuranceNone') }}</option>
            <option v-for="l in ASSURANCE_LEVELS" :key="l" :value="l">{{ l }}</option>
          </select>
        </label>
        <div class="flex justify-end gap-2">
          <AppButton variant="ghost" size="sm" @click="roleModalOpen = false">{{ $t('common.actions.cancel') }}</AppButton>
          <AppButton variant="primary" size="sm" :disabled="roleBusy || !roleForm.role_title.trim() || !roleForm.org_id" @click="createRole">{{ $t('dashboard.sponsor.actions.create') }}</AppButton>
        </div>
      </div>
    </AppModal>
  </div>
</template>
