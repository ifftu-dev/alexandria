<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { useLocalApi } from '@/composables/useLocalApi'
import { AppSpinner, EmptyState, AppBadge } from '@/components/ui'
import type { SkillProof } from '@/types'

const { invoke } = useLocalApi()

const proofs = ref<SkillProof[]>([])
const loading = ref(true)

onMounted(async () => {
  try {
    proofs.value = await invoke<SkillProof[]>('list_skill_proofs')
  } catch (e) {
    console.error('Failed to load skill proofs:', e)
  } finally {
    loading.value = false
  }
})

function confidencePercent(c: number): string {
  return `${(c * 100).toFixed(0)}%`
}

const levelColors: Record<string, string> = {
  remember: 'secondary',
  understand: 'primary',
  apply: 'accent',
  analyze: 'warning',
  evaluate: 'success',
  create: 'governance',
}
</script>

<template>
  <div>
    <h1 class="text-xl font-bold mb-1">Skills & Proofs</h1>
    <p class="text-sm text-[rgb(var(--color-muted-foreground))] mb-6">
      Your earned skill proofs with Bloom's taxonomy proficiency levels.
    </p>

    <AppSpinner v-if="loading" label="Loading skill proofs..." />

    <EmptyState
      v-else-if="proofs.length === 0"
      title="No skill proofs yet"
      description="Complete course assessments to earn skill proofs. Each proof attests proficiency at a specific Bloom's taxonomy level."
    />

    <div v-else class="grid grid-cols-1 sm:grid-cols-2 gap-4">
      <div
        v-for="proof in proofs"
        :key="proof.id"
        class="card p-4"
      >
        <div class="flex items-start justify-between mb-2">
          <div>
            <div class="text-sm font-medium">{{ proof.skill_id }}</div>
            <AppBadge :variant="(levelColors[proof.proficiency_level] as any) ?? 'secondary'">
              {{ proof.proficiency_level }}
            </AppBadge>
          </div>
          <div class="text-right">
            <div class="text-lg font-bold text-[rgb(var(--color-primary))]">
              {{ confidencePercent(proof.confidence) }}
            </div>
            <div class="text-xs text-[rgb(var(--color-muted-foreground))]">confidence</div>
          </div>
        </div>

        <!-- Confidence bar -->
        <div class="h-1.5 rounded-full bg-[rgb(var(--color-muted))] overflow-hidden">
          <div
            class="h-full rounded-full bg-[rgb(var(--color-primary))] transition-all duration-500"
            :style="{ width: `${proof.confidence * 100}%` }"
          />
        </div>

        <div class="flex items-center justify-between mt-2 text-xs text-[rgb(var(--color-muted-foreground))]">
          <span>{{ proof.evidence_count }} evidence record{{ proof.evidence_count !== 1 ? 's' : '' }}</span>
          <span>{{ proof.updated_at }}</span>
        </div>
      </div>
    </div>
  </div>
</template>
