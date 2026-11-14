// The remembered skill graph view: one profile preference shared by every
// surface that shows the graph. Backed by the `ui.skill_graph_view` synced
// setting, which the backend holds to `simple` (default) or `detailed`.

import { computed, type WritableComputedRef } from 'vue'

import { useSetting } from './useSettings'

export type SkillGraphView = 'simple' | 'detailed'

export function isSkillGraphView(value: unknown): value is SkillGraphView {
  return value === 'simple' || value === 'detailed'
}

/**
 * Two-way ref onto the remembered view. Reads `simple` whenever the setting
 * is missing or holds anything other than a known view.
 */
export function useSkillGraphView(): WritableComputedRef<SkillGraphView> {
  const setting = useSetting<unknown>('ui.skill_graph_view')
  return computed<SkillGraphView>({
    get: () => (isSkillGraphView(setting.ref.value) ? setting.ref.value : 'simple'),
    set: (view) => {
      void setting.set(view)
    },
  })
}
