<script setup lang="ts">
import { ref, onMounted, computed } from 'vue'
import { useRoute } from 'vue-router'
import { useLocalApi } from '@/composables/useLocalApi'
import { useAuth } from '@/composables/useAuth'
import {
  AppButton, AppBadge, AppInput, AppModal, AppTabs,
  EmptyState, StatusBadge,
} from '@/components/ui'
import type {
  DaoInfo, DaoMember, Election, ElectionNominee, Proposal,
  OpenElectionParams, SubmitProposalParams,
} from '@/types'

const { invoke } = useLocalApi()
const route = useRoute()
const { stakeAddress } = useAuth()

const daoId = route.params.id as string
const activeTab = ref('overview')

const dao = ref<DaoInfo | null>(null)
const members = ref<DaoMember[]>([])
const elections = ref<Election[]>([])
const proposals = ref<Proposal[]>([])
const loading = ref(true)
const error = ref('')

// Election detail
const selectedElection = ref<Election | null>(null)
const electionNominees = ref<ElectionNominee[]>([])
const loadingElection = ref(false)

// Create election
const showCreateElection = ref(false)
const electionForm = ref<OpenElectionParams>({
  dao_id: daoId,
  title: '',
  description: '',
  seats: 5,
})
const creatingElection = ref(false)

// Create proposal
const showCreateProposal = ref(false)
const proposalForm = ref<SubmitProposalParams>({
  dao_id: daoId,
  title: '',
  description: '',
  category: 'policy',
})
const creatingProposal = ref(false)

const tabs = [
  { key: 'overview', label: 'Overview' },
  { key: 'elections', label: 'Elections' },
  { key: 'proposals', label: 'Proposals' },
]

const committeMembers = computed(() => members.value.filter(m => m.role === 'committee'))
const regularMembers = computed(() => members.value.filter(m => m.role === 'member'))
const totalMembers = computed(() => members.value.length)
const electionCount = computed(() => elections.value.length)
const proposalCount = computed(() => proposals.value.length)

function votePercent(vFor: number, vAgainst: number): number {
  const total = vFor + vAgainst
  return total > 0 ? (vFor / total) * 100 : 0
}

function shortAddr(addr: string): string {
  if (addr.length <= 20) return addr
  return addr.slice(0, 12) + '...' + addr.slice(-8)
}

const categoryColors: Record<string, string> = {
  policy: 'bg-blue-500/10 text-blue-400 border-blue-500/20',
  taxonomy: 'bg-emerald-500/10 text-emerald-400 border-emerald-500/20',
  curriculum: 'bg-violet-500/10 text-violet-400 border-violet-500/20',
  technical: 'bg-cyan-500/10 text-cyan-400 border-cyan-500/20',
  governance: 'bg-amber-500/10 text-amber-400 border-amber-500/20',
  other: 'bg-gray-500/10 text-gray-400 border-gray-500/20',
}

function getCategoryClasses(category: string): string {
  return categoryColors[category] ?? categoryColors.other ?? ''
}

onMounted(async () => {
  try {
    const [daoResult, e, p] = await Promise.all([
      invoke<[DaoInfo, DaoMember[]]>('get_dao', { daoId }),
      invoke<Election[]>('list_elections', { daoId }),
      invoke<Proposal[]>('list_proposals', { daoId }),
    ])
    dao.value = daoResult[0]
    members.value = daoResult[1]
    elections.value = e
    proposals.value = p
  } catch (e) {
    console.error('Failed to load DAO:', e)
  } finally {
    loading.value = false
  }
})

// ---- Election Actions ----

async function openElectionDetail(election: Election) {
  selectedElection.value = election
  loadingElection.value = true
  try {
    const [el, noms] = await invoke<[Election, ElectionNominee[]]>('get_election', { electionId: election.id })
    selectedElection.value = el
    electionNominees.value = noms
  } catch (e) {
    error.value = String(e)
  } finally {
    loadingElection.value = false
  }
}

function closeElectionDetail() {
  selectedElection.value = null
  electionNominees.value = []
}

async function createElection() {
  if (!electionForm.value.title.trim()) {
    error.value = 'Election title is required.'
    return
  }
  creatingElection.value = true
  error.value = ''
  try {
    const election = await invoke<Election>('open_election', { params: electionForm.value })
    elections.value.unshift(election)
    showCreateElection.value = false
  } catch (e) {
    error.value = String(e)
  } finally {
    creatingElection.value = false
  }
}

async function nominateSelf() {
  if (!selectedElection.value || !stakeAddress.value) return
  error.value = ''
  try {
    const nominee = await invoke<ElectionNominee>('nominate', {
      electionId: selectedElection.value.id,
      stakeAddress: stakeAddress.value,
    })
    electionNominees.value.push(nominee)
  } catch (e) {
    error.value = String(e)
  }
}

