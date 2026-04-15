<script setup lang="ts">
import { ref, onMounted, computed } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { invoke } from '@tauri-apps/api/core'
import {
  AppButton,
  AppBadge,
  AppAlert,
  AppSpinner,
  AppModal,
  AppTextarea,
  AppInput,
  ProvenanceBadge,
} from '@/components/ui'
import VideoPlayer from '@/components/course/VideoPlayer.vue'
import type {
  OpinionRow,
  SubjectFieldInfo,
  SkillProof,
  SubmitChallengeParams,
} from '@/types'

const route = useRoute()
const router = useRouter()

const opinion = ref<OpinionRow | null>(null)
const subjectField = ref<SubjectFieldInfo | null>(null)
const loading = ref(true)
const error = ref('')

// Local identity (to know if we're looking at our own post)
const selfStakeAddress = ref<string>('')

// Challenge modal
const showChallenge = ref(false)
const challengeReason = ref('')
const challengeStake = ref<number>(5) // ADA
const challengeDaoId = ref('')
const challengeSubmitting = ref(false)
const challengeError = ref('')

const isOwner = computed(
  () => opinion.value !== null && opinion.value.author_address === selfStakeAddress.value,
)

async function loadOpinion() {
  loading.value = true
  error.value = ''
  try {
    const id = route.params.id as string
    const row = await invoke<OpinionRow | null>('get_opinion', { opinionId: id })
    if (!row) {
      error.value = 'Opinion not found.'
      return
    }
    opinion.value = row

    // Load supporting info in parallel. Any individual failure is
    // non-fatal — the page still renders.
    const [fields, identity] = await Promise.all([
      invoke<SubjectFieldInfo[]>('list_subject_fields', {}).catch(() => []),
      invoke<{ stake_address: string } | null>('get_profile').catch(() => null),
    ])
    subjectField.value = fields.find((f) => f.id === row.subject_field_id) ?? null
    selfStakeAddress.value = identity?.stake_address ?? ''
  } catch (e) {
    error.value = String(e)
  } finally {
    loading.value = false
  }
}

async function withdraw() {
  if (!opinion.value) return
  if (!confirm('Withdraw this opinion? The video will be unpinned locally and the post hidden.')) {
    return
  }
  try {
    await invoke('withdraw_own_opinion', { opinionId: opinion.value.id })
    router.push('/opinions')
  } catch (e) {
    error.value = `Withdraw failed: ${e}`
  }
}

async function submitChallenge() {
  if (!opinion.value) return
  if (challengeReason.value.trim().length < 10) {
    challengeError.value = 'Please give a substantive reason (≥ 10 characters).'
    return
  }
  if (challengeStake.value < 5) {
    challengeError.value = 'Minimum stake is 5 ADA.'
    return
  }
  if (!challengeDaoId.value.trim()) {
    challengeError.value = 'Please provide a DAO ID to review the challenge.'
    return
  }

  challengeSubmitting.value = true
  challengeError.value = ''
  try {
    const params: SubmitChallengeParams = {
      target_type: 'opinion',
      target_ids: [opinion.value.id],
      evidence_cids: [],
      reason: challengeReason.value.trim(),
      stake_lovelace: Math.round(challengeStake.value * 1_000_000),
      dao_id: challengeDaoId.value.trim(),
      learner_address: opinion.value.author_address,
    }
    await invoke('submit_evidence_challenge', { params })
    showChallenge.value = false
    // Reload the opinion — the UI doesn't change until resolution, but we
    // want the user to see any `withdrawn=1` flip that might happen if a
    // fast-track handler fires.
    await loadOpinion()
  } catch (e) {
    challengeError.value = String(e)
  } finally {
    challengeSubmitting.value = false
  }
}

function formatDate(iso: string | null | undefined): string {
  if (!iso) return ''
  try {
    return new Date(iso).toLocaleDateString(undefined, {
      year: 'numeric',
      month: 'short',
      day: 'numeric',
    })
  } catch {
    return iso
  }
}

function formatProof(p: SkillProof): string {
  return `${p.skill_id} (${p.proficiency_level})`
}

// Author's credential proofs (looked up by ID — may not all exist
// locally if we haven't synced them yet)
const linkedProofs = ref<SkillProof[]>([])
async function loadLinkedProofs() {
  if (!opinion.value) return
  try {
    const all = await invoke<SkillProof[]>('list_skill_proofs', {})
    linkedProofs.value = all.filter((p) =>
      opinion.value?.credential_proof_ids.includes(p.id),
    )
  } catch {
    // non-fatal
  }
}

onMounted(async () => {
  await loadOpinion()
  await loadLinkedProofs()
})
</script>

