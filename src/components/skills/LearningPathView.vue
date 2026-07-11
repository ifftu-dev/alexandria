<script setup lang="ts">
import { useI18n } from 'vue-i18n'
import { useRouter } from 'vue-router'
import type { LearningPath, LearningStepStatus } from '@/types'

defineProps<{ path: LearningPath }>()

const { t } = useI18n()
const router = useRouter()

const statusMeta: Record<LearningStepStatus, { dot: string; label: string; cls: string }> = {
  earned: { dot: '✓', label: t('skills.path.statusEarned'), cls: 'step--earned' },
  available: { dot: '▶', label: t('skills.path.statusAvailable'), cls: 'step--available' },
  locked: { dot: '○', label: t('skills.path.statusLocked'), cls: 'step--locked' },
}
</script>

<template>
  <div>
    <div v-if="path.steps.length === 0" class="text-sm text-muted-foreground">
      {{ $t('skills.path.empty') }}
    </div>

    <ol v-else class="space-y-1.5">
      <li
        v-for="step in path.steps"
        :key="step.skill_id"
        class="step"
        :class="statusMeta[step.status].cls"
      >
        <span class="step-dot">{{ statusMeta[step.status].dot }}</span>
        <div class="min-w-0 flex-1">
          <div class="flex items-center gap-2">
            <button
              class="truncate text-sm font-medium text-foreground hover:text-primary"
              @click="router.push(`/skills/${step.skill_id}`)"
            >
              {{ step.name }}
            </button>
            <span v-if="step.is_goal" class="goal-badge">{{ $t('skills.path.goalBadge') }}</span>
            <span class="ml-auto shrink-0 text-[0.65rem] uppercase tracking-wide text-muted-foreground">
              {{ statusMeta[step.status].label }}
            </span>
          </div>

          <!-- Course recommendations for unproven, unlocked skills -->
          <div
            v-if="step.status === 'available' && step.course_recs.length > 0"
            class="mt-1 flex flex-wrap gap-1.5"
          >
            <button
              v-for="c in step.course_recs"
              :key="c.id"
              class="course-pill"
              @click="router.push(`/learn/${c.id}`)"
            >
              {{ c.title }}
            </button>
          </div>
          <p
            v-else-if="step.status === 'available' && step.course_recs.length === 0"
            class="mt-0.5 text-[0.7rem] text-muted-foreground"
          >
            {{ $t('skills.path.noCourse') }}
          </p>
        </div>
      </li>
    </ol>
  </div>
</template>

<style scoped>
.step {
  display: flex;
  align-items: flex-start;
  gap: 0.6rem;
  padding: 0.4rem 0.5rem;
  border-radius: 0.5rem;
}
.step--available {
  background: color-mix(in srgb, var(--app-primary) 8%, transparent);
}
.step-dot {
  width: 1.1rem;
  text-align: center;
  font-size: 0.8rem;
  line-height: 1.4;
  flex-shrink: 0;
}
.step--earned .step-dot {
  color: var(--app-success);
}
.step--available .step-dot {
  color: var(--app-primary);
}
.step--locked .step-dot {
  color: var(--app-muted-foreground);
}
.step--locked {
  opacity: 0.6;
}
.goal-badge {
  font-size: 0.6rem;
  font-weight: 600;
  color: var(--app-accent, var(--app-primary));
  background: color-mix(in srgb, var(--app-primary) 14%, transparent);
  padding: 0.05rem 0.35rem;
  border-radius: 999px;
  flex-shrink: 0;
}
.course-pill {
  font-size: 0.7rem;
  padding: 0.1rem 0.5rem;
  border-radius: 999px;
  background: var(--app-muted);
  color: var(--app-foreground);
  transition: background 0.15s;
}
.course-pill:hover {
  background: color-mix(in srgb, var(--app-primary) 18%, var(--app-muted));
}
</style>
