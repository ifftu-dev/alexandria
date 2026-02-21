<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useLocalApi } from '@/composables/useLocalApi'
import { AppSpinner, AppBadge, AppButton, DataRow, EmptyState } from '@/components/ui'
import type { SkillDetail, SkillProof, EvidenceRecord } from '@/types'

const { invoke } = useLocalApi()
const route = useRoute()
const router = useRouter()

const skillId = route.params.id as string

const detail = ref<SkillDetail | null>(null)
const proofs = ref<SkillProof[]>([])
const evidence = ref<EvidenceRecord[]>([])
const loading = ref(true)
const error = ref<string | null>(null)

onMounted(async () => {
  try {
    const [d, allProofs, allEvidence] = await Promise.all([
      invoke<SkillDetail>('get_skill', { skillId }),
      invoke<SkillProof[]>('list_skill_proofs'),
      invoke<EvidenceRecord[]>('list_evidence', { skillId }),
    ])
    detail.value = d
    proofs.value = allProofs.filter(p => p.skill_id === skillId)
    evidence.value = allEvidence
  } catch (e: any) {
    error.value = typeof e === 'string' ? e : e?.message ?? 'Failed to load skill'
    console.error('Failed to load skill:', e)
  } finally {
    loading.value = false
  }
})

function goToSkill(id: string) {
  router.push(`/skills/${id}`)
}

const bloomColors: Record<string, string> = {
  remember: 'secondary',
  understand: 'primary',
  apply: 'accent',
  analyze: 'warning',
  evaluate: 'success',
  create: 'governance',
}

const relationLabels: Record<string, string> = {
  related: 'Related',
  complementary: 'Complementary',
  alternative: 'Alternative',
}
</script>