<template>
  <div class="max-w-4xl">
    <button
      type="button"
      class="mb-4 flex items-center gap-1 text-sm text-muted-foreground hover:text-foreground"
      @click="$router.back()"
    >
      <svg class="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
        <path stroke-linecap="round" stroke-linejoin="round" d="M15 19l-7-7 7-7" />
      </svg>
      Back
    </button>

    <div v-if="loading" class="flex justify-center py-12">
      <AppSpinner />
    </div>

    <AppAlert v-else-if="error" type="error">{{ error }}</AppAlert>

    <div v-else-if="opinion">
      <!-- Withdrawn banner -->
      <AppAlert
        v-if="opinion.withdrawn"
        type="warning"
        class="mb-4"
      >
        This opinion has been withdrawn<template v-if="opinion.withdrawn_reason">
          ({{ opinion.withdrawn_reason }})</template>.
      </AppAlert>

      <!-- Video player -->
      <div class="rounded-xl overflow-hidden bg-black mb-6">
        <VideoPlayer :content-cid="opinion.video_cid" :title="opinion.title" />
      </div>

      <!-- Meta -->
      <div class="flex items-start gap-4 mb-4">
        <div class="min-w-0 flex-1">
          <div class="flex items-center gap-2 mb-1">
            <h1 class="text-2xl font-bold text-foreground">{{ opinion.title }}</h1>
            <ProvenanceBadge :provenance="opinion.provenance" />
          </div>
          <div class="mt-2 flex items-center gap-2 text-sm text-muted-foreground">
            <AppBadge v-if="subjectField" variant="secondary">
              {{ subjectField.icon_emoji ? subjectField.icon_emoji + ' ' : '' }}{{ subjectField.name }}
            </AppBadge>
            <span>·</span>
            <span>{{ formatDate(opinion.published_at) }}</span>
            <span v-if="opinion.duration_seconds">·</span>
            <span v-if="opinion.duration_seconds">
              {{ Math.round(opinion.duration_seconds / 60) }} min
            </span>
          </div>
        </div>

        <div class="flex gap-2">
          <AppButton
            v-if="isOwner && !opinion.withdrawn"
            variant="ghost"
            size="sm"
            @click="withdraw"
          >
            Withdraw
          </AppButton>
          <AppButton
            v-if="!isOwner && !opinion.withdrawn"
            variant="ghost"
            size="sm"
            @click="showChallenge = true"
          >
            <svg class="w-4 h-4 mr-1" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
              <path stroke-linecap="round" stroke-linejoin="round" d="M3 3v18h18" />
              <path stroke-linecap="round" stroke-linejoin="round" d="M3 12l6-6 4 4 8-8" />
            </svg>
            Challenge
          </AppButton>
        </div>
      </div>

      <!-- Summary -->
      <p v-if="opinion.summary" class="text-base text-foreground mb-6 leading-relaxed">
        {{ opinion.summary }}
      </p>

      <!-- Author + credentials -->
      <div class="rounded-xl border border-border bg-card p-5 mb-6">
        <h2 class="text-sm font-semibold uppercase tracking-wider text-muted-foreground mb-3">
          Author
        </h2>
        <code class="block text-xs text-muted-foreground break-all mb-4">
          {{ opinion.author_address }}
        </code>

        <h3 class="text-xs font-semibold uppercase tracking-wider text-muted-foreground mb-2">
          Staked credentials
        </h3>
        <div v-if="linkedProofs.length === 0" class="text-xs text-muted-foreground">
          The referenced skill proofs haven't synced to this node yet. The
          signature is verified, but we can't display credential details.
        </div>
        <div v-else class="flex flex-wrap gap-2">
          <AppBadge v-for="p in linkedProofs" :key="p.id" variant="success">
            {{ formatProof(p) }}
          </AppBadge>
        </div>
      </div>
    </div>

    <!-- Challenge modal -->
    <AppModal :open="showChallenge" title="Challenge this opinion" @close="showChallenge = false">
      <div class="space-y-4">
        <AppAlert type="info">
          Challenging costs a minimum 5 ADA stake. If the DAO committee upholds
          your challenge, the opinion is marked withdrawn on all honoring
          nodes. If rejected, your stake is slashed.
        </AppAlert>

        <AppTextarea
          v-model="challengeReason"
          label="Reason"
          placeholder="Why does this opinion merit takedown? Be specific."
          :rows="4"
        />

        <AppInput
          v-model="challengeDaoId"
          label="DAO ID"
          placeholder="ID of the subject-field DAO that reviews this challenge"
        />

        <div>
          <label class="mb-1 block text-sm font-medium text-foreground">
            Stake (ADA)
          </label>
          <input
            v-model.number="challengeStake"
            type="number"
            min="5"
            step="0.5"
            class="w-full rounded-md border border-border bg-background px-3 py-2 text-sm"
          />
        </div>

        <AppAlert v-if="challengeError" type="error">{{ challengeError }}</AppAlert>
      </div>
      <template #footer>
        <AppButton variant="ghost" @click="showChallenge = false">Cancel</AppButton>
        <AppButton :loading="challengeSubmitting" @click="submitChallenge">
          Submit challenge
        </AppButton>
      </template>
    </AppModal>
  </div>
</template>
