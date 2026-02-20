<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { useRoute } from 'vue-router'
import { useLocalApi } from '@/composables/useLocalApi'
import { AppSpinner, EmptyState, AppBadge, DataRow } from '@/components/ui'
import type { SkillProof, EvidenceRecord } from '@/types'

const { invoke } = useLocalApi()
const route = useRoute()

const skillId = route.params.id as string

const proofs = ref<SkillProof[]>([])
const evidence = ref<EvidenceRecord[]>([])
const loading = ref(true)

onMounted(async () => {
  try {
    const [allProofs, allEvidence] = await Promise.all([
      invoke<SkillProof[]>('list_skill_proofs'),
      invoke<EvidenceRecord[]>('list_evidence'),
    ])
    proofs.value = allProofs.filter(p => p.skill_id === skillId)
    evidence.value = allEvidence.filter(e => e.skill_id === skillId)
  } catch (e) {
    console.error('Failed to load skill data:', e)
  } finally {
    loading.value = false
  }
})
</script>

<template>
  <div>
    <AppSpinner v-if="loading" label="Loading skill..." />

    <div v-else>
      <h1 class="text-xl font-bold mb-1">{{ skillId }}</h1>
      <p class="text-sm text-[rgb(var(--color-muted-foreground))] mb-6">
        Skill proofs and evidence records for this skill.
      </p>

      <!-- Proofs -->
      <div v-if="proofs.length > 0" class="card p-5 mb-6">
        <h2 class="text-base font-semibold mb-3">Skill Proofs</h2>
        <div class="space-y-3">
          <div v-for="proof in proofs" :key="proof.id" class="flex items-center justify-between">
            <div class="flex items-center gap-2">
              <AppBadge variant="primary">{{ proof.proficiency_level }}</AppBadge>
              <span class="text-sm">{{ (proof.confidence * 100).toFixed(1) }}% confidence</span>
            </div>
            <span class="text-xs text-[rgb(var(--color-muted-foreground))]">
              {{ proof.evidence_count }} evidence
            </span>
          </div>
        </div>
      </div>

      <!-- Evidence Records -->
      <div class="card p-5">
        <h2 class="text-base font-semibold mb-3">Evidence Records</h2>
        <EmptyState
          v-if="evidence.length === 0"
          title="No evidence yet"
          description="Evidence records are created when you complete assessments."
        />
        <div v-else class="space-y-3">
          <div v-for="ev in evidence" :key="ev.id" class="p-3 rounded bg-[rgb(var(--color-muted)/0.3)]">
            <div class="flex items-center justify-between mb-1">
              <AppBadge variant="primary">{{ ev.proficiency_level }}</AppBadge>
              <span class="text-sm font-medium">Score: {{ (ev.score * 100).toFixed(0) }}%</span>
            </div>
            <div class="space-y-1 text-xs text-[rgb(var(--color-muted-foreground))]">
              <DataRow label="Difficulty">{{ ev.difficulty.toFixed(2) }}</DataRow>
              <DataRow label="Trust">{{ ev.trust_factor.toFixed(2) }}</DataRow>
              <DataRow label="Date">{{ ev.created_at }}</DataRow>
            </div>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>
