import type { MessageKey } from '@/i18n/keys'
import { BLOOM_ORDER, type BloomLevel } from '@/utils/bloom'
import type { AtlasField, AtlasNode } from '@/utils/skillAtlas'

/**
 * Distinct field colours, `--atlas-field-0` to `--atlas-field-7` in
 * `main.css`, light and dark. A field's colour is its atlas slot modulo this
 * size: slots past the palette wrap around and share a colour. The palette
 * holds at least as many colours as there are bundled fields (a test pins
 * this), so bundling a field beyond it means adding a colour first.
 */
export const ATLAS_PALETTE_SIZE = 8

export function atlasFieldColor(field: Pick<AtlasField, 'id' | 'slot'>): string {
  if (field.id === null || field.slot === null) return 'var(--app-muted-foreground)'
  return `var(--atlas-field-${field.slot % ATLAS_PALETTE_SIZE})`
}

/** The `t` of `useI18n()`, narrowed to what atlas labels need. */
export type AtlasTranslate = (key: MessageKey, named?: Record<string, unknown>) => string

/**
 * How the atlas surfaces (explorer, sidebar graph, skills page) show a field,
 * a Bloom level and a skill, so they cannot disagree.
 */
export function createAtlasPresenter(t: AtlasTranslate) {
  /** "2 Understand": the 1-based Bloom number, then its name. */
  function bloomLabel(level: BloomLevel): string {
    return `${BLOOM_ORDER.indexOf(level) + 1} ${t(`common.bloom.${level}`)}`
  }

  function fieldName(field: Pick<AtlasField, 'id' | 'name'>): string {
    if (field.id === null) return t('skills.atlas.unassigned')
    return field.name ?? t('skills.atlas.unknownField', { id: field.id })
  }

  /** Earned, Ready next (every prerequisite earned) or Ready. */
  function state(node: Pick<AtlasNode, 'status' | 'readyNext'>): string {
    if (node.status === 'earned') return t('skills.atlas.earned')
    return t(node.readyNext ? 'skills.atlas.readyNext' : 'skills.atlas.ready')
  }

  /** The visible evidence line: earned Bloom, its absence, Ready next or Ready. */
  function evidence(node: AtlasNode): string {
    if (node.status === 'ready') return state(node)
    return node.earnedBloom ? bloomLabel(node.earnedBloom.name) : t('skills.atlas.levelUnavailable')
  }

  /**
   * Name, earned or not-yet-earned (and ready next when every prerequisite is
   * earned), and Bloom level. Earned skills give their current earned level;
   * Ready skills give their learning-outcome level.
   */
  function accessibleName(node: AtlasNode): string {
    const name = node.skill.name
    if (node.status === 'earned') {
      return node.earnedBloom
        ? t('skills.atlas.nameEarned', { name, level: bloomLabel(node.earnedBloom.name) })
        : t('skills.atlas.nameEarnedNoLevel', { name })
    }
    if (node.readyNext) {
      return node.intrinsicBloom
        ? t('skills.atlas.nameReadyNext', { name, level: bloomLabel(node.intrinsicBloom) })
        : t('skills.atlas.nameReadyNextNoLevel', { name })
    }
    return node.intrinsicBloom
      ? t('skills.atlas.nameReady', { name, level: bloomLabel(node.intrinsicBloom) })
      : t('skills.atlas.nameReadyNoLevel', { name })
  }

  return { bloomLabel, fieldName, state, evidence, accessibleName }
}
