import type { SkillGraphEdge, SkillGraphRelation, SkillInfo, SubjectFieldInfo, VerifiableCredential } from '@/types'
import { BLOOM_ORDER, type BloomLevel } from './bloom'

export type BloomRank = 0 | 1 | 2 | 3 | 4 | 5
export type AtlasStatus = 'earned' | 'ready'

export interface AtlasBloom {
  rank: BloomRank
  name: BloomLevel
}

export interface AtlasRelation {
  skillId: string
  name: string
  loaded: boolean
  earned: boolean
}

export const SKILL_RELATION_TYPES = ['related', 'complementary', 'alternative'] as const
export type SkillRelationType = typeof SKILL_RELATION_TYPES[number]

/** A related, complementary or alternative skill: a connection, never a prerequisite. */
export interface AtlasRelatedSkill {
  skillId: string
  name: string
  type: SkillRelationType
  loaded: boolean
}

export interface AtlasNode {
  skill: Readonly<SkillInfo>
  fieldId: string | null
  status: AtlasStatus
  earnedBloom: AtlasBloom | null
  intrinsicBloom: BloomLevel | null
  prerequisites: readonly AtlasRelation[]
  dependents: readonly AtlasRelation[]
  /** Undirected: a relation appears on both of its skills. */
  relations: readonly AtlasRelatedSkill[]
  /**
   * At least one prerequisite, and every one the skill counts is loaded and
   * earned. Unknown, unloaded or uncounted prerequisites never satisfy it.
   */
  prerequisitesSatisfied: boolean
  /** Not yet earned, with its prerequisites satisfied: the only skills the atlas emphasises as next. */
  readyNext: boolean
}

export interface AtlasField {
  id: string | null
  name: string | null
  /** Stable colour and region; null only for unassigned skills. */
  slot: number | null
  nodeIds: readonly string[]
  total: number
  earned: number
  ready: number
}

export interface AtlasModel {
  nodes: readonly AtlasNode[]
  fields: readonly AtlasField[]
  edges: readonly SkillGraphEdge[]
  counts: { total: number; earned: number; ready: number }
  /** Built once per load; key order matches `nodes`. */
  nodeById: ReadonlyMap<string, AtlasNode>
  /** Unassigned skills sit under `null`. */
  fieldById: ReadonlyMap<string | null, AtlasField>
}

export interface BuildSkillAtlasInput {
  skills: readonly SkillInfo[]
  edges: readonly SkillGraphEdge[]
  /** Omitted or empty when relations could not be loaded; the graph works without them. */
  relations?: readonly SkillGraphRelation[]
  activeCredentials: readonly VerifiableCredential[]
  subjectDid: string | null
  subjectFields?: readonly (Pick<SubjectFieldInfo, 'id' | 'name'> & { atlas_slot?: number | null })[]
}

/** A credential level, route or DOM value is a Bloom rank only if it is one of the six. */
export function isBloomRank(value: unknown): value is BloomRank {
  return typeof value === 'number' && Number.isInteger(value) && value >= 0 && value <= 5
}

function bloomLevel(value: string | null | undefined): BloomLevel | null {
  const normalized = value?.toLowerCase()
  return BLOOM_ORDER.find((level) => level === normalized) ?? null
}

/** The stored default stands in for a type this build does not know. */
function relationType(value: string): SkillRelationType {
  return SKILL_RELATION_TYPES.find((type) => type === value) ?? 'related'
}

/** Orders skill and field IDs by code unit, the same in every locale. */
export function compareIds(left: string, right: string): number {
  return left < right ? -1 : left > right ? 1 : 0
}

