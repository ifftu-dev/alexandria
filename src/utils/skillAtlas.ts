import type { SkillGraphEdge, SkillInfo, SubjectFieldInfo, VerifiableCredential } from '@/types'
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

export interface AtlasNode {
  skill: Readonly<SkillInfo>
  fieldId: string | null
  status: AtlasStatus
  earnedBloom: AtlasBloom | null
  intrinsicBloom: BloomLevel | null
  prerequisites: readonly AtlasRelation[]
  dependents: readonly AtlasRelation[]
  prerequisitesSatisfied: boolean
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
}

export interface BuildSkillAtlasInput {
  skills: readonly SkillInfo[]
  edges: readonly SkillGraphEdge[]
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

function compareIds(left: string, right: string): number {
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

  const nodes: AtlasNode[] = [...skills.values()].sort((a, b) => compareIds(a.id, b.id)).map((skill) => {
    const prerequisites = [...(prerequisitesBySkill.get(skill.id)?.values() ?? [])].sort((a, b) => compareIds(a.skillId, b.skillId))
    const dependents = [...(dependentsBySkill.get(skill.id)?.values() ?? [])].sort((a, b) => compareIds(a.skillId, b.skillId))
    const rank = ranks.get(skill.id)
    return {
      skill,
      fieldId: skill.subject_field_id || null,
      status: earnedIds.has(skill.id) ? 'earned' : 'ready',
      earnedBloom: rank == null ? null : { rank, name: BLOOM_ORDER[rank] },
      intrinsicBloom: bloomLevel(skill.bloom_level),
      prerequisites,
      dependents,
      // Satisfied means at least one prerequisite, every one known and earned.
      prerequisitesSatisfied: prerequisites.length > 0 && prerequisites.length >= skill.prerequisite_count &&
        prerequisites.every((relation) => relation.earned),
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
  }
}
