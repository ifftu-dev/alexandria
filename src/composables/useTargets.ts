// Learning targets — skill graphs the user is working toward.
//
// Targets persist in the `learner.targets` synced setting (a JSON
// array), so they follow the user across devices. The learning path
// for a target is computed on the backend from the user's earned
// skills (`compute_learning_path`).

import { computed } from 'vue'

import { useSetting } from './useSettings'
import { useLocalApi } from './useLocalApi'
import type { LearningPath, Target } from '@/types'

function genId(): string {
  const c = globalThis.crypto
  if (c && 'randomUUID' in c) return c.randomUUID()
  return `t_${Date.now()}_${Math.random().toString(36).slice(2, 10)}`
}

export function useTargets() {
  const { invoke } = useLocalApi()
  const setting = useSetting<Target[]>('learner.targets')

  const targets = computed<Target[]>(() => setting.ref.value ?? [])

  async function addTarget(input: {
    label: string
    goalSkillIds: string[]
    sourceDid?: string | null
  }): Promise<Target> {
    const target: Target = {
      id: genId(),
      label: input.label,
      source_did: input.sourceDid ?? null,
      goal_skill_ids: input.goalSkillIds,
      created_at: new Date().toISOString(),
    }
    await setting.set([...(setting.ref.value ?? []), target])
    return target
  }

  async function removeTarget(id: string): Promise<void> {
    await setting.set((setting.ref.value ?? []).filter((t) => t.id !== id))
  }

  /** True if a target already covers exactly this goal set (same DID). */
  function hasTargetForDid(did: string): boolean {
    return targets.value.some((t) => t.source_did === did)
  }

  /** Learning path for one target. */
  function pathFor(target: Target): Promise<LearningPath> {
    return invoke<LearningPath>('compute_learning_path', {
      goalSkillIds: target.goal_skill_ids,
    })
  }

  /** Merged path across every target (deduped goal set). */
  function combinedPath(): Promise<LearningPath> {
    const all = [...new Set(targets.value.flatMap((t) => t.goal_skill_ids))]
    return invoke<LearningPath>('compute_learning_path', { goalSkillIds: all })
  }

  return {
    targets,
    addTarget,
    removeTarget,
    hasTargetForDid,
    pathFor,
    combinedPath,
  }
}
