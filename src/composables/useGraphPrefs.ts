// Per-skill visibility + teaching preferences for the owner's skill
// graph. Backed by the `instructor.graph_prefs` synced setting:
//   { "<skill_id>": { public: boolean, teaching: boolean } }
// An absent entry means public + not-teaching (earned skills are public
// by default — see src-tauri/src/p2p/graph_fetch.rs).

import { computed } from 'vue'

import { useSetting } from './useSettings'
import type { GraphNodePref } from '@/types'

type PrefMap = Record<string, GraphNodePref>

const DEFAULT_PREF: GraphNodePref = { public: true, teaching: false }

export function useGraphPrefs() {
  const setting = useSetting<PrefMap>('instructor.graph_prefs')

  const prefs = computed<PrefMap>(() => setting.ref.value ?? {})

  function prefFor(skillId: string): GraphNodePref {
    return prefs.value[skillId] ?? { ...DEFAULT_PREF }
  }

  async function update(skillId: string, patch: Partial<GraphNodePref>): Promise<void> {
    const current = prefFor(skillId)
    const next: PrefMap = {
      ...prefs.value,
      [skillId]: { ...current, ...patch },
    }
    await setting.set(next)
  }

  function setPublic(skillId: string, value: boolean): Promise<void> {
    return update(skillId, { public: value })
  }

  function setTeaching(skillId: string, value: boolean): Promise<void> {
    return update(skillId, { teaching: value })
  }

  /** Bulk-apply a patch to many skills at once (single write). */
  async function updateMany(skillIds: string[], patch: Partial<GraphNodePref>): Promise<void> {
    const next: PrefMap = { ...prefs.value }
    for (const id of skillIds) {
      next[id] = { ...(next[id] ?? { ...DEFAULT_PREF }), ...patch }
    }
    await setting.set(next)
  }

  return { prefs, prefFor, setPublic, setTeaching, updateMany }
}
