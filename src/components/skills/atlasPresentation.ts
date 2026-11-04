import type { MessageKey } from '@/i18n/keys'
import { BLOOM_ORDER, type BloomLevel } from '@/utils/bloom'
import type { AtlasField, AtlasNode } from '@/utils/skillAtlas'

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

  /** The visible evidence line: earned Bloom, its absence, or Ready. */
  function evidence(node: AtlasNode): string {
    if (node.status === 'ready') return t('skills.atlas.ready')
    return node.earnedBloom ? bloomLabel(node.earnedBloom.name) : t('skills.atlas.levelUnavailable')
  }

  /**
   * Name, earned or not-yet-earned, and Bloom level. Earned skills give their
   * current earned level; Ready skills give their learning-outcome level.
   */
  function accessibleName(node: AtlasNode): string {
    const name = node.skill.name
    if (node.status === 'earned') {
      return node.earnedBloom
        ? t('skills.atlas.nameEarned', { name, level: bloomLabel(node.earnedBloom.name) })
        : t('skills.atlas.nameEarnedNoLevel', { name })
    }
    return node.intrinsicBloom
      ? t('skills.atlas.nameReady', { name, level: bloomLabel(node.intrinsicBloom) })
      : t('skills.atlas.nameReadyNoLevel', { name })
  }

  return { bloomLabel, fieldName, evidence, accessibleName }
}
