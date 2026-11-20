// The remembered skill graph view, one profile preference per surface: the
// skills page remembers its view in the `ui.skill_graph_view` synced setting
// (default Simple), and the graph expanded from the sidebar in
// `ui.skill_graph_expanded_view` (default Web). The backend holds both to
// `web`, `simple` or `detailed`.

import { computed, type WritableComputedRef } from 'vue'

import type { AtlasSurface } from './useSkillAtlas'
import { useSetting } from './useSettings'

export const SKILL_GRAPH_VIEWS = ['web', 'simple', 'detailed'] as const
export type SkillGraphView = typeof SKILL_GRAPH_VIEWS[number]

export function isSkillGraphView(value: unknown): value is SkillGraphView {
  return SKILL_GRAPH_VIEWS.some(view => view === value)
}

/** Each surface's settings key and the view it shows until the learner chooses. */
export const SKILL_GRAPH_VIEW_SETTINGS = {
  page: { key: 'ui.skill_graph_view', fallback: 'simple' },
  modal: { key: 'ui.skill_graph_expanded_view', fallback: 'web' },
} as const satisfies Record<AtlasSurface, { key: string; fallback: SkillGraphView }>

/**
 * Two-way ref onto a surface's remembered view. Reads that surface's default
 * whenever its setting is missing or holds anything other than a known view.
 */
export function useSkillGraphView(surface: AtlasSurface): WritableComputedRef<SkillGraphView> {
  const { key, fallback } = SKILL_GRAPH_VIEW_SETTINGS[surface]
  const setting = useSetting<unknown>(key)
  return computed<SkillGraphView>({
    get: () => (isSkillGraphView(setting.ref.value) ? setting.ref.value : fallback),
    set: (view) => {
      void setting.set(view)
    },
  })
}
