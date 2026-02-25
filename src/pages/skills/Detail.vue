<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useLocalApi } from '@/composables/useLocalApi'
import { AppBadge, EmptyState } from '@/components/ui'
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
  <div>
    <!-- Back button -->
    <button
      class="mb-6 inline-flex items-center gap-1.5 text-xs text-[rgb(var(--color-muted-foreground))] hover:text-[rgb(var(--color-foreground))] transition-colors"
      @click="router.push('/skills')"
    >
      <svg class="w-3.5 h-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
        <path stroke-linecap="round" stroke-linejoin="round" d="M15 19l-7-7 7-7" />
      </svg>
      Back to Taxonomy
    </button>

    <!-- Loading skeleton -->
    <div v-if="loading" class="animate-pulse space-y-6">
      <!-- Header card skeleton -->
      <div class="card p-5">
        <div class="flex items-start justify-between gap-4">
          <div class="min-w-0 flex-1 space-y-3">
            <div class="flex items-center gap-3">
              <div class="h-7 w-56 rounded bg-[rgb(var(--color-muted))]" />
              <div class="h-5 w-20 rounded-full bg-[rgb(var(--color-muted))]" />
            </div>
            <div class="h-4 w-full max-w-md rounded bg-[rgb(var(--color-muted)/0.5)]" />
            <div class="flex gap-3">
              <div class="h-7 w-28 rounded-full bg-[rgb(var(--color-muted)/0.3)]" />
              <div class="h-7 w-24 rounded-full bg-[rgb(var(--color-muted)/0.3)]" />
              <div class="h-7 w-20 rounded-full bg-[rgb(var(--color-muted)/0.3)]" />
            </div>
          </div>
          <div class="text-right space-y-2 shrink-0">
            <div class="h-8 w-16 rounded bg-[rgb(var(--color-muted))] ml-auto" />
            <div class="h-3 w-20 rounded bg-[rgb(var(--color-muted)/0.5)] ml-auto" />
            <div class="h-5 w-16 rounded-full bg-[rgb(var(--color-muted)/0.3)] ml-auto" />
          </div>
        </div>
      </div>

      <!-- 2-col grid skeleton -->
      <div class="grid grid-cols-1 sm:grid-cols-2 gap-4">
        <div class="card p-5 space-y-3">
          <div class="h-4 w-28 rounded bg-[rgb(var(--color-muted))]" />
          <div v-for="i in 3" :key="i" class="h-10 rounded-lg bg-[rgb(var(--color-muted)/0.3)]" />
        </div>
        <div class="card p-5 space-y-3">
          <div class="h-4 w-24 rounded bg-[rgb(var(--color-muted))]" />
          <div v-for="i in 2" :key="i" class="h-10 rounded-lg bg-[rgb(var(--color-muted)/0.3)]" />
        </div>
      </div>

      <!-- Evidence list skeleton -->
      <div class="card p-5 space-y-3">
        <div class="h-4 w-36 rounded bg-[rgb(var(--color-muted))]" />
        <div v-for="i in 3" :key="i" class="rounded-lg bg-[rgb(var(--color-muted)/0.2)] p-4 space-y-3">
          <div class="flex items-center justify-between">
            <div class="h-5 w-20 rounded-full bg-[rgb(var(--color-muted)/0.4)]" />
            <div class="h-5 w-12 rounded bg-[rgb(var(--color-muted)/0.4)]" />
          </div>
          <div class="grid grid-cols-3 gap-4">
            <div class="space-y-1">
              <div class="h-2.5 w-14 rounded bg-[rgb(var(--color-muted)/0.3)]" />
              <div class="h-4 w-10 rounded bg-[rgb(var(--color-muted)/0.4)]" />
            </div>
            <div class="space-y-1">
              <div class="h-2.5 w-10 rounded bg-[rgb(var(--color-muted)/0.3)]" />
              <div class="h-4 w-10 rounded bg-[rgb(var(--color-muted)/0.4)]" />
            </div>
            <div class="space-y-1">
              <div class="h-2.5 w-8 rounded bg-[rgb(var(--color-muted)/0.3)]" />
              <div class="h-4 w-20 rounded bg-[rgb(var(--color-muted)/0.4)]" />
            </div>
          </div>
        </div>
      </div>
    </div>

    <!-- Error state -->
    <div v-else-if="error" class="card p-8">
      <div class="flex flex-col items-center text-center gap-3">
        <div class="flex h-12 w-12 items-center justify-center rounded-full bg-[rgb(var(--color-error)/0.1)]">
          <svg class="w-6 h-6 text-[rgb(var(--color-error))]" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.5">
            <path stroke-linecap="round" stroke-linejoin="round" d="M12 9v3.75m-9.303 3.376c-.866 1.5.217 3.374 1.948 3.374h14.71c1.73 0 2.813-1.874 1.948-3.374L13.949 3.378c-.866-1.5-3.032-1.5-3.898 0L2.697 16.126zM12 15.75h.007v.008H12v-.008z" />
          </svg>
        </div>
        <div>
          <p class="text-sm font-medium text-[rgb(var(--color-foreground))]">Failed to load skill</p>
          <p class="text-xs text-[rgb(var(--color-muted-foreground))] mt-1">{{ error }}</p>
        </div>
        <button
          class="mt-2 inline-flex items-center gap-1.5 text-xs text-[rgb(var(--color-muted-foreground))] hover:text-[rgb(var(--color-foreground))] transition-colors"
          @click="router.push('/skills')"
        >
          <svg class="w-3.5 h-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
            <path stroke-linecap="round" stroke-linejoin="round" d="M15 19l-7-7 7-7" />
          </svg>
          Back to Skills
        </button>
      </div>
    </div>

    <template v-else-if="detail">
      <!-- Skill header card -->
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
            <p v-if="detail.skill.description" class="text-sm text-[rgb(var(--color-muted-foreground))] mb-3 max-w-2xl">
              {{ detail.skill.description }}
            </p>

            <!-- Stat pills -->
            <div class="mt-3 flex flex-wrap items-center gap-3">
              <span
                v-if="detail.skill.subject_field_name"
                class="inline-flex items-center gap-1.5 rounded-full bg-[rgb(var(--color-muted)/0.3)] px-3 py-1 text-xs"
              >
                <svg class="w-3.5 h-3.5 text-[rgb(var(--color-muted-foreground))]" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                  <path stroke-linecap="round" stroke-linejoin="round" d="M19 11H5m14 0a2 2 0 012 2v6a2 2 0 01-2 2H5a2 2 0 01-2-2v-6a2 2 0 012-2m14 0V9a2 2 0 00-2-2M5 11V9a2 2 0 012-2m0 0V5a2 2 0 012-2h6a2 2 0 012 2v2M7 7h10" />
                </svg>
                {{ detail.skill.subject_field_name }}
              </span>
              <span
                v-if="detail.skill.subject_name"
                class="inline-flex items-center gap-1.5 rounded-full bg-[rgb(var(--color-muted)/0.3)] px-3 py-1 text-xs"
              >
                <svg class="w-3.5 h-3.5 text-[rgb(var(--color-muted-foreground))]" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                  <path stroke-linecap="round" stroke-linejoin="round" d="M12 6.253v13m0-13C10.832 5.477 9.246 5 7.5 5S4.168 5.477 3 6.253v13C4.168 18.477 5.754 18 7.5 18s3.332.477 4.5 1.253m0-13C13.168 5.477 14.754 5 16.5 5c1.747 0 3.332.477 4.5 1.253v13C19.832 18.477 18.247 18 16.5 18c-1.746 0-3.332.477-4.5 1.253" />
                </svg>
                {{ detail.skill.subject_name }}
              </span>
              <span class="inline-flex items-center gap-1.5 rounded-full bg-[rgb(var(--color-muted)/0.3)] px-3 py-1 text-xs">
                <svg class="w-3.5 h-3.5 text-[rgb(var(--color-muted-foreground))]" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                  <path stroke-linecap="round" stroke-linejoin="round" d="M3 13.125C3 12.504 3.504 12 4.125 12h2.25c.621 0 1.125.504 1.125 1.125v6.75C7.5 20.496 6.996 21 6.375 21h-2.25A1.125 1.125 0 013 19.875v-6.75zM9.75 8.625c0-.621.504-1.125 1.125-1.125h2.25c.621 0 1.125.504 1.125 1.125v11.25c0 .621-.504 1.125-1.125 1.125h-2.25a1.125 1.125 0 01-1.125-1.125V8.625zM16.5 4.125c0-.621.504-1.125 1.125-1.125h2.25C20.496 3 21 3.504 21 4.125v15.75c0 .621-.504 1.125-1.125 1.125h-2.25a1.125 1.125 0 01-1.125-1.125V4.125z" />
                </svg>
                {{ detail.skill.bloom_level }}
              </span>
            </div>
          </div>

          <!-- Confidence display -->
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
      <div class="grid grid-cols-1 sm:grid-cols-2 gap-4 mt-6">
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
          <div v-else class="space-y-1">
            <div
              v-for="prereq in detail.prerequisites"
              :key="prereq.id"
              class="flex items-center justify-between rounded-lg px-3 py-2.5 cursor-pointer transition-all hover:bg-[rgb(var(--color-muted)/0.4)]"
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
          <div v-else class="space-y-1">
            <div
              v-for="dep in detail.dependents"
              :key="dep.id"
              class="flex items-center justify-between rounded-lg px-3 py-2.5 cursor-pointer transition-all hover:bg-[rgb(var(--color-muted)/0.4)]"
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
      <div v-if="detail.related.length > 0" class="card p-5 mt-6">
        <h2 class="text-sm font-semibold text-[rgb(var(--color-foreground))] mb-3">
          Related Skills
          <span class="text-[rgb(var(--color-muted-foreground))] font-normal ml-1">
            ({{ detail.related.length }})
          </span>
        </h2>
        <div class="space-y-1">
          <div
            v-for="rel in detail.related"
            :key="rel.skill_id"
            class="flex items-center justify-between rounded-lg px-3 py-2.5 cursor-pointer transition-all hover:bg-[rgb(var(--color-muted)/0.4)]"
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
      <div v-if="proofs.length > 0" class="card p-5 mt-6">
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
      <div class="card p-5 mt-6">
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
            <div class="flex items-center justify-between mb-2">
              <AppBadge :variant="(bloomColors[ev.proficiency_level] as any) ?? 'secondary'" class="text-[0.6rem]">
                {{ ev.proficiency_level }}
              </AppBadge>
              <span class="text-sm font-medium font-mono text-[rgb(var(--color-foreground))]">
                {{ (ev.score * 100).toFixed(0) }}%
              </span>
            </div>
            <div class="grid grid-cols-3 gap-4 mt-2">
              <div>
                <p class="text-[10px] uppercase tracking-wider text-[rgb(var(--color-muted-foreground))]">Difficulty</p>
                <p class="text-sm font-medium font-mono">{{ ev.difficulty.toFixed(2) }}</p>
              </div>
              <div>
                <p class="text-[10px] uppercase tracking-wider text-[rgb(var(--color-muted-foreground))]">Trust</p>
                <p class="text-sm font-medium font-mono">{{ ev.trust_factor.toFixed(2) }}</p>
              </div>
              <div>
                <p class="text-[10px] uppercase tracking-wider text-[rgb(var(--color-muted-foreground))]">Date</p>
                <p class="text-sm font-medium font-mono">{{ ev.created_at }}</p>
              </div>
            </div>
          </div>
        </div>
      </div>
    </template>
  </div>
</template>
