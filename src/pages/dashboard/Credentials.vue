<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
import { useLocalApi } from '@/composables/useLocalApi'
import { AppBadge, AppButton } from '@/components/ui'
import type { SkillProof, SkillDetail } from '@/types'

const { invoke } = useLocalApi()

const proofs = ref<SkillProof[]>([])
const skillNames = ref<Record<string, string>>({})
const loading = ref(true)
const minting = ref<string | null>(null)

// Stats
const totalProofs = computed(() => proofs.value.length)
const avgConfidence = computed(() => {
  if (proofs.value.length === 0) return 0
  return proofs.value.reduce((s, p) => s + p.confidence, 0) / proofs.value.length
})

/** Resolve skill_id → display name, with cache */
async function resolveSkillName(skillId: string): Promise<string> {
  if (skillNames.value[skillId]) return skillNames.value[skillId]
  try {
    const detail = await invoke<SkillDetail>('get_skill', { skillId })
    skillNames.value[skillId] = detail.skill.name
    return detail.skill.name
  } catch {
    return skillId // fallback to raw ID
  }
}

onMounted(async () => {
  try {
    proofs.value = await invoke<SkillProof[]>('list_skill_proofs')
    // Pre-fetch skill names for all proofs
    const uniqueSkillIds = [...new Set(proofs.value.map(p => p.skill_id))]
    await Promise.all(uniqueSkillIds.map(id => resolveSkillName(id)))
  } catch (e) {
    console.error('Failed to load proofs:', e)
  } finally {
    loading.value = false
  }
})

async function mintNft(proof: SkillProof) {
  minting.value = proof.id
  try {
    const skillName = await resolveSkillName(proof.skill_id)
    await invoke('mint_skill_proof_nft', {
      proofId: proof.id,
      skillName,
      proficiencyLevel: proof.proficiency_level,
      confidence: proof.confidence,
    })
    proofs.value = await invoke<SkillProof[]>('list_skill_proofs')
  } catch (e) {
    console.error('Failed to mint:', e)
  } finally {
    minting.value = null
  }
}
</script>

