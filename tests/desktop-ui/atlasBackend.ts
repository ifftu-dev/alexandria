import taxonomy from '../../bootstrap/public_taxonomy.json'
import type { Identity, SkillInfo, SkillGraphEdge, VerifiableCredential } from '@/types'
import type { SettingEntry } from '@/composables/useSettings'

/** Strict external IPC fixture shared by mounted surfaces and browser journeys. */
export function atlasBackend(stress = false) {
  const subjects = taxonomy.subjects.map(subject => ({ ...subject,
    skill_count: taxonomy.skills.filter(skill => skill.subject_id === subject.id).length }))
  const fields = taxonomy.subject_fields.map(field => ({ ...field,
    subject_count: subjects.filter(subject => subject.subject_field_id === field.id).length,
    skill_count: subjects.filter(subject => subject.subject_field_id === field.id).reduce((sum, subject) => sum + subject.skill_count, 0) }))
  const skills: SkillInfo[] = taxonomy.skills.map(skill => {
    const subject = subjects.find(item => item.id === skill.subject_id)
    const field = fields.find(item => item.id === subject?.subject_field_id)
    return { ...skill, subject_name: subject?.name ?? null, subject_field_id: field?.id ?? null,
      subject_field_name: field?.name ?? null,
      prerequisite_count: taxonomy.skill_prerequisites.filter(edge => edge.skill_id === skill.id).length,
      dependent_count: taxonomy.skill_prerequisites.filter(edge => edge.prerequisite_id === skill.id).length }
  })
  const edges: SkillGraphEdge[] = taxonomy.skill_prerequisites.map(edge => ({ ...edge,
    skill_name: skills.find(s => s.id === edge.skill_id)!.name, skill_bloom: skills.find(s => s.id === edge.skill_id)!.bloom_level,
    prerequisite_name: skills.find(s => s.id === edge.prerequisite_id)!.name, prerequisite_bloom: skills.find(s => s.id === edge.prerequisite_id)!.bloom_level }))
  if (stress) {
    skills.push({ id: 'fixture-unassigned', name: 'A very long skill label about community stewardship, collaborative problem solving, and inclusive systems design — نام کی مکمل وضاحت',
      description: 'Long bilingual label and unloaded prerequisite', subject_id: null, subject_name: null,
      subject_field_id: null, subject_field_name: null, bloom_level: 'evaluate', prerequisite_count: 1, dependent_count: 0, created_at: null })
    edges.push({ skill_id: 'fixture-unassigned', skill_name: 'Long label', skill_bloom: 'evaluate',
      prerequisite_id: 'missing', prerequisite_name: 'An unloaded prerequisite', prerequisite_bloom: 'apply' })
  }
  const identity: Identity = { stake_address: 'stake-fixture', payment_address: 'payment-fixture', username: null,
    display_name: 'Atlas learner', bio: null, avatar_cid: null, visibility: null, profile_hash: null, created_at: '', updated_at: '' }
  const settings: SettingEntry[] = ['learner.targets', 'instructor.graph_prefs'].map(key => ({ key, scope: 'sync', category: '', label: key,
    description: '', kind: 'json', default_value: key === 'learner.targets' ? '[]' : '{}', current_value: key === 'learner.targets' ? '[]' : '{}', is_default: true }))
  const state = { profile: 'profile-a' as string | null, level: 1, earned: true, reads: 0, unexpected: [] as string[] }
  function credential(skillId: string, level: unknown): VerifiableCredential {
    return { '@context': [], id: `vc-${skillId}`, type: ['VerifiableCredential'], issuer: 'did:fixture:issuer', validFrom: '2026-10-27T00:00:00Z',
      credentialSubject: { id: `did:fixture:${state.profile}`, skillId, level, score: 1 },
      proof: { type: '', cryptosuite: '', created: '', verificationMethod: '', proofPurpose: '', proofValue: '' } }
  }
  async function invoke(command: string, args?: Record<string, unknown>): Promise<unknown> {
    switch (command) {
      case 'get_incompatible_profile_data': return null
      case 'get_profile_cleanup_required': return false
      case 'list_profiles': return []
      case 'get_active_profile_id': return state.profile
      case 'get_profile_session_token': return state.profile
      case 'get_profile': return identity
      case 'get_wallet_info': return { stake_address: identity.stake_address, payment_address: identity.payment_address, has_mnemonic_backup: true }
      case 'unlock_profile': state.profile = String(args?.id); return { profile: identity, wallet: await invoke('get_wallet_info') }
      case 'lock_profile': state.profile = null; return null
      case 'get_local_did': return state.profile ? `did:fixture:${state.profile}` : null
      case 'list_subject_fields': return fields
      case 'list_subjects': return subjects
      case 'list_skills': return skills
      case 'list_skill_graph_edges': return edges
      case 'list_credentials': {
        state.reads++
        if (args?.activeOnly && args.subject !== `did:fixture:${state.profile}`) throw new Error('Atlas credentials must be subject-bound')
        const creds = state.earned ? [credential('skill_big_o', state.level), ...(stress ? [credential('fixture-unassigned', 'invalid')] : [])] : []
        return args?.skillId ? creds.filter(vc => vc.credentialSubject.skillId === args.skillId) : creds
      }
      case 'get_skill': return {
        skill: skills.find(s => s.id === args?.skillId),
        prerequisites: edges.filter(edge => edge.skill_id === args?.skillId).flatMap(edge => skills.filter(skill => skill.id === edge.prerequisite_id)),
        dependents: edges.filter(edge => edge.prerequisite_id === args?.skillId).flatMap(edge => skills.filter(skill => skill.id === edge.skill_id)),
        related: taxonomy.skill_relations.filter(relation => relation.skill_id === args?.skillId).map(relation => ({
          skill_id: relation.related_skill_id, skill_name: skills.find(skill => skill.id === relation.related_skill_id)!.name,
          bloom_level: skills.find(skill => skill.id === relation.related_skill_id)!.bloom_level, relation_type: relation.relation_type,
        })),
      }
      case 'get_my_skill_graph': return { subject_did: `did:fixture:${state.profile}`, nodes: state.earned ? [{
        id: 'skill_big_o', name: 'Big-O Analysis', bloom_level: 'analyze', subject_name: 'Computer Science', public: true, teaching: false,
      }] : [], edges: [] }
      case 'list_settings': return structuredClone(settings)
      case 'set_setting': {
        const entry = settings.find(item => item.key === args?.key)
        if (!entry) throw new Error(`Unknown fixture setting ${args?.key}`)
        entry.current_value = String(args?.value); return null
      }
      case 'plugin:event|listen': return 1
      case 'plugin:event|unlisten': return null
      case 'list_courses': case 'list_opinions': case 'list_derived_states': case 'list_goal_templates': return []
      case 'recompute_all': return 0
      case 'compute_learning_path': return { steps: [], total: 0, earned_count: 0, goal_skill_ids: args?.goalSkillIds ?? [] }
      case 'assessment_plan_goal': return { steps: [] }
      default: state.unexpected.push(command); throw new Error(`Unexpected atlas fixture IPC: ${command}`)
    }
  }
  return { state, invoke, skills, settings }
}
