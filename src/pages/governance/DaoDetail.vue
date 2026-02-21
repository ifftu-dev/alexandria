<script setup lang="ts">
import { ref, onMounted, computed } from 'vue'
import { useRoute } from 'vue-router'
import { useLocalApi } from '@/composables/useLocalApi'
import { useAuth } from '@/composables/useAuth'
import {
  AppButton, AppSpinner, AppBadge, AppInput, AppModal, AppTabs,
  EmptyState, StatusBadge, DataRow,
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

function votePercent(vFor: number, vAgainst: number): number {
  const total = vFor + vAgainst
  return total > 0 ? (vFor / total) * 100 : 0
}

function shortAddr(addr: string): string {
  if (addr.length <= 20) return addr
  return addr.slice(0, 12) + '...' + addr.slice(-8)
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
    <AppSpinner v-if="loading" label="Loading DAO..." />

    <EmptyState v-else-if="!dao" title="DAO not found" />

    <div v-else>
      <!-- Header -->
      <div class="flex items-start justify-between mb-6">
        <div class="flex items-start gap-3">
          <div class="governance-seal text-sm font-bold">
            {{ dao.scope_type === 'subject_field' ? 'SF' : 'SU' }}
          </div>
          <div>
            <div class="flex items-center gap-2 mb-1">
              <StatusBadge :status="dao.status" />
            </div>
            <h1 class="text-xl font-bold">{{ dao.name }}</h1>
            <p v-if="dao.description" class="text-sm text-[rgb(var(--color-muted-foreground))] mt-1">
              {{ dao.description }}
            </p>
          </div>
        </div>
        <router-link to="/governance" class="text-xs text-[rgb(var(--color-muted-foreground))] hover:underline">
          Back
        </router-link>
      </div>

      <p v-if="error" class="text-sm text-[rgb(var(--color-error))] mb-4">{{ error }}</p>

      <!-- Tabs -->
      <AppTabs v-model="activeTab" :tabs="tabs" class="mb-6" />

      <!-- ==================== OVERVIEW TAB ==================== -->
      <div v-if="activeTab === 'overview'" class="space-y-6">
        <div class="card p-5">
          <h2 class="text-base font-semibold mb-3">Details</h2>
          <div class="space-y-2">
            <DataRow label="Scope">{{ dao.scope_type }}</DataRow>
            <DataRow label="Scope ID" mono>{{ dao.scope_id }}</DataRow>
            <DataRow label="Committee Size">{{ dao.committee_size }}</DataRow>
            <DataRow label="Election Interval">{{ dao.election_interval_days }} days</DataRow>
            <DataRow v-if="dao.on_chain_tx" label="On-chain TX" mono>{{ dao.on_chain_tx }}</DataRow>
            <DataRow label="Created">{{ dao.created_at }}</DataRow>
          </div>
        </div>

        <!-- Committee -->
        <div class="card p-5">
          <h2 class="text-base font-semibold mb-3">Committee ({{ committeMembers.length }} / {{ dao.committee_size }})</h2>
          <EmptyState v-if="committeMembers.length === 0" title="No committee members" description="Run an election to install a committee." />
          <div v-else class="space-y-2">
            <div v-for="m in committeMembers" :key="m.stake_address" class="flex items-center justify-between p-2 rounded bg-[rgb(var(--color-muted)/0.3)]">
              <div class="flex items-center gap-2">
                <AppBadge variant="governance">committee</AppBadge>
                <span class="text-sm font-mono">{{ shortAddr(m.stake_address) }}</span>
              </div>
              <span class="text-xs text-[rgb(var(--color-muted-foreground))]">{{ m.joined_at }}</span>
            </div>
          </div>
        </div>

        <!-- Regular members -->
        <div v-if="regularMembers.length" class="card p-5">
          <h2 class="text-base font-semibold mb-3">Members ({{ regularMembers.length }})</h2>
          <div class="space-y-2">
            <div v-for="m in regularMembers" :key="m.stake_address" class="flex items-center justify-between p-2 rounded bg-[rgb(var(--color-muted)/0.3)]">
              <div class="flex items-center gap-2">
                <AppBadge variant="secondary">{{ m.role }}</AppBadge>
                <span class="text-sm font-mono">{{ shortAddr(m.stake_address) }}</span>
              </div>
              <span class="text-xs text-[rgb(var(--color-muted-foreground))]">{{ m.joined_at }}</span>
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
            <button class="text-xs text-[rgb(var(--color-primary))] hover:underline" @click="closeElectionDetail">
              Elections
            </button>
            <span class="text-xs text-[rgb(var(--color-muted-foreground))]">/</span>
            <span class="text-xs font-medium">{{ selectedElection.title }}</span>
          </div>

          <div class="card p-5">
            <div class="flex items-start justify-between mb-4">
              <div>
                <h3 class="text-sm font-semibold">{{ selectedElection.title }}</h3>
                <p v-if="selectedElection.description" class="text-xs text-[rgb(var(--color-muted-foreground))] mt-0.5">
                  {{ selectedElection.description }}
                </p>
              </div>
              <StatusBadge :status="selectedElection.phase" />
            </div>

            <div class="grid grid-cols-3 gap-3 text-xs mb-4">
              <DataRow label="Seats">{{ selectedElection.seats }}</DataRow>
              <DataRow label="Phase">{{ selectedElection.phase }}</DataRow>
              <DataRow label="Created">{{ selectedElection.created_at }}</DataRow>
            </div>

            <!-- Phase actions -->
            <div class="flex gap-2 mb-4">
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
            <h4 class="text-xs font-semibold text-[rgb(var(--color-muted-foreground))] uppercase tracking-wide mb-2">Nominees</h4>
            <AppSpinner v-if="loadingElection" label="Loading..." />
            <EmptyState v-else-if="electionNominees.length === 0" title="No nominees yet" />
            <div v-else class="space-y-2">
              <div
                v-for="nom in electionNominees"
                :key="nom.id"
                class="flex items-center justify-between p-3 rounded border"
                :class="nom.is_winner ? 'border-[rgb(var(--color-success))] bg-[rgb(var(--color-success)/0.05)]' : 'border-[rgb(var(--color-border))]'"
              >
                <div class="flex items-center gap-2">
                  <AppBadge v-if="nom.is_winner" variant="success">Winner</AppBadge>
                  <AppBadge v-else-if="nom.accepted" variant="primary">Accepted</AppBadge>
                  <AppBadge v-else variant="warning">Pending</AppBadge>
                  <span class="text-sm font-mono">{{ shortAddr(nom.stake_address) }}</span>
                </div>
                <div class="flex items-center gap-3">
                  <span class="text-xs text-[rgb(var(--color-muted-foreground))]">{{ nom.votes_received }} votes</span>
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
            </div>
          </div>
        </div>

        <!-- Election List -->
        <div v-else class="space-y-4">
          <div v-for="election in elections" :key="election.id" class="card-interactive p-4" @click="openElectionDetail(election)">
            <div class="flex items-start justify-between mb-2">
              <div>
                <div class="text-sm font-semibold">{{ election.title }}</div>
                <p v-if="election.description" class="text-xs text-[rgb(var(--color-muted-foreground))] mt-0.5">
                  {{ election.description }}
                </p>
              </div>
              <StatusBadge :status="election.phase" />
            </div>
            <div class="flex items-center gap-4 text-xs text-[rgb(var(--color-muted-foreground))]">
              <span>{{ election.seats }} seat{{ election.seats !== 1 ? 's' : '' }}</span>
              <span v-if="election.voting_end">Voting ends: {{ election.voting_end }}</span>
              <span v-if="election.finalized_at">Finalized: {{ election.finalized_at }}</span>
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
          <div v-for="proposal in proposals" :key="proposal.id" class="card p-4">
            <div class="flex items-start justify-between mb-2">
              <div>
                <div class="text-sm font-semibold">{{ proposal.title }}</div>
                <p v-if="proposal.description" class="text-xs text-[rgb(var(--color-muted-foreground))] mt-0.5">
                  {{ proposal.description }}
                </p>
              </div>
              <StatusBadge :status="proposal.status" />
            </div>

            <!-- Vote gauge -->
            <div v-if="proposal.votes_for + proposal.votes_against > 0" class="mb-3">
              <div class="vote-gauge">
                <div
                  class="vote-gauge-for"
                  :style="{ width: `${votePercent(proposal.votes_for, proposal.votes_against)}%` }"
                />
                <div
                  class="vote-gauge-against"
                  :style="{ width: `${100 - votePercent(proposal.votes_for, proposal.votes_against)}%` }"
                />
              </div>
              <div class="flex justify-between text-xs text-[rgb(var(--color-muted-foreground))] mt-1">
                <span>{{ proposal.votes_for }} for</span>
                <span>{{ proposal.votes_against }} against</span>
              </div>
            </div>

            <div class="flex items-center gap-3 text-xs text-[rgb(var(--color-muted-foreground))] mb-3">
              <AppBadge variant="secondary">{{ proposal.category }}</AppBadge>
              <span v-if="proposal.voting_deadline">Deadline: {{ proposal.voting_deadline }}</span>
              <span class="font-mono">by {{ shortAddr(proposal.proposer) }}</span>
            </div>

            <!-- Actions -->
            <div v-if="stakeAddress" class="flex gap-2">
              <template v-if="proposal.status === 'draft'">
                <AppButton size="xs" @click="approveProposal(proposal.id)">Approve (Open Voting)</AppButton>
                <AppButton size="xs" variant="danger" @click="cancelProposal(proposal.id)">Cancel</AppButton>
              </template>
              <template v-else-if="proposal.status === 'published'">
                <AppButton size="xs" variant="outline" @click="voteOnProposal(proposal.id, true)">Vote For</AppButton>
                <AppButton size="xs" variant="danger" @click="voteOnProposal(proposal.id, false)">Vote Against</AppButton>
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
          <p v-if="error" class="text-sm text-[rgb(var(--color-error))]">{{ error }}</p>
          <div>
            <label class="block text-xs font-medium mb-1">Title</label>
            <AppInput v-model="electionForm.title" placeholder="e.g. Q1 2026 Committee Election" />
          </div>
          <div>
            <label class="block text-xs font-medium mb-1">Description</label>
            <input v-model="electionForm.description" class="input w-full" placeholder="Purpose of this election..." />
          </div>
          <div>
            <label class="block text-xs font-medium mb-1">Seats</label>
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
          <p v-if="error" class="text-sm text-[rgb(var(--color-error))]">{{ error }}</p>
          <div>
            <label class="block text-xs font-medium mb-1">Title</label>
            <AppInput v-model="proposalForm.title" placeholder="e.g. Add NLP skills to taxonomy" />
          </div>
          <div>
            <label class="block text-xs font-medium mb-1">Description</label>
            <input v-model="proposalForm.description" class="input w-full" placeholder="Describe the proposed change..." />
          </div>
          <div>
            <label class="block text-xs font-medium mb-1">Category</label>
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
