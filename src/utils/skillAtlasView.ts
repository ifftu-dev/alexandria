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
  /** The Web view's camera, apart from the Detailed `camera`; null fits what is shown. */
  webCamera: AtlasCamera | null
  /** Web toggles: every name, arrowheads on every prerequisite line, and the dotted relation lines. */
  webNames: boolean
  webArrows: boolean
  webRelated: boolean
  /** The Web view shows only the skills around the selection, `neighborhoodDepth` steps out. */
  webNarrowed: boolean
}

/** Fresh defaults shared by profile state and the props-only explorer. */
export function createAtlasViewState(): AtlasViewState {
  return {
    selectedSkillId: null, focusedFieldId: null, unassignedFocused: false, query: '', bloomEmphasis: null,
    neighborhood: 'none', neighborhoodDepth: 1, camera: null, inspectorCollapsed: false,
    webCamera: null, webNames: false, webArrows: false, webRelated: true, webNarrowed: false,
  }
}
