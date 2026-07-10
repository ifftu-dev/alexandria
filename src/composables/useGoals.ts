// Learning goals — skill graphs the user is working toward.
//
// Goals persist in the `learner.targets` synced setting (a JSON
// array), so they follow the user across devices. The learning path
// for a goal is computed on the backend from the user's earned
// skills (`compute_learning_path`).

import { computed } from 'vue'

import { useSetting } from './useSettings'
import { useLocalApi } from './useLocalApi'
import type { LearningPath, Goal, GoalTemplate, GoalResolution, GoalInput } from '@/types'

function genId(): string {
  const c = globalThis.crypto
  if (c && 'randomUUID' in c) return c.randomUUID()
  return `t_${Date.now()}_${Math.random().toString(36).slice(2, 10)}`
}

export function useGoals() {
  const { invoke } = useLocalApi()
  const setting = useSetting<Goal[]>('learner.targets')

  const goals = computed<Goal[]>(() => setting.ref.value ?? [])

  async function addGoal(input: {
    label: string
    goalSkillIds: string[]
    sourceDid?: string | null
    kind?: Goal['kind']
    sourceKey?: string
    sourceUrl?: string
    resolutionProvenance?: Goal['resolution_provenance']
    taxonomyVersion?: string
  }): Promise<Goal> {
    const goal: Goal = {
      id: genId(),
      label: input.label,
      source_did: input.sourceDid ?? null,
      goal_skill_ids: input.goalSkillIds,
      created_at: new Date().toISOString(),
      kind: input.kind,
      source_key: input.sourceKey,
      source_url: input.sourceUrl,
      resolution_provenance: input.resolutionProvenance,
      taxonomy_version: input.taxonomyVersion,
    }
    await setting.set([...(setting.ref.value ?? []), goal])
    return goal
  }

  /** Curated goal templates, optionally filtered by kind. */
  function listGoalTemplates(kind?: GoalTemplate['kind']): Promise<GoalTemplate[]> {
    return invoke<GoalTemplate[]>('list_goal_templates', { kind: kind ?? null })
  }

  /** Resolve a goal input to target skills (curated map) or suggestions (JD). */
  function resolveGoal(input: GoalInput): Promise<GoalResolution> {
    return invoke<GoalResolution>('resolve_goal', { input })
  }

  async function removeGoal(id: string): Promise<void> {
    await setting.set((setting.ref.value ?? []).filter((t) => t.id !== id))
  }

  /** True if a goal already covers exactly this goal set (same DID). */
  function hasGoalForDid(did: string): boolean {
    return goals.value.some((t) => t.source_did === did)
  }

  /** Learning path for one goal. */
  function pathFor(goal: Goal): Promise<LearningPath> {
    return invoke<LearningPath>('compute_learning_path', {
      goalSkillIds: goal.goal_skill_ids,
    })
  }

  /** Merged path across every goal (deduped goal set). */
  function combinedPath(): Promise<LearningPath> {
    const all = [...new Set(goals.value.flatMap((t) => t.goal_skill_ids))]
    return invoke<LearningPath>('compute_learning_path', { goalSkillIds: all })
  }

  return {
    goals,
    addGoal,
    removeGoal,
    hasGoalForDid,
    pathFor,
    combinedPath,
    listGoalTemplates,
    resolveGoal,
  }
}