async function acceptNomination(nomineeId: string) {
  error.value = ''
  try {
    await invoke('accept_nomination', { nomineeId })
    const nom = electionNominees.value.find(n => n.id === nomineeId)
    if (nom) nom.accepted = true
  } catch (e) {
    error.value = String(e)
  }
}

async function startVoting() {
  if (!selectedElection.value) return
  error.value = ''
  try {
    await invoke('start_election_voting', { electionId: selectedElection.value.id })
    selectedElection.value.phase = 'voting'
    const match = elections.value.find(e => e.id === selectedElection.value!.id)
    if (match) match.phase = 'voting'
  } catch (e) {
    error.value = String(e)
  }
}

async function castElectionVote(nomineeId: string) {
  if (!selectedElection.value || !stakeAddress.value) return
  error.value = ''
  try {
    await invoke('cast_election_vote', {
      electionId: selectedElection.value.id,
      voter: stakeAddress.value,
      nomineeId,
    })
    const nom = electionNominees.value.find(n => n.id === nomineeId)
    if (nom) nom.votes_received += 1
  } catch (e) {
    error.value = String(e)
  }
}

async function finalizeElection() {
  if (!selectedElection.value) return
  error.value = ''
  try {
    const nominees = await invoke<ElectionNominee[]>('finalize_election', { electionId: selectedElection.value.id })
    electionNominees.value = nominees
    selectedElection.value.phase = 'finalized'
    const match = elections.value.find(e => e.id === selectedElection.value!.id)
    if (match) match.phase = 'finalized'
  } catch (e) {
    error.value = String(e)
  }
}

async function installCommittee() {
  if (!selectedElection.value) return
  error.value = ''
  try {
    const newMembers = await invoke<DaoMember[]>('install_committee', { electionId: selectedElection.value.id })
    members.value = newMembers
  } catch (e) {
    error.value = String(e)
  }
}

// ---- Proposal Actions ----

async function createProposal() {
  if (!proposalForm.value.title.trim()) {
    error.value = 'Proposal title is required.'
    return
  }
  creatingProposal.value = true
  error.value = ''
  try {
    const proposal = await invoke<Proposal>('submit_proposal', { params: proposalForm.value })
    proposals.value.unshift(proposal)
    showCreateProposal.value = false
  } catch (e) {
    error.value = String(e)
  } finally {
    creatingProposal.value = false
  }
}

async function approveProposal(proposalId: string) {
  error.value = ''
  try {
    const updated = await invoke<Proposal>('approve_proposal', { proposalId })
    const idx = proposals.value.findIndex(p => p.id === proposalId)
    if (idx >= 0) proposals.value[idx] = updated
  } catch (e) {
    error.value = String(e)
  }
}

async function cancelProposal(proposalId: string) {
  error.value = ''
  try {
    await invoke('cancel_proposal', { proposalId })
    const match = proposals.value.find(p => p.id === proposalId)
    if (match) match.status = 'cancelled'
  } catch (e) {
    error.value = String(e)
  }
}

async function voteOnProposal(proposalId: string, inFavor: boolean) {
  if (!stakeAddress.value) return
  error.value = ''
  try {
    await invoke('cast_proposal_vote', { proposalId, voter: stakeAddress.value, inFavor })
    const match = proposals.value.find(p => p.id === proposalId)
    if (match) {
      if (inFavor) match.votes_for += 1
      else match.votes_against += 1
    }
  } catch (e) {
    error.value = String(e)
  }
}

async function resolveProposal(proposalId: string) {
  error.value = ''
  try {
    const updated = await invoke<Proposal>('resolve_proposal', { proposalId })
    const idx = proposals.value.findIndex(p => p.id === proposalId)
    if (idx >= 0) proposals.value[idx] = updated
  } catch (e) {
    error.value = String(e)
  }
}

const proposalCategories = ['policy', 'taxonomy', 'curriculum', 'technical', 'governance', 'other']
</script>

