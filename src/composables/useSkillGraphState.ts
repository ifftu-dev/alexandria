import { ref } from 'vue'
import type { SkillInfo, SkillGraphEdge, SkillProof } from '@/types'

/**
 * Shared state for the sidebar skill graph widget.
 *
 * Data is loaded once by SidebarSkillGraph.vue, and the reactive refs
 * are shared across components via module-level singletons.
 *
 * In Tauri (client-only), module-level refs serve as shared state.
 */

// Module-level singletons — shared across all consumers
const skills = ref<SkillInfo[]>([])
const edges = ref<SkillGraphEdge[]>([])
const proofs = ref<SkillProof[]>([])
const earnedSkillIds = ref<Set<string>>(new Set())
const earnedCount = ref(0)
const availableCount = ref(0)
const lockedCount = ref(0)
const totalCount = ref(0)
const loaded = ref(false)
const isModalOpen = ref(false)

export type SkillStatus = 'earned' | 'available' | 'locked'

export function useSkillGraphState() {
  return {
    skills,
    edges,
    proofs,
    earnedSkillIds,
    earnedCount,
    availableCount,
    lockedCount,
    totalCount,
    loaded,
    isModalOpen,
  }
}
