import { describe, expect, it } from 'vitest'
import { buildSkillAtlas, isBloomRank } from '../skillAtlas'
import type { SkillInfo, VerifiableCredential } from '@/types'

const skill = (id: string, overrides: Partial<SkillInfo> = {}): SkillInfo => ({
  id, name: id, description: null, subject_id: 'subject', subject_name: 'Subject',
  subject_field_id: 'field', subject_field_name: 'Field', bloom_level: 'create',
  prerequisite_count: 0, dependent_count: 0, created_at: null, ...overrides,
})
const credential = (skillId: string, level: unknown, subject = 'did:me'): VerifiableCredential => ({
  '@context': [], type: [], issuer: 'did:issuer', validFrom: '2026-01-01T00:00:00Z',
  credentialSubject: { id: subject, skillId, level },
  proof: { type: 'DataIntegrityProof', cryptosuite: 'eddsa-jcs-2022', created: '', verificationMethod: '', proofPurpose: '', proofValue: '' },
})

describe('buildSkillAtlas', () => {
  it('shows the highest subject-bound earned Bloom separately from intrinsic Bloom and keeps unearned skills Ready', () => {
    const atlas = buildSkillAtlas({ skills: [skill('a'), skill('b')], edges: [], subjectDid: 'did:me',
      activeCredentials: [credential('a', 0), credential('a', 3), credential('a', 5, 'did:other')] })
    expect(atlas.nodes.map(node => [node.skill.id, node.status, node.earnedBloom, node.intrinsicBloom])).toEqual([
      ['a', 'earned', { rank: 3, name: 'analyze' }, 'create'], ['b', 'ready', null, 'create'],
    ])
  })
  it('keeps malformed levels earned without inventing a rank, and never exposes achievements without identity', () => {
    const invalid = [undefined, null, '3', NaN, Infinity, -1, 6, 1.5]
    const skills = invalid.map((_, i) => skill(String(i)))
    const activeCredentials = invalid.map((rank, i) => credential(String(i), rank))
    const input = { skills, edges: [], activeCredentials }
    expect(buildSkillAtlas({ ...input, subjectDid: 'did:me' }).nodes.map(n => [n.status, n.earnedBloom]))
      .toEqual(invalid.map(() => ['earned', null]))
    for (const subjectDid of [null, '']) {
      expect(buildSkillAtlas({ ...input, subjectDid }).nodes.every(n => n.status === 'ready')).toBe(true)
    }
    expect(buildSkillAtlas({ ...input, subjectDid: 'did:me', activeCredentials: [credential('0', NaN), credential('0', 2), credential('0', 9)] }).nodes[0]?.earnedBloom)
      .toEqual({ rank: 2, name: 'apply' })
  })

  it('honors the supplied backend listing without inventing a date or verification policy', () => {
    const atlas = buildSkillAtlas({ skills: [skill('a'), skill('b'), skill('c')], edges: [], subjectDid: 'did:me',
      activeCredentials: [
        { ...credential('a', 5), validUntil: '2026-10-27T00:00:00Z' },
        credential('a', 1),
        { ...credential('b', 3), validFrom: '2026-10-28T00:00:00Z' },
        { ...credential('c', 3), validUntil: 'invalid' },
      ] })
    expect(atlas.nodes.map(n => [n.status, n.earnedBloom])).toEqual([
      ['earned', { rank: 5, name: 'create' }], ['earned', { rank: 3, name: 'analyze' }], ['earned', { rank: 3, name: 'analyze' }],
    ])
  })

  it('preserves metadata and cross-field/unloaded prerequisites, counting distinct loaded IDs in stable regions', () => {
    const a = skill('a', { prerequisite_count: 2, subject_field_name: null })
    const b = skill('b', { subject_field_id: 'other', subject_field_name: 'Other' })
    const c = skill('c', { subject_field_id: null, subject_field_name: null, prerequisite_count: 1 })
    const edges = [
      { skill_id: 'a', skill_name: 'A', skill_bloom: 'create', prerequisite_id: 'b', prerequisite_name: 'B', prerequisite_bloom: 'apply' },
      { skill_id: 'a', skill_name: 'A', skill_bloom: 'create', prerequisite_id: 'outside', prerequisite_name: 'Outside', prerequisite_bloom: 'apply' },
    ]
    const atlas = buildSkillAtlas({ skills: [c, b, a, a], edges: [...edges, edges[0]!], subjectDid: 'did:me',
      subjectFields: [{ id: 'field', name: 'Resolved field' }],
      activeCredentials: [credential('b', 0), credential('b', 0), credential('outside', 2)] })
    expect(atlas.counts).toEqual({ total: 3, earned: 1, ready: 2 })
    expect(atlas.fields.map(f => [f.id, f.name, f.nodeIds, f.earned, f.ready])).toEqual([
      ['field', 'Resolved field', ['a'], 0, 1], ['other', 'Other', ['b'], 1, 0], [null, null, ['c'], 0, 1],
    ])
    expect(atlas.nodes.map(n => n.skill.id)).toEqual(['a', 'b', 'c'])
    expect(atlas.nodes[0]?.skill).toEqual(a)
    expect(atlas.edges).toEqual([...edges, edges[0]])
    expect(atlas.nodes[0]?.prerequisites).toEqual([
      { skillId: 'b', name: 'b', loaded: true, earned: true },
      { skillId: 'outside', name: 'Outside', loaded: false, earned: true },
    ])
    expect(atlas.nodes[1]?.dependents).toEqual([{ skillId: 'a', name: 'a', loaded: true, earned: false }])
    // Satisfied needs at least one prerequisite, every one loaded and earned:
    // a's 'outside' is earned but not loaded, b has none, c's one is not loaded.
    expect(atlas.nodes.map(n => [n.status, n.prerequisitesSatisfied, n.readyNext])).toEqual([
      ['ready', false, false], ['earned', false, false], ['ready', false, false],
    ])
  })

  it('marks a skill ready next only when every one of its counted prerequisites is loaded and earned', () => {
    const edge = (skillId: string, prerequisiteId: string) => ({ skill_id: skillId, skill_name: skillId, skill_bloom: 'create',
      prerequisite_id: prerequisiteId, prerequisite_name: prerequisiteId, prerequisite_bloom: 'apply' })
    const atlas = buildSkillAtlas({ subjectDid: 'did:me',
      skills: [skill('p1'), skill('p2'), skill('p3'), skill('none'), skill('next', { prerequisite_count: 2 }),
        skill('partial', { prerequisite_count: 2 }), skill('unloaded', { prerequisite_count: 2 }),
        skill('uncounted', { prerequisite_count: 3 }), skill('overcounted', { prerequisite_count: 1 }), skill('done', { prerequisite_count: 1 })],
      edges: [edge('next', 'p1'), edge('next', 'p2'), edge('partial', 'p1'), edge('partial', 'p3'), edge('unloaded', 'p1'),
        edge('unloaded', 'gone'), edge('uncounted', 'p1'), edge('uncounted', 'p2'), edge('overcounted', 'p1'), edge('overcounted', 'p2'), edge('done', 'p1')],
      activeCredentials: [credential('p1', 2), credential('p2', 2), credential('gone', 2), credential('done', 1)] })
    const state = Object.fromEntries(atlas.nodes.map(n => [n.skill.id, [n.prerequisitesSatisfied, n.readyNext]]))
    expect(state).toEqual({
      p1: [false, false], p2: [false, false], p3: [false, false], none: [false, false],
      next: [true, true],
      partial: [false, false], unloaded: [false, false], uncounted: [false, false], overcounted: [false, false],
      // Already earned: its prerequisites are satisfied, but it is not next.
      done: [true, false],
    })
  })

  it('resolves incomplete field labels from sibling metadata and keeps ordering independent of achievements', () => {
    const skills = [skill('z'), skill('a', { subject_field_name: null }), skill('b', { subject_field_id: '', subject_field_name: null, bloom_level: 'unknown' })]
    const input = { skills, edges: [], subjectDid: 'did:me', activeCredentials: [] }
    const initial = buildSkillAtlas(input)
    const refreshed = buildSkillAtlas({ ...input, skills: [...skills].reverse(), activeCredentials: [credential('z', 5)] })
    expect(initial.fields.map(f => [f.id, f.name, f.nodeIds])).toEqual([
      ['field', 'Field', ['a', 'z']], [null, null, ['b']],
    ])
    expect(refreshed.fields.map(f => [f.id, f.name, f.nodeIds])).toEqual(initial.fields.map(f => [f.id, f.name, f.nodeIds]))
    expect(initial.nodes[1]?.intrinsicBloom).toBeNull()
  })

  it('places fields by their atlas slot and gives an unplaced field the slot after every placed one', () => {
    const skills = [skill('a', { subject_field_id: 'a_field' }), skill('b', { subject_field_id: 'b_field' }), skill('c', { subject_field_id: null })]
    const atlas = buildSkillAtlas({ skills, edges: [], subjectDid: 'did:me', activeCredentials: [],
      subjectFields: [{ id: 'b_field', name: 'B', atlas_slot: 4 }, { id: 'unloaded', name: 'U', atlas_slot: 6 }] })
    expect(atlas.fields.map(f => [f.id, f.slot])).toEqual([['b_field', 4], ['a_field', 7], [null, null]])
  })

  it('looks up every node and field by ID, including unassigned skills', () => {
    const atlas = buildSkillAtlas({ skills: [skill('a'), skill('b', { subject_field_id: 'other', subject_field_name: 'Other' }), skill('c', { subject_field_id: null })],
      edges: [], subjectDid: 'did:me', activeCredentials: [credential('b', 2)] })
    expect(atlas.nodeById.get('b')).toMatchObject({ skill: { id: 'b' }, status: 'earned', fieldId: 'other' })
    expect(atlas.nodeById.get('missing')).toBeUndefined()
    expect(atlas.fieldById.get('other')).toMatchObject({ name: 'Other', nodeIds: ['b'], earned: 1 })
    expect(atlas.fieldById.get(null)).toMatchObject({ id: null, nodeIds: ['c'] })
    expect([...atlas.nodeById.keys()]).toEqual(['a', 'b', 'c'])
  })
  it('accepts only the six Bloom ranks from outside values', () => {
    expect([0, 1, 2, 3, 4, 5].every(isBloomRank)).toBe(true)
    expect([-1, 6, 1.5, NaN, '3', null, undefined].some(isBloomRank)).toBe(false)
  })
})