<template>
  <div>
    <!-- ==================== SKELETON LOADER ==================== -->
    <div v-if="loading" class="animate-pulse space-y-6">
      <!-- Breadcrumb skeleton -->
      <div class="flex items-center gap-2">
        <div class="h-4 w-20 rounded bg-muted" />
        <div class="h-4 w-3 rounded bg-muted/50" />
        <div class="h-4 w-32 rounded bg-muted" />
      </div>
      <!-- Header skeleton -->
      <div class="flex items-start gap-4">
        <div class="h-14 w-14 rounded-xl bg-amber-500/10" />
        <div class="flex-1 space-y-3">
          <div class="flex items-center gap-3">
            <div class="h-7 w-64 rounded bg-muted" />
            <div class="h-5 w-16 rounded-full bg-muted" />
          </div>
          <div class="h-4 w-96 max-w-full rounded bg-muted/50" />
          <div class="flex gap-4">
            <div class="h-6 w-24 rounded-lg bg-muted/30" />
            <div class="h-6 w-28 rounded-lg bg-muted/30" />
            <div class="h-6 w-22 rounded-lg bg-muted/30" />
            <div class="h-6 w-26 rounded-lg bg-muted/30" />
          </div>
        </div>
      </div>
      <!-- Tabs skeleton -->
      <div class="flex gap-4 border-b border-border/50">
        <div class="h-8 w-20 rounded bg-muted/30" />
        <div class="h-8 w-20 rounded bg-muted/30" />
        <div class="h-8 w-20 rounded bg-muted/30" />
      </div>
      <!-- Content skeleton -->
      <div class="card p-5 space-y-3">
        <div class="h-5 w-24 rounded bg-muted" />
        <div v-for="i in 5" :key="i" class="flex items-center justify-between py-3">
          <div class="h-4 w-28 rounded bg-muted/30" />
          <div class="h-4 w-40 rounded bg-muted/30" />
        </div>
      </div>
      <div class="card p-5 space-y-3">
        <div class="h-5 w-32 rounded bg-muted" />
        <div v-for="i in 3" :key="i" class="h-14 rounded-lg bg-muted/20" />
      </div>
    </div>

    <EmptyState v-else-if="!dao" title="DAO not found" />

    <div v-else class="max-w-4xl">
      <!-- ==================== BREADCRUMB ==================== -->
      <div class="flex items-center gap-2 mb-6 text-xs">
        <router-link to="/governance" class="text-muted-foreground hover:text-foreground transition-colors">
          Governance
        </router-link>
        <svg class="w-3 h-3 text-muted-foreground/50" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
          <path stroke-linecap="round" stroke-linejoin="round" d="M9 5l7 7-7 7" />
        </svg>
        <span class="text-foreground font-medium">{{ dao.name }}</span>
      </div>

      <!-- ==================== HEADER ==================== -->
      <div class="flex items-start gap-4 mb-8">
        <div class="flex h-14 w-14 items-center justify-center rounded-xl bg-amber-500/10 shrink-0">
          <span v-if="dao.icon_emoji" class="text-2xl">{{ dao.icon_emoji }}</span>
          <svg v-else class="w-7 h-7 text-amber-500" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.5">
            <path stroke-linecap="round" stroke-linejoin="round" d="M12 21v-8.25M15.75 21v-8.25M8.25 21v-8.25M3 9l9-6 9 6m-1.5 12V10.332A48.36 48.36 0 0012 9.75c-2.551 0-5.056.2-7.5.582V21M3 21h18M12 6.75h.008v.008H12V6.75z" />
          </svg>
        </div>
        <div class="min-w-0 flex-1">
          <div class="flex items-center gap-3 mb-1.5">
            <h1 class="text-2xl font-bold tracking-tight">{{ dao.name }}</h1>
            <StatusBadge :status="dao.status" />
          </div>
          <p v-if="dao.description" class="text-sm text-muted-foreground mb-4 max-w-2xl">
            {{ dao.description }}
          </p>

          <!-- Stats chips -->
          <div class="flex items-center flex-wrap gap-3">
            <span class="inline-flex items-center gap-1.5 text-xs text-muted-foreground">
              <svg class="w-3.5 h-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                <path stroke-linecap="round" stroke-linejoin="round" d="M15 19.128a9.38 9.38 0 002.625.372 9.337 9.337 0 004.121-.952 4.125 4.125 0 00-7.533-2.493M15 19.128v-.003c0-1.113-.285-2.16-.786-3.07M15 19.128v.106A12.318 12.318 0 018.624 21c-2.331 0-4.512-.645-6.374-1.766l-.001-.109a6.375 6.375 0 0111.964-3.07M12 6.375a3.375 3.375 0 11-6.75 0 3.375 3.375 0 016.75 0zm8.25 2.25a2.625 2.625 0 11-5.25 0 2.625 2.625 0 015.25 0z" />
              </svg>
              {{ totalMembers }} member{{ totalMembers !== 1 ? 's' : '' }}
            </span>
            <span class="inline-flex items-center gap-1.5 text-xs text-muted-foreground">
              <svg class="w-3.5 h-3.5 text-amber-500" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                <path stroke-linecap="round" stroke-linejoin="round" d="M9 12.75L11.25 15 15 9.75m-3-7.036A11.959 11.959 0 013.598 6 11.99 11.99 0 003 9.749c0 5.592 3.824 10.29 9 11.623 5.176-1.332 9-6.03 9-11.622 0-1.31-.21-2.571-.598-3.751h-.152c-3.196 0-6.1-1.248-8.25-3.285z" />
              </svg>
              {{ committeMembers.length }}/{{ dao.committee_size }} committee
            </span>
            <span class="inline-flex items-center gap-1.5 text-xs text-muted-foreground">
              <svg class="w-3.5 h-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                <path stroke-linecap="round" stroke-linejoin="round" d="M12 6v6h4.5m4.5 0a9 9 0 11-18 0 9 9 0 0118 0z" />
              </svg>
              {{ electionCount }} election{{ electionCount !== 1 ? 's' : '' }}
            </span>
            <span class="inline-flex items-center gap-1.5 text-xs text-muted-foreground">
              <svg class="w-3.5 h-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                <path stroke-linecap="round" stroke-linejoin="round" d="M19.5 14.25v-2.625a3.375 3.375 0 00-3.375-3.375h-1.5A1.125 1.125 0 0113.5 7.125v-1.5a3.375 3.375 0 00-3.375-3.375H8.25m0 12.75h7.5m-7.5 3H12M10.5 2.25H5.625c-.621 0-1.125.504-1.125 1.125v17.25c0 .621.504 1.125 1.125 1.125h12.75c.621 0 1.125-.504 1.125-1.125V11.25a9 9 0 00-9-9z" />
              </svg>
              {{ proposalCount }} proposal{{ proposalCount !== 1 ? 's' : '' }}
            </span>
          </div>
        </div>
      </div>

      <p v-if="error" class="text-sm text-error mb-4">{{ error }}</p>

      <!-- Tabs -->
      <AppTabs v-model="activeTab" :tabs="tabs" class="mb-6" />

      <!-- ==================== OVERVIEW TAB ==================== -->
      <div v-if="activeTab === 'overview'" class="space-y-6">
        <!-- Details -->
        <div class="card p-5">
          <h2 class="text-base font-semibold mb-4">Details</h2>
          <div>
            <div class="flex items-center justify-between py-3 border-b border-border/50">
              <span class="text-sm text-muted-foreground">Scope</span>
              <span class="text-sm font-medium">{{ dao.scope_type }}</span>
            </div>
            <div class="flex items-center justify-between py-3 border-b border-border/50">
              <span class="text-sm text-muted-foreground">Scope ID</span>
              <span class="text-sm font-medium font-mono">{{ dao.scope_id }}</span>
            </div>
            <div class="flex items-center justify-between py-3 border-b border-border/50">
              <span class="text-sm text-muted-foreground">Committee Size</span>
              <span class="text-sm font-medium">{{ dao.committee_size }}</span>
            </div>
            <div class="flex items-center justify-between py-3 border-b border-border/50">
              <span class="text-sm text-muted-foreground">Election Interval</span>
              <span class="text-sm font-medium">{{ dao.election_interval_days }} days</span>
            </div>
            <div v-if="dao.on_chain_tx" class="flex items-center justify-between py-3 border-b border-border/50">
              <span class="text-sm text-muted-foreground">On-chain TX</span>
              <span class="text-sm font-medium font-mono">{{ dao.on_chain_tx }}</span>
            </div>
            <div class="flex items-center justify-between py-3">
              <span class="text-sm text-muted-foreground">Created</span>
              <span class="text-sm font-medium">{{ dao.created_at }}</span>
            </div>
          </div>
        </div>

        <!-- Committee -->
        <div class="card p-5">
          <div class="flex items-center justify-between mb-4">
            <h2 class="text-base font-semibold">Committee</h2>
            <span class="text-xs text-muted-foreground px-2 py-0.5 rounded-full bg-amber-500/10 text-amber-500">
              {{ committeMembers.length }} / {{ dao.committee_size }} seats
            </span>
          </div>
          <EmptyState v-if="committeMembers.length === 0" title="No committee members" description="Run an election to install a committee." />
          <div v-else class="space-y-2">
            <div
              v-for="m in committeMembers"
              :key="m.stake_address"
              class="flex items-center justify-between rounded-lg border border-border p-3"
            >
              <div class="flex items-center gap-2.5">
                <div class="flex h-8 w-8 items-center justify-center rounded-lg bg-amber-500/10">
                  <svg class="w-4 h-4 text-amber-500" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                    <path stroke-linecap="round" stroke-linejoin="round" d="M9 12.75L11.25 15 15 9.75m-3-7.036A11.959 11.959 0 013.598 6 11.99 11.99 0 003 9.749c0 5.592 3.824 10.29 9 11.623 5.176-1.332 9-6.03 9-11.622 0-1.31-.21-2.571-.598-3.751h-.152c-3.196 0-6.1-1.248-8.25-3.285z" />
                  </svg>
                </div>
                <div>
                  <span class="text-sm font-mono block">{{ shortAddr(m.stake_address) }}</span>
                  <AppBadge variant="governance" class="text-[0.6rem] mt-0.5">committee</AppBadge>
                </div>
              </div>
              <span class="text-xs text-muted-foreground">{{ m.joined_at }}</span>
            </div>
          </div>
        </div>

        <!-- Regular members -->
        <div v-if="regularMembers.length" class="card p-5">
          <h2 class="text-base font-semibold mb-4">Members ({{ regularMembers.length }})</h2>
          <div class="space-y-2">
            <div
              v-for="m in regularMembers"
              :key="m.stake_address"
              class="flex items-center justify-between rounded-lg border border-border p-3"
            >
              <div class="flex items-center gap-2.5">
                <div class="flex h-8 w-8 items-center justify-center rounded-lg bg-muted/50">
                  <svg class="w-4 h-4 text-muted-foreground" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                    <path stroke-linecap="round" stroke-linejoin="round" d="M15.75 6a3.75 3.75 0 11-7.5 0 3.75 3.75 0 017.5 0zM4.501 20.118a7.5 7.5 0 0114.998 0A17.933 17.933 0 0112 21.75c-2.676 0-5.216-.584-7.499-1.632z" />
                  </svg>
                </div>
                <div>
                  <span class="text-sm font-mono block">{{ shortAddr(m.stake_address) }}</span>
                  <AppBadge variant="secondary" class="text-[0.6rem] mt-0.5">{{ m.role }}</AppBadge>
                </div>
              </div>
              <span class="text-xs text-muted-foreground">{{ m.joined_at }}</span>
            </div>
          </div>
        </div>
      </div>

      <!-- ==================== ELECTIONS TAB ==================== -->
      <div v-else-if="activeTab === 'elections'">
        <div class="flex items-center justify-between mb-4">
          <h2 class="text-base font-semibold">Elections</h2>
          <AppButton v-if="stakeAddress && dao.status === 'active'" size="sm" @click="showCreateElection = true; electionForm = { dao_id: daoId, title: '', description: '', seats: dao.committee_size }">
            + Open Election
          </AppButton>
        </div>

        <EmptyState
          v-if="elections.length === 0 && !selectedElection"
          title="No elections"
          description="Open an election to select committee members."
        />

        <!-- Election Detail View -->
        <div v-if="selectedElection" class="space-y-4">
          <div class="flex items-center gap-2 mb-2">
            <button class="text-xs text-primary hover:underline" @click="closeElectionDetail">
              Elections
            </button>
            <svg class="w-3 h-3 text-muted-foreground/50" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
              <path stroke-linecap="round" stroke-linejoin="round" d="M9 5l7 7-7 7" />
            </svg>
            <span class="text-xs font-medium">{{ selectedElection.title }}</span>
          </div>

          <div class="rounded-xl border border-border bg-card p-6">
            <div class="flex items-start justify-between mb-5">
              <div>
                <h3 class="text-lg font-semibold">{{ selectedElection.title }}</h3>
                <p v-if="selectedElection.description" class="text-sm text-muted-foreground mt-1">
                  {{ selectedElection.description }}
                </p>
              </div>
              <StatusBadge :status="selectedElection.phase" />
            </div>

            <!-- Election meta chips -->
            <div class="flex items-center flex-wrap gap-3 mb-5">
              <span class="inline-flex items-center gap-1.5 px-2.5 py-1 rounded-lg bg-muted/30 text-xs text-muted-foreground">
                <svg class="w-3 h-3" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                  <path stroke-linecap="round" stroke-linejoin="round" d="M18 18.72a9.094 9.094 0 003.741-.479 3 3 0 00-4.682-2.72m.94 3.198l.001.031c0 .225-.012.447-.037.666A11.944 11.944 0 0112 21c-2.17 0-4.207-.576-5.963-1.584A6.062 6.062 0 016 18.719m12 0a5.971 5.971 0 00-.941-3.197m0 0A5.995 5.995 0 0012 12.75a5.995 5.995 0 00-5.058 2.772m0 0a3 3 0 00-4.681 2.72 8.986 8.986 0 003.74.477m.94-3.197a5.971 5.971 0 00-.94 3.197M15 6.75a3 3 0 11-6 0 3 3 0 016 0zm6 3a2.25 2.25 0 11-4.5 0 2.25 2.25 0 014.5 0zm-13.5 0a2.25 2.25 0 11-4.5 0 2.25 2.25 0 014.5 0z" />
                </svg>
                {{ selectedElection.seats }} seat{{ selectedElection.seats !== 1 ? 's' : '' }}
              </span>
              <span class="inline-flex items-center gap-1.5 px-2.5 py-1 rounded-lg bg-muted/30 text-xs text-muted-foreground">
                <svg class="w-3 h-3" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                  <path stroke-linecap="round" stroke-linejoin="round" d="M9.568 3H5.25A2.25 2.25 0 003 5.25v4.318c0 .597.237 1.17.659 1.591l9.581 9.581c.699.699 1.78.872 2.607.33a18.095 18.095 0 005.223-5.223c.542-.827.369-1.908-.33-2.607L11.16 3.66A2.25 2.25 0 009.568 3z" />
                </svg>
                {{ selectedElection.phase }}
              </span>
              <span class="inline-flex items-center gap-1.5 px-2.5 py-1 rounded-lg bg-muted/30 text-xs text-muted-foreground">
                <svg class="w-3 h-3" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                  <path stroke-linecap="round" stroke-linejoin="round" d="M6.75 3v2.25M17.25 3v2.25M3 18.75V7.5a2.25 2.25 0 012.25-2.25h13.5A2.25 2.25 0 0121 7.5v11.25m-18 0A2.25 2.25 0 005.25 21h13.5A2.25 2.25 0 0021 18.75m-18 0v-7.5A2.25 2.25 0 015.25 9h13.5A2.25 2.25 0 0121 11.25v7.5" />
                </svg>
                {{ selectedElection.created_at }}
              </span>
            </div>

            <!-- Phase actions -->
            <div class="flex gap-2 mb-6">
              <AppButton v-if="selectedElection.phase === 'nomination' && stakeAddress" size="sm" variant="outline" @click="nominateSelf">
                Nominate Self
              </AppButton>
              <AppButton v-if="selectedElection.phase === 'nomination'" size="sm" @click="startVoting">
                Start Voting
              </AppButton>
              <AppButton v-if="selectedElection.phase === 'voting'" size="sm" @click="finalizeElection">
                Finalize
              </AppButton>
              <AppButton v-if="selectedElection.phase === 'finalized'" size="sm" @click="installCommittee">
                Install Committee
              </AppButton>
            </div>

            <!-- Nominees -->
            <h4 class="text-xs font-semibold text-muted-foreground uppercase tracking-wide mb-3">Nominees</h4>

            <!-- Loading skeleton for nominees -->
            <div v-if="loadingElection" class="animate-pulse space-y-3">
              <div v-for="i in 3" :key="i" class="rounded-lg border border-border/30 p-4">
                <div class="flex items-center justify-between">
                  <div class="flex items-center gap-3">
                    <div class="h-5 w-16 rounded-full bg-muted" />
                    <div class="h-4 w-36 rounded bg-muted/50" />
                  </div>
                  <div class="h-4 w-20 rounded bg-muted/30" />
                </div>
              </div>
            </div>

            <EmptyState v-else-if="electionNominees.length === 0" title="No nominees yet" />

            <div v-else class="space-y-3">
              <div
                v-for="nom in electionNominees"
                :key="nom.id"
                class="rounded-xl border p-4 transition-all"
                :class="nom.is_winner
                  ? 'border-emerald-500/30 bg-emerald-500/5'
                  : 'border-border hover:border-primary/30'"
              >
                <div class="flex items-center justify-between mb-2">
                  <div class="flex items-center gap-2.5">
                    <AppBadge v-if="nom.is_winner" variant="success">Winner</AppBadge>
                    <AppBadge v-else-if="nom.accepted" variant="primary">Accepted</AppBadge>
                    <AppBadge v-else variant="warning">Pending</AppBadge>
                    <span class="text-sm font-mono">{{ shortAddr(nom.stake_address) }}</span>
                  </div>
                  <div class="flex items-center gap-3">
                    <AppButton
                      v-if="!nom.accepted && nom.stake_address === stakeAddress"
                      size="xs"
                      variant="outline"
                      @click="acceptNomination(nom.id)"
                    >
                      Accept
                    </AppButton>
                    <AppButton
                      v-if="selectedElection.phase === 'voting' && nom.accepted && stakeAddress"
                      size="xs"
                      @click="castElectionVote(nom.id)"
                    >
                      Vote
                    </AppButton>
                  </div>
                </div>
                <!-- Vote bar -->
                <div class="flex items-center gap-3">
                  <div class="flex-1 h-1.5 rounded-full bg-muted/30 overflow-hidden">
                    <div
                      class="h-full rounded-full bg-primary transition-all duration-300"
                      :style="{ width: electionNominees.reduce((max, n) => Math.max(max, n.votes_received), 0) > 0
                        ? `${(nom.votes_received / electionNominees.reduce((max, n) => Math.max(max, n.votes_received), 0)) * 100}%`
                        : '0%' }"
                    />
                  </div>
                  <span class="text-xs text-muted-foreground tabular-nums shrink-0">{{ nom.votes_received }} vote{{ nom.votes_received !== 1 ? 's' : '' }}</span>
                </div>
              </div>
            </div>
          </div>
        </div>

        <!-- Election List -->
        <div v-else class="space-y-4">
          <div
            v-for="election in elections"
            :key="election.id"
            class="rounded-xl border border-border bg-card p-5 transition-all hover:border-primary/30 cursor-pointer"
            @click="openElectionDetail(election)"
          >
            <div class="flex items-start justify-between mb-3">
              <div>
                <div class="text-sm font-semibold">{{ election.title }}</div>
                <p v-if="election.description" class="text-sm text-muted-foreground mt-1">
                  {{ election.description }}
                </p>
              </div>
              <StatusBadge :status="election.phase" />
            </div>
            <div class="flex items-center flex-wrap gap-3">
              <span class="inline-flex items-center gap-1.5 px-2 py-0.5 rounded-md bg-muted/30 text-xs text-muted-foreground">
                <svg class="w-3 h-3" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                  <path stroke-linecap="round" stroke-linejoin="round" d="M18 18.72a9.094 9.094 0 003.741-.479 3 3 0 00-4.682-2.72m.94 3.198l.001.031c0 .225-.012.447-.037.666A11.944 11.944 0 0112 21c-2.17 0-4.207-.576-5.963-1.584A6.062 6.062 0 016 18.719m12 0a5.971 5.971 0 00-.941-3.197m0 0A5.995 5.995 0 0012 12.75a5.995 5.995 0 00-5.058 2.772m0 0a3 3 0 00-4.681 2.72 8.986 8.986 0 003.74.477m.94-3.197a5.971 5.971 0 00-.94 3.197M15 6.75a3 3 0 11-6 0 3 3 0 016 0zm6 3a2.25 2.25 0 11-4.5 0 2.25 2.25 0 014.5 0zm-13.5 0a2.25 2.25 0 11-4.5 0 2.25 2.25 0 014.5 0z" />
                </svg>
                {{ election.seats }} seat{{ election.seats !== 1 ? 's' : '' }}
              </span>
              <span v-if="election.voting_end" class="inline-flex items-center gap-1.5 px-2 py-0.5 rounded-md bg-muted/30 text-xs text-muted-foreground">
                <svg class="w-3 h-3" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                  <path stroke-linecap="round" stroke-linejoin="round" d="M12 6v6h4.5m4.5 0a9 9 0 11-18 0 9 9 0 0118 0z" />
                </svg>
                Ends {{ election.voting_end }}
              </span>
              <span v-if="election.finalized_at" class="inline-flex items-center gap-1.5 px-2 py-0.5 rounded-md bg-emerald-500/10 text-xs text-emerald-400">
                <svg class="w-3 h-3" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                  <path stroke-linecap="round" stroke-linejoin="round" d="M9 12.75L11.25 15 15 9.75M21 12a9 9 0 11-18 0 9 9 0 0118 0z" />
                </svg>
                Finalized {{ election.finalized_at }}
              </span>
            </div>
          </div>
        </div>
      </div>

      <!-- ==================== PROPOSALS TAB ==================== -->
      <div v-else-if="activeTab === 'proposals'">
        <div class="flex items-center justify-between mb-4">
          <h2 class="text-base font-semibold">Proposals</h2>
          <AppButton v-if="stakeAddress && dao.status === 'active'" size="sm" @click="showCreateProposal = true; proposalForm = { dao_id: daoId, title: '', description: '', category: 'policy' }">
            + Submit Proposal
          </AppButton>
        </div>

        <EmptyState
          v-if="proposals.length === 0"
          title="No proposals"
          description="Submit a proposal to suggest changes within this DAO's scope."
        />

        <div v-else class="space-y-4">
          <div
            v-for="proposal in proposals"
            :key="proposal.id"
            class="rounded-xl border border-border bg-card p-5 transition-all hover:border-primary/30"
          >
            <div class="flex items-start justify-between mb-3">
              <div class="min-w-0 flex-1 mr-3">
                <div class="flex items-center gap-2 mb-1">
                  <span
                    class="inline-flex items-center px-2 py-0.5 rounded-md text-[0.65rem] font-medium border"
                    :class="getCategoryClasses(proposal.category)"
                  >
                    {{ proposal.category }}
                  </span>
                  <StatusBadge :status="proposal.status" />
                </div>
                <div class="text-sm font-semibold mt-1.5">{{ proposal.title }}</div>
                <p v-if="proposal.description" class="text-sm text-muted-foreground mt-1">
                  {{ proposal.description }}
                </p>
              </div>
            </div>

            <!-- Vote bar -->
            <div v-if="proposal.votes_for + proposal.votes_against > 0" class="mb-4">
              <div class="flex h-2 rounded-full overflow-hidden bg-muted/30">
                <div
                  class="bg-emerald-500 transition-all duration-300"
                  :style="{ width: votePercent(proposal.votes_for, proposal.votes_against) + '%' }"
                />
                <div
                  class="bg-red-500 transition-all duration-300"
                  :style="{ width: (100 - votePercent(proposal.votes_for, proposal.votes_against)) + '%' }"
                />
              </div>
              <div class="flex justify-between text-xs mt-1.5">
                <span class="text-emerald-400">{{ proposal.votes_for }} for</span>
                <span class="text-red-400">{{ proposal.votes_against }} against</span>
              </div>
            </div>

            <!-- Meta -->
            <div class="flex items-center flex-wrap gap-3 text-xs text-muted-foreground mb-4">
              <span v-if="proposal.voting_deadline" class="inline-flex items-center gap-1.5">
                <svg class="w-3 h-3" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                  <path stroke-linecap="round" stroke-linejoin="round" d="M12 6v6h4.5m4.5 0a9 9 0 11-18 0 9 9 0 0118 0z" />
                </svg>
                Deadline: {{ proposal.voting_deadline }}
              </span>
              <span class="inline-flex items-center gap-1.5">
                <svg class="w-3 h-3" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                  <path stroke-linecap="round" stroke-linejoin="round" d="M15.75 6a3.75 3.75 0 11-7.5 0 3.75 3.75 0 017.5 0zM4.501 20.118a7.5 7.5 0 0114.998 0A17.933 17.933 0 0112 21.75c-2.676 0-5.216-.584-7.499-1.632z" />
                </svg>
                <span class="font-mono">{{ shortAddr(proposal.proposer) }}</span>
              </span>
            </div>

            <!-- Actions -->
            <div v-if="stakeAddress" class="flex flex-wrap gap-2">
              <template v-if="proposal.status === 'draft'">
                <AppButton size="xs" @click="approveProposal(proposal.id)">Approve (Open Voting)</AppButton>
                <AppButton size="xs" variant="danger" @click="cancelProposal(proposal.id)">Cancel</AppButton>
              </template>
              <template v-else-if="proposal.status === 'published'">
                <AppButton size="xs" variant="outline" @click="voteOnProposal(proposal.id, true)">
                  <svg class="w-3 h-3 mr-1" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                    <path stroke-linecap="round" stroke-linejoin="round" d="M4.5 12.75l6 6 9-13.5" />
                  </svg>
                  Vote For
                </AppButton>
                <AppButton size="xs" variant="danger" @click="voteOnProposal(proposal.id, false)">
                  <svg class="w-3 h-3 mr-1" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                    <path stroke-linecap="round" stroke-linejoin="round" d="M6 18L18 6M6 6l12 12" />
                  </svg>
                  Vote Against
                </AppButton>
                <AppButton size="xs" variant="ghost" @click="resolveProposal(proposal.id)">Resolve</AppButton>
                <AppButton size="xs" variant="ghost" @click="cancelProposal(proposal.id)">Cancel</AppButton>
              </template>
            </div>
          </div>
        </div>
      </div>

      <!-- ==================== CREATE ELECTION MODAL ==================== -->
      <AppModal :open="showCreateElection" title="Open Election" @close="showCreateElection = false">
        <div class="space-y-4">
          <p v-if="error" class="text-sm text-error">{{ error }}</p>
          <div>
            <label class="block text-xs font-medium text-muted-foreground mb-1.5">Title</label>
            <AppInput v-model="electionForm.title" placeholder="e.g. Q1 2026 Committee Election" />
          </div>
          <div>
            <label class="block text-xs font-medium text-muted-foreground mb-1.5">Description</label>
            <input v-model="electionForm.description" class="input w-full" placeholder="Purpose of this election..." />
          </div>
          <div>
            <label class="block text-xs font-medium text-muted-foreground mb-1.5">Seats</label>
            <input v-model.number="electionForm.seats" type="number" min="1" max="21" class="input w-full" />
          </div>
          <div class="flex justify-end gap-2 pt-2">
            <AppButton variant="ghost" @click="showCreateElection = false">Cancel</AppButton>
            <AppButton :loading="creatingElection" @click="createElection">Open Election</AppButton>
          </div>
        </div>
      </AppModal>

      <!-- ==================== CREATE PROPOSAL MODAL ==================== -->
      <AppModal :open="showCreateProposal" title="Submit Proposal" @close="showCreateProposal = false">
        <div class="space-y-4">
          <p v-if="error" class="text-sm text-error">{{ error }}</p>
          <div>
            <label class="block text-xs font-medium text-muted-foreground mb-1.5">Title</label>
            <AppInput v-model="proposalForm.title" placeholder="e.g. Add NLP skills to taxonomy" />
          </div>
          <div>
            <label class="block text-xs font-medium text-muted-foreground mb-1.5">Description</label>
            <input v-model="proposalForm.description" class="input w-full" placeholder="Describe the proposed change..." />
          </div>
          <div>
            <label class="block text-xs font-medium text-muted-foreground mb-1.5">Category</label>
            <select v-model="proposalForm.category" class="input w-full">
              <option v-for="cat in proposalCategories" :key="cat" :value="cat">{{ cat }}</option>
            </select>
          </div>
          <div class="flex justify-end gap-2 pt-2">
            <AppButton variant="ghost" @click="showCreateProposal = false">Cancel</AppButton>
            <AppButton :loading="creatingProposal" @click="createProposal">Submit Proposal</AppButton>
          </div>
        </div>
      </AppModal>
    </div>
  </div>
</template>