<template>
  <div class="space-y-6">
    <!-- Back button -->
    <AppButton variant="ghost" size="sm" @click="router.push('/skills')">
      Back to Taxonomy
    </AppButton>

    <AppSpinner v-if="loading" label="Loading skill..." />

    <div v-else-if="error" class="card p-8 text-center">
      <p class="text-sm text-[rgb(var(--color-error))]">{{ error }}</p>
      <AppButton variant="ghost" size="sm" class="mt-4" @click="router.push('/skills')">
        Back to Skills
      </AppButton>
    </div>

    <template v-else-if="detail">
      <!-- Skill header -->
      <div class="card p-5">
        <div class="flex items-start justify-between gap-4">
          <div class="min-w-0 flex-1">
            <div class="flex items-center gap-3 mb-2">
              <h1 class="text-xl font-bold text-[rgb(var(--color-foreground))]">
                {{ detail.skill.name }}
              </h1>
              <AppBadge
                :variant="(bloomColors[detail.skill.bloom_level] as any) ?? 'secondary'"
              >
                {{ detail.skill.bloom_level }}
              </AppBadge>
            </div>
            <p v-if="detail.skill.description" class="text-sm text-[rgb(var(--color-muted-foreground))] mb-3">
              {{ detail.skill.description }}
            </p>
            <div class="flex flex-wrap gap-4 text-xs text-[rgb(var(--color-muted-foreground))]">
              <DataRow v-if="detail.skill.subject_field_name" label="Field">
                {{ detail.skill.subject_field_name }}
              </DataRow>
              <DataRow v-if="detail.skill.subject_name" label="Subject">
                {{ detail.skill.subject_name }}
              </DataRow>
              <DataRow label="Bloom Level">
                {{ detail.skill.bloom_level }}
              </DataRow>
            </div>
          </div>

          <!-- Proof summary (if proven) -->
          <div v-if="proofs.length > 0" class="text-right flex-shrink-0">
            <p class="font-mono text-2xl font-bold text-[rgb(var(--color-primary))]">
              {{ (proofs[0]!.confidence * 100).toFixed(0) }}%
            </p>
            <p class="text-xs text-[rgb(var(--color-muted-foreground))]">confidence</p>
            <AppBadge variant="success" class="mt-1">Proven</AppBadge>
          </div>
        </div>
      </div>

      <!-- Prerequisites & Dependents -->
      <div class="grid grid-cols-1 sm:grid-cols-2 gap-4">
        <!-- Prerequisites -->
        <div class="card p-5">
          <h2 class="text-sm font-semibold text-[rgb(var(--color-foreground))] mb-3">
            Prerequisites
            <span class="text-[rgb(var(--color-muted-foreground))] font-normal ml-1">
              ({{ detail.prerequisites.length }})
            </span>
          </h2>
          <p v-if="detail.prerequisites.length === 0" class="text-xs text-[rgb(var(--color-muted-foreground))] italic">
            No prerequisites -- this is a foundational skill.
          </p>
          <div v-else class="space-y-2">
            <div
              v-for="prereq in detail.prerequisites"
              :key="prereq.id"
              class="flex items-center justify-between rounded-md px-3 py-2 cursor-pointer transition-colors hover:bg-[rgb(var(--color-muted)/0.5)]"
              @click="goToSkill(prereq.id)"
            >
              <div class="flex items-center gap-2 min-w-0">
                <svg class="w-3.5 h-3.5 text-[rgb(var(--color-muted-foreground))] flex-shrink-0" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                  <path stroke-linecap="round" stroke-linejoin="round" d="M9 5l7 7-7 7" />
                </svg>
                <span class="text-sm truncate">{{ prereq.name }}</span>
              </div>
              <AppBadge :variant="(bloomColors[prereq.bloom_level] as any) ?? 'secondary'" class="text-[0.6rem] flex-shrink-0">
                {{ prereq.bloom_level }}
              </AppBadge>
            </div>
          </div>
        </div>

        <!-- Dependents -->
        <div class="card p-5">
          <h2 class="text-sm font-semibold text-[rgb(var(--color-foreground))] mb-3">
            Dependents
            <span class="text-[rgb(var(--color-muted-foreground))] font-normal ml-1">
              ({{ detail.dependents.length }})
            </span>
          </h2>
          <p v-if="detail.dependents.length === 0" class="text-xs text-[rgb(var(--color-muted-foreground))] italic">
            No skills depend on this one yet.
          </p>
          <div v-else class="space-y-2">
            <div
              v-for="dep in detail.dependents"
              :key="dep.id"
              class="flex items-center justify-between rounded-md px-3 py-2 cursor-pointer transition-colors hover:bg-[rgb(var(--color-muted)/0.5)]"
              @click="goToSkill(dep.id)"
            >
              <div class="flex items-center gap-2 min-w-0">
                <svg class="w-3.5 h-3.5 text-[rgb(var(--color-muted-foreground))] flex-shrink-0" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                  <path stroke-linecap="round" stroke-linejoin="round" d="M9 5l7 7-7 7" />
                </svg>
                <span class="text-sm truncate">{{ dep.name }}</span>
              </div>
              <AppBadge :variant="(bloomColors[dep.bloom_level] as any) ?? 'secondary'" class="text-[0.6rem] flex-shrink-0">
                {{ dep.bloom_level }}
              </AppBadge>
            </div>
          </div>
        </div>
      </div>

      <!-- Related Skills -->
      <div v-if="detail.related.length > 0" class="card p-5">
        <h2 class="text-sm font-semibold text-[rgb(var(--color-foreground))] mb-3">
          Related Skills
          <span class="text-[rgb(var(--color-muted-foreground))] font-normal ml-1">
            ({{ detail.related.length }})
          </span>
        </h2>
        <div class="space-y-2">
          <div
            v-for="rel in detail.related"
            :key="rel.skill_id"
            class="flex items-center justify-between rounded-md px-3 py-2 cursor-pointer transition-colors hover:bg-[rgb(var(--color-muted)/0.5)]"
            @click="goToSkill(rel.skill_id)"
          >
            <div class="flex items-center gap-2 min-w-0">
              <span class="text-sm truncate">{{ rel.skill_name }}</span>
              <AppBadge :variant="(bloomColors[rel.bloom_level] as any) ?? 'secondary'" class="text-[0.6rem]">
                {{ rel.bloom_level }}
              </AppBadge>
            </div>
            <span class="text-xs text-[rgb(var(--color-muted-foreground))]">
              {{ relationLabels[rel.relation_type] ?? rel.relation_type }}
            </span>
          </div>
        </div>
      </div>

      <!-- Skill Proofs -->
      <div v-if="proofs.length > 0" class="card p-5">
        <h2 class="text-sm font-semibold text-[rgb(var(--color-foreground))] mb-3">Skill Proofs</h2>
        <div class="space-y-3">
          <div v-for="proof in proofs" :key="proof.id">
            <div class="flex items-center justify-between mb-1">
              <AppBadge :variant="(bloomColors[proof.proficiency_level] as any) ?? 'secondary'">
                {{ proof.proficiency_level }}
              </AppBadge>
              <span class="text-sm font-medium text-[rgb(var(--color-foreground))]">
                {{ (proof.confidence * 100).toFixed(1) }}% confidence
              </span>
            </div>
            <div class="h-1.5 rounded-full bg-[rgb(var(--color-muted))] overflow-hidden">
              <div
                class="h-full rounded-full bg-[rgb(var(--color-primary))] transition-all duration-500"
                :style="{ width: `${proof.confidence * 100}%` }"
              />
            </div>
            <div class="text-xs text-[rgb(var(--color-muted-foreground))] mt-1">
              {{ proof.evidence_count }} evidence record{{ proof.evidence_count !== 1 ? 's' : '' }}
            </div>
          </div>
        </div>
      </div>

      <!-- Evidence Records -->
      <div class="card p-5">
        <h2 class="text-sm font-semibold text-[rgb(var(--color-foreground))] mb-3">Evidence Records</h2>
        <EmptyState
          v-if="evidence.length === 0"
          title="No evidence yet"
          description="Evidence records are created when you complete assessments linked to this skill."
        />
        <div v-else class="space-y-2">
          <div
            v-for="ev in evidence"
            :key="ev.id"
            class="rounded-lg bg-[rgb(var(--color-muted)/0.3)] p-3"
          >
            <div class="flex items-center justify-between mb-1">
              <AppBadge :variant="(bloomColors[ev.proficiency_level] as any) ?? 'secondary'" class="text-[0.6rem]">
                {{ ev.proficiency_level }}
              </AppBadge>
              <span class="text-sm font-medium text-[rgb(var(--color-foreground))]">
                {{ (ev.score * 100).toFixed(0) }}%
              </span>
            </div>
            <div class="grid grid-cols-3 gap-2 text-xs text-[rgb(var(--color-muted-foreground))]">
              <DataRow label="Difficulty">{{ ev.difficulty.toFixed(2) }}</DataRow>
              <DataRow label="Trust">{{ ev.trust_factor.toFixed(2) }}</DataRow>
              <DataRow label="Date">{{ ev.created_at }}</DataRow>
            </div>
          </div>
        </div>
      </div>
    </template>
  </div>
</template>