<template>
  <div class="py-8 px-4 sm:px-6 lg:px-8">
    <!-- Header -->
    <div class="mb-8">
      <h1 class="text-3xl font-bold text-[rgb(var(--color-foreground))]">Credentials</h1>
      <p class="mt-2 text-[rgb(var(--color-muted-foreground))]">
        Your skill proofs that can be minted as Cardano NFTs. Each credential is a verifiable, soulbound token.
      </p>
    </div>

    <!-- Skeleton -->
    <div v-if="loading" class="space-y-6">
      <div class="grid gap-4 sm:grid-cols-3">
        <div v-for="i in 3" :key="i" class="animate-pulse rounded-xl border border-[rgb(var(--color-border))] bg-[rgb(var(--color-card))] p-6">
          <div class="h-3 w-20 rounded bg-[rgb(var(--color-muted-foreground)/0.15)] mb-3" />
          <div class="h-8 w-12 rounded bg-[rgb(var(--color-muted-foreground)/0.2)]" />
        </div>
      </div>
      <div class="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
        <div v-for="i in 3" :key="i" class="animate-pulse rounded-xl border border-[rgb(var(--color-border))] bg-[rgb(var(--color-card))] p-5">
          <div class="h-4 w-36 rounded bg-[rgb(var(--color-muted-foreground)/0.15)] mb-3" />
          <div class="h-5 w-16 rounded-full bg-[rgb(var(--color-muted-foreground)/0.1)] mb-3" />
          <div class="h-1.5 w-full rounded-full bg-[rgb(var(--color-muted-foreground)/0.1)]" />
        </div>
      </div>
    </div>

    <template v-else>
      <!-- Stats -->
      <div class="mb-8 grid gap-4 sm:grid-cols-3">
        <div class="rounded-xl border border-[rgb(var(--color-border))] bg-[rgb(var(--color-card))] p-6">
          <p class="text-sm text-[rgb(var(--color-muted-foreground))]">Total Credentials</p>
          <p class="mt-2 text-3xl font-bold text-[rgb(var(--color-foreground))]">{{ totalProofs }}</p>
        </div>
        <div class="rounded-xl border border-[rgb(var(--color-border))] bg-[rgb(var(--color-card))] p-6">
          <p class="text-sm text-[rgb(var(--color-muted-foreground))]">Avg. Confidence</p>
          <p class="mt-2 text-3xl font-bold text-[rgb(var(--color-primary))]">{{ Math.round(avgConfidence * 100) }}%</p>
        </div>
        <div class="rounded-xl border border-[rgb(var(--color-border))] bg-[rgb(var(--color-card))] p-6">
          <p class="text-sm text-[rgb(var(--color-muted-foreground))]">Evidence Records</p>
          <p class="mt-2 text-3xl font-bold text-[rgb(var(--color-foreground))]">{{ proofs.reduce((s, p) => s + p.evidence_count, 0) }}</p>
        </div>
      </div>

      <!-- Empty state -->
      <div v-if="proofs.length === 0" class="py-16 text-center">
        <div class="mx-auto mb-4 flex h-16 w-16 items-center justify-center rounded-full bg-[rgb(var(--color-primary)/0.1)]">
          <svg class="h-8 w-8 text-[rgb(var(--color-primary))]" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.5">
            <path stroke-linecap="round" stroke-linejoin="round" d="M9 12.75L11.25 15 15 9.75M21 12c0 1.268-.63 2.39-1.593 3.068a3.745 3.745 0 01-1.043 3.296 3.745 3.745 0 01-3.296 1.043A3.745 3.745 0 0112 21c-1.268 0-2.39-.63-3.068-1.593a3.746 3.746 0 01-3.296-1.043 3.745 3.745 0 01-1.043-3.296A3.745 3.745 0 013 12c0-1.268.63-2.39 1.593-3.068a3.745 3.745 0 011.043-3.296 3.746 3.746 0 013.296-1.043A3.746 3.746 0 0112 3c1.268 0 2.39.63 3.068 1.593a3.746 3.746 0 013.296 1.043 3.746 3.746 0 011.043 3.296A3.745 3.745 0 0121 12z" />
          </svg>
        </div>
        <h3 class="text-lg font-semibold text-[rgb(var(--color-foreground))]">No credentials yet</h3>
        <p class="mt-1 text-sm text-[rgb(var(--color-muted-foreground))] max-w-sm mx-auto">
          Earn skill proofs by completing assessments, then mint them as on-chain credentials.
        </p>
      </div>

      <!-- Credential cards -->
      <div v-else class="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
        <div
          v-for="proof in proofs"
          :key="proof.id"
          class="rounded-xl border border-[rgb(var(--color-border))] bg-[rgb(var(--color-card))] p-5 transition-colors hover:border-[rgb(var(--color-primary)/0.3)]"
        >
          <div class="flex items-start justify-between mb-3">
            <div class="min-w-0 flex-1">
              <p class="text-sm font-semibold text-[rgb(var(--color-foreground))] truncate">{{ skillNames[proof.skill_id] || proof.skill_id }}</p>
              <div class="mt-1.5 flex items-center gap-2">
                <AppBadge variant="primary">{{ proof.proficiency_level }}</AppBadge>
                <span class="text-xs text-[rgb(var(--color-muted-foreground))]">
                  {{ proof.evidence_count }} evidence
                </span>
              </div>
            </div>
            <div class="text-right flex-shrink-0">
              <p class="font-mono text-lg font-bold text-[rgb(var(--color-primary))]">
                {{ (proof.confidence * 100).toFixed(0) }}%
              </p>
              <p class="text-[10px] text-[rgb(var(--color-muted-foreground))]">confidence</p>
            </div>
          </div>

          <!-- Confidence bar -->
          <div class="mb-4">
            <div class="h-1.5 overflow-hidden rounded-full bg-[rgb(var(--color-muted)/0.3)]">
              <div
                class="h-full rounded-full bg-[rgb(var(--color-primary))] transition-all duration-500"
                :style="{ width: `${proof.confidence * 100}%` }"
              />
            </div>
          </div>

          <!-- Mint button -->
          <AppButton
            size="sm"
            class="w-full"
            :loading="minting === proof.id"
            @click="mintNft(proof)"
          >
            <svg class="mr-1.5 h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
              <path stroke-linecap="round" stroke-linejoin="round" d="M9 12.75L11.25 15 15 9.75M21 12c0 1.268-.63 2.39-1.593 3.068a3.745 3.745 0 01-1.043 3.296 3.745 3.745 0 01-3.296 1.043A3.745 3.745 0 0112 21c-1.268 0-2.39-.63-3.068-1.593a3.746 3.746 0 01-3.296-1.043 3.745 3.745 0 01-1.043-3.296A3.745 3.745 0 013 12c0-1.268.63-2.39 1.593-3.068a3.745 3.745 0 011.043-3.296 3.746 3.746 0 013.296-1.043A3.746 3.746 0 0112 3c1.268 0 2.39.63 3.068 1.593a3.746 3.746 0 013.296 1.043 3.746 3.746 0 011.043 3.296A3.745 3.745 0 0121 12z" />
            </svg>
            Mint NFT
          </AppButton>
        </div>
      </div>
    </template>
  </div>
</template>
