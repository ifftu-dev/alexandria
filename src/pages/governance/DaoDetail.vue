<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { useRoute } from 'vue-router'
import { useLocalApi } from '@/composables/useLocalApi'
import { AppSpinner, EmptyState, AppTabs, StatusBadge, AppBadge, DataRow } from '@/components/ui'
import type { DaoInfo, Election, Proposal } from '@/types'

const { invoke } = useLocalApi()
const route = useRoute()

const daoId = route.params.id as string
const activeTab = ref('overview')

const dao = ref<DaoInfo | null>(null)
const elections = ref<Election[]>([])
const proposals = ref<Proposal[]>([])
const loading = ref(true)

const tabs = [
  { key: 'overview', label: 'Overview' },
  { key: 'elections', label: 'Elections' },
  { key: 'proposals', label: 'Proposals' },
]

onMounted(async () => {
  try {
    const [d, e, p] = await Promise.all([
      invoke<DaoInfo>('get_dao', { daoId }),
      invoke<Election[]>('list_elections', { daoId }),
      invoke<Proposal[]>('list_proposals', { daoId }),
    ])
    dao.value = d
    elections.value = e
    proposals.value = p
  } catch (e) {
    console.error('Failed to load DAO:', e)
  } finally {
    loading.value = false
  }
})

function votePercent(vFor: number, vAgainst: number): number {
  const total = vFor + vAgainst
  return total > 0 ? (vFor / total) * 100 : 0
}
</script>

<template>
  <div>
    <AppSpinner v-if="loading" label="Loading DAO..." />

    <EmptyState v-else-if="!dao" title="DAO not found" />

    <div v-else>
      <!-- Header -->
      <div class="flex items-start gap-3 mb-6">
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

      <!-- Tabs -->
      <AppTabs v-model="activeTab" :tabs="tabs" class="mb-6" />

      <!-- Overview -->
      <div v-if="activeTab === 'overview'" class="card p-5">
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

      <!-- Elections -->
      <div v-else-if="activeTab === 'elections'">
        <EmptyState
          v-if="elections.length === 0"
          title="No elections"
          description="Elections are opened by DAO committee members."
        />

        <div v-else class="space-y-4">
          <div v-for="election in elections" :key="election.id" class="card p-4">
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
            </div>
          </div>
        </div>
      </div>

      <!-- Proposals -->
      <div v-else-if="activeTab === 'proposals'">
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
            <div v-if="proposal.votes_for + proposal.votes_against > 0" class="mb-2">
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

            <div class="flex items-center gap-3 text-xs text-[rgb(var(--color-muted-foreground))]">
              <AppBadge variant="secondary">{{ proposal.category }}</AppBadge>
              <span v-if="proposal.voting_deadline">Deadline: {{ proposal.voting_deadline }}</span>
            </div>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>
