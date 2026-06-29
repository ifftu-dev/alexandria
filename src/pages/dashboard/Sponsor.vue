<script setup lang="ts">
/**
 * Sponsor dashboard — enterprise organizations + role/JD assessments
 * (productization P2). A sponsor creates an organization, authors
 * role-specific assessments tied to a job description with an issuance
 * policy + required assurance level, and issues gated RoleCredentials to
 * candidates from a completed integrity session.
 */
import { ref, computed, onMounted } from 'vue'
import { AppButton, AppBadge, AppModal, AppInput, AppTextarea, AppTabs, EmptyState, AppAlert } from '@/components/ui'
import { useSponsor } from '@/composables/useSponsor'
import { useAuth } from '@/composables/useAuth'
import type { IssuancePolicy, VerifiableCredential } from '@/types'

const sponsor = useSponsor()
const { stakeAddress } = useAuth()

const tab = ref<'orgs' | 'roles' | 'issue'>('orgs')
const tabs = [
  { key: 'orgs', label: 'Organizations' },
  { key: 'roles', label: 'Role Assessments' },
  { key: 'issue', label: 'Issue Credential' },
]

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
      <h1 class="text-lg font-semibold text-foreground">Sponsor</h1>
      <p class="text-sm text-muted-foreground">
        Define role-specific, job-description-based assessments and issue trusted credentials gated on assessment integrity.
      </p>
    </div>

    <AppAlert v-if="sponsor.error.value" variant="error" class="mb-3">{{ sponsor.error.value }}</AppAlert>

    <AppTabs :tabs="tabs" :model-value="tab" class="mb-4" @update:model-value="tab = $event as typeof tab" />

    <!-- Organizations ------------------------------------------------- -->
    <section v-if="tab === 'orgs'">
      <div class="mb-3 flex justify-end">
        <AppButton variant="primary" size="sm" @click="orgModalOpen = true">New Organization</AppButton>
      </div>
      <EmptyState v-if="!sponsor.organizations.value.length" title="No organizations yet" description="Create one to start authoring role assessments." />
      <div v-else class="grid gap-3">
        <div v-for="org in sponsor.organizations.value" :key="org.id" class="rounded-lg border border-border bg-card p-3">
          <div class="flex items-center justify-between">
            <span class="font-medium text-foreground">{{ org.name }}</span>
            <span class="font-mono text-xs text-muted-foreground">{{ org.id.slice(0, 12) }}</span>
          </div>
          <p class="mt-1 truncate text-xs text-muted-foreground">owner: {{ org.owner_address }}</p>
        </div>
      </div>
    </section>

    <!-- Role assessments ---------------------------------------------- -->
    <section v-else-if="tab === 'roles'">
      <div class="mb-3 flex justify-end">
        <AppButton variant="primary" size="sm" :disabled="!sponsor.organizations.value.length" @click="openRoleModal">
          New Role Assessment
        </AppButton>
      </div>
      <EmptyState v-if="!sponsor.roleAssessments.value.length" title="No role assessments" description="Author one against an organization." />
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
            <span v-if="ra.issuance_policy?.require_clean" class="text-muted-foreground">requires clean</span>
            <span v-if="ra.issuance_policy?.min_integrity != null" class="text-muted-foreground">
              min integrity {{ ra.issuance_policy.min_integrity }}
            </span>
            <span v-for="s in ra.skill_ids" :key="s" class="rounded bg-muted px-1.5 py-0.5 text-muted-foreground">{{ s }}</span>
          </div>
          <div class="mt-2 flex gap-2">
            <AppButton v-if="ra.status === 'draft'" variant="ghost" size="sm" @click="sponsor.setRoleAssessmentStatus(ra.id, 'published')">Publish</AppButton>
            <AppButton v-if="ra.status !== 'archived'" variant="ghost" size="sm" @click="sponsor.setRoleAssessmentStatus(ra.id, 'archived')">Archive</AppButton>
          </div>
        </div>
      </div>
    </section>

    <!-- Issue --------------------------------------------------------- -->
    <section v-else>
      <div class="grid gap-3 rounded-lg border border-border bg-card p-4">
        <label class="text-xs text-muted-foreground">Role assessment
          <select v-model="issueForm.role_assessment_id" class="mt-1 w-full rounded-md border border-border bg-background p-2 text-sm">
            <option value="">Select…</option>
            <option v-for="ra in publishedRoles" :key="ra.id" :value="ra.id">{{ ra.role_title }} — {{ orgName_(ra.org_id) }}</option>
          </select>
        </label>
        <AppInput v-model="issueForm.subject" label="Candidate DID" placeholder="did:key:z6Mk…" />
        <AppInput v-model="issueForm.integrity_session_id" label="Integrity session id" placeholder="isess_…" />
        <div>
          <AppButton variant="primary" size="sm" :disabled="issueBusy" @click="issue">
            {{ issueBusy ? 'Issuing…' : 'Issue Role Credential' }}
          </AppButton>
        </div>
        <AppAlert v-if="issueError" variant="error">{{ issueError }}</AppAlert>
        <div v-if="issueResult" class="rounded-md border border-emerald-300 bg-emerald-50 p-3 text-sm dark:border-emerald-800/40 dark:bg-emerald-900/20">
          <p class="font-medium text-emerald-700 dark:text-emerald-400">Credential issued</p>
          <p class="mt-1 font-mono text-xs break-all text-muted-foreground">{{ issueResult.id }}</p>
          <p v-if="issueResult.integrity" class="mt-1 text-xs text-muted-foreground">
            assurance:
            <AppBadge :variant="assuranceTone(issueResult.integrity.assuranceLevel)">{{ issueResult.integrity.assuranceLevel }}</AppBadge>
            · integrity {{ issueResult.integrity.integrityScore ?? 'n/a' }}
          </p>
        </div>
      </div>
    </section>

    <!-- New org modal -->
    <AppModal :open="orgModalOpen" title="New Organization" @close="orgModalOpen = false">
      <div class="grid gap-3">
        <AppInput v-model="orgName" label="Name" placeholder="Acme Corp" />
        <div class="flex justify-end gap-2">
          <AppButton variant="ghost" size="sm" @click="orgModalOpen = false">Cancel</AppButton>
          <AppButton variant="primary" size="sm" :disabled="orgBusy || !orgName.trim()" @click="createOrg">Create</AppButton>
        </div>
      </div>
    </AppModal>

    <!-- New role assessment modal -->
    <AppModal :open="roleModalOpen" title="New Role Assessment" @close="roleModalOpen = false">
      <div class="grid max-h-[70vh] gap-3 overflow-y-auto">
        <label class="text-xs text-muted-foreground">Organization
          <select v-model="roleForm.org_id" class="mt-1 w-full rounded-md border border-border bg-background p-2 text-sm">
            <option v-for="o in sponsor.organizations.value" :key="o.id" :value="o.id">{{ o.name }}</option>
          </select>
        </label>
        <AppInput v-model="roleForm.role_title" label="Role title" placeholder="SRE L4" />
        <AppTextarea v-model="roleForm.job_description" label="Job description" placeholder="Operate production at scale…" />
        <AppInput v-model="roleForm.skill_ids" label="Skill ids (comma-separated)" placeholder="skill:sre, skill:linux" />
        <p class="text-xs font-medium text-foreground">Issuance policy</p>
        <div class="grid grid-cols-3 gap-2">
          <AppInput v-model="roleForm.min_integrity" label="Min integrity" placeholder="0.70" type="number" />
          <AppInput v-model="roleForm.max_critical" label="Max critical" placeholder="0" type="number" />
          <AppInput v-model="roleForm.max_warning" label="Max warning" placeholder="2" type="number" />
        </div>
        <label class="flex items-center gap-2 text-sm text-foreground">
          <input v-model="roleForm.require_clean" type="checkbox" /> Require clean session
        </label>
        <label class="text-xs text-muted-foreground">Required assurance level
          <select v-model="roleForm.required_assurance_level" class="mt-1 w-full rounded-md border border-border bg-background p-2 text-sm">
            <option value="">(none)</option>
            <option v-for="l in ASSURANCE_LEVELS" :key="l" :value="l">{{ l }}</option>
          </select>
        </label>
        <div class="flex justify-end gap-2">
          <AppButton variant="ghost" size="sm" @click="roleModalOpen = false">Cancel</AppButton>
          <AppButton variant="primary" size="sm" :disabled="roleBusy || !roleForm.role_title.trim() || !roleForm.org_id" @click="createRole">Create</AppButton>
        </div>
      </div>
    </AppModal>
  </div>
</template>