export function buildSkillAtlas(input: BuildSkillAtlasInput): AtlasModel {
  const skills = new Map<string, SkillInfo>()
  for (const skill of input.skills) if (!skills.has(skill.id)) skills.set(skill.id, skill)

  const ranks = new Map<string, BloomRank>()
  const earnedIds = new Set<string>()
  if (input.subjectDid) {
    for (const credential of input.activeCredentials) {
      const claim = credential.credentialSubject
      if (claim.id !== input.subjectDid || typeof claim.skillId !== 'string' || claim.skillId.length === 0) continue
      earnedIds.add(claim.skillId)
      const rank = claim.level
      const best = ranks.get(claim.skillId)
      if (isBloomRank(rank) && (best === undefined || rank > best)) ranks.set(claim.skillId, rank)
    }
  }

  const prerequisitesBySkill = new Map<string, Map<string, AtlasRelation>>()
  const dependentsBySkill = new Map<string, Map<string, AtlasRelation>>()
  for (const edge of input.edges) {
    const prerequisiteId = edge.prerequisite_id
    const skillId = edge.skill_id
    if (!prerequisitesBySkill.has(skillId)) prerequisitesBySkill.set(skillId, new Map())
    if (!dependentsBySkill.has(prerequisiteId)) dependentsBySkill.set(prerequisiteId, new Map())
    const prerequisite = skills.get(prerequisiteId)
    const dependent = skills.get(skillId)
    prerequisitesBySkill.get(skillId)?.set(prerequisiteId, {
      skillId: prerequisiteId,
      name: prerequisite?.name ?? edge.prerequisite_name ?? prerequisiteId,
      loaded: prerequisite !== undefined,
      earned: earnedIds.has(prerequisiteId),
    })
    dependentsBySkill.get(prerequisiteId)?.set(skillId, {
      skillId,
      name: dependent?.name ?? edge.skill_name ?? skillId,
      loaded: dependent !== undefined,
      earned: earnedIds.has(skillId),
    })
  }

  const relationsBySkill = new Map<string, Map<string, AtlasRelatedSkill>>()
  const relate = (fromId: string, toId: string, type: SkillRelationType) => {
    if (!skills.has(fromId)) return
    if (!relationsBySkill.has(fromId)) relationsBySkill.set(fromId, new Map())
    const related = relationsBySkill.get(fromId)
    const other = skills.get(toId)
    if (related && !related.has(toId)) related.set(toId, { skillId: toId, name: other?.name ?? toId, type, loaded: other !== undefined })
  }
  for (const relation of input.relations ?? []) {
    if (relation.skill_id === relation.related_skill_id) continue
    const type = relationType(relation.relation_type)
    relate(relation.skill_id, relation.related_skill_id, type)
    relate(relation.related_skill_id, relation.skill_id, type)
  }

  const nodes: AtlasNode[] = [...skills.values()].sort((a, b) => compareIds(a.id, b.id)).map((skill) => {
    const prerequisites = [...(prerequisitesBySkill.get(skill.id)?.values() ?? [])].sort((a, b) => compareIds(a.skillId, b.skillId))
    const dependents = [...(dependentsBySkill.get(skill.id)?.values() ?? [])].sort((a, b) => compareIds(a.skillId, b.skillId))
    const relations = [...(relationsBySkill.get(skill.id)?.values() ?? [])].sort((a, b) => compareIds(a.skillId, b.skillId))
    const rank = ranks.get(skill.id)
    const status: AtlasStatus = earnedIds.has(skill.id) ? 'earned' : 'ready'
    const prerequisitesSatisfied = prerequisites.length > 0 && prerequisites.length === skill.prerequisite_count &&
      prerequisites.every((relation) => relation.loaded && relation.earned)
    return {
      skill,
      fieldId: skill.subject_field_id || null,
      status,
      earnedBloom: rank == null ? null : { rank, name: BLOOM_ORDER[rank] },
      intrinsicBloom: bloomLevel(skill.bloom_level),
      prerequisites,
      dependents,
      relations,
      prerequisitesSatisfied,
      readyNext: status === 'ready' && prerequisitesSatisfied,
    }
  })

  const fieldsMap = new Map<string | null, { id: string | null; name: string | null; nodeIds: string[]; earned: number }>()
  const fieldNames = new Map((input.subjectFields ?? []).map((field) => [field.id, field.name]))
  for (const node of nodes) {
    if (node.fieldId && node.skill.subject_field_name && !fieldNames.has(node.fieldId)) {
      fieldNames.set(node.fieldId, node.skill.subject_field_name)
    }
  }
  for (const node of nodes) {
    const id = node.fieldId
    if (!fieldsMap.has(id)) fieldsMap.set(id, { id, name: id === null ? null : fieldNames.get(id) ?? null, nodeIds: [], earned: 0 })
    const field = fieldsMap.get(id)
    if (!field) continue
    field.nodeIds.push(node.skill.id)
    if (node.status === 'earned') field.earned++
  }
  // A field the taxonomy does not place takes the next slot after every
  // placed one (in ID order), so it can never move a placed field.
  const slots = new Map<string, number>()
  for (const field of input.subjectFields ?? []) if (field.atlas_slot != null) slots.set(field.id, field.atlas_slot)
  let nextSlot = Math.max(-1, ...slots.values()) + 1
  for (const id of [...fieldsMap.keys()].filter((id): id is string => id !== null).sort(compareIds)) {
    if (!slots.has(id)) slots.set(id, nextSlot++)
  }
  const fields = [...fieldsMap.values()].map((field) => ({
    ...field, slot: field.id === null ? null : slots.get(field.id) ?? null,
    total: field.nodeIds.length, ready: field.nodeIds.length - field.earned,
  })).sort((a, b) => a.slot === null ? 1 : b.slot === null ? -1 : a.slot - b.slot)
  const earned = nodes.filter((node) => node.status === 'earned').length
  return {
    nodes, fields, edges: [...input.edges],
    counts: { total: nodes.length, earned, ready: nodes.length - earned },
    nodeById: new Map(nodes.map((node) => [node.skill.id, node])),
    fieldById: new Map(fields.map((field) => [field.id, field])),
  }
}
