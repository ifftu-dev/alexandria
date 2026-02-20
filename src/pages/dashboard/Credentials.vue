<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { useLocalApi } from '@/composables/useLocalApi'
import { AppSpinner, EmptyState, AppBadge, AppButton } from '@/components/ui'
import type { SkillProof } from '@/types'

const { invoke } = useLocalApi()

const proofs = ref<SkillProof[]>([])
const loading = ref(true)
const minting = ref<string | null>(null)

onMounted(async () => {
  try {
    proofs.value = await invoke<SkillProof[]>('list_skill_proofs')
  } catch (e) {
    console.error('Failed to load proofs:', e)
  } finally {
    loading.value = false
  }
})

async function mintNft(proof: SkillProof) {
  minting.value = proof.id
  try {
    await invoke('mint_skill_proof_nft', {
      skillId: proof.skill_id,
      proficiencyLevel: proof.proficiency_level,
    })
    // Refresh
    proofs.value = await invoke<SkillProof[]>('list_skill_proofs')
  } catch (e) {
    console.error('Failed to mint:', e)
  } finally {
    minting.value = null
  }
}
</script>

<template>
  <div>
    <h1 class="text-xl font-bold mb-1">Credentials</h1>
    <p class="text-sm text-[rgb(var(--color-muted-foreground))] mb-6">
      Your skill proofs that can be minted as Cardano NFTs. Each credential is a verifiable, soulbound token.
    </p>

    <AppSpinner v-if="loading" label="Loading credentials..." />

    <EmptyState
      v-else-if="proofs.length === 0"
      title="No credentials yet"
      description="Earn skill proofs by completing assessments, then mint them as on-chain credentials."
    />

    <div v-else class="space-y-3">
      <div v-for="proof in proofs" :key="proof.id" class="card p-4 flex items-center justify-between">
        <div>
          <div class="text-sm font-medium">{{ proof.skill_id }}</div>
          <div class="flex items-center gap-2 mt-1">
            <AppBadge variant="primary">{{ proof.proficiency_level }}</AppBadge>
            <span class="text-xs text-[rgb(var(--color-muted-foreground))]">
              {{ (proof.confidence * 100).toFixed(0) }}% confidence
            </span>
          </div>
        </div>
        <AppButton
          size="sm"
          :loading="minting === proof.id"
          @click="mintNft(proof)"
        >
          Mint NFT
        </AppButton>
      </div>
    </div>
  </div>
</template>
