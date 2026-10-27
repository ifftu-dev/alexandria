import type { BloomRank } from '@/utils/skillAtlas'

export interface AtlasCamera { x: number; y: number; zoom: number }
export interface AtlasViewState {
  selectedSkillId: string | null
  focusedFieldId: string | null
  unassignedFocused: boolean
  query: string
  bloomEmphasis: BloomRank | null
  neighborhood: 'none' | 'prerequisites' | 'dependents'
  neighborhoodDepth: 1 | 2
  camera: AtlasCamera | null
  inspectorCollapsed: boolean
}

/** Fresh defaults shared by profile state and the props-only explorer. */
export function createAtlasViewState(): AtlasViewState {
  return {
    selectedSkillId: null, focusedFieldId: null, unassignedFocused: false, query: '', bloomEmphasis: null,
    neighborhood: 'none', neighborhoodDepth: 1, camera: null, inspectorCollapsed: false,
  }
}
