import taxonomy from '../../bootstrap/public_taxonomy.json'
import type { Chapter, Course, Element, Identity, SkillInfo, SkillGraphEdge, VerifiableCredential } from '@/types'
import type { SettingEntry } from '@/composables/useSettings'
import type { SkillGraphView } from '@/composables/useSkillGraphView'

/**
 * `stress` adds one long-label unassigned skill. `scale` grows the bootstrap
 * taxonomy to SCALE_SKILLS skills spread over every subject of every field,
 * with prerequisites mostly inside a field and some across fields.
 */
export type AtlasScenario = 'stress' | 'scale'
export const SCALE_SKILLS = 1500

/** Deterministic, so every run pans the same graph. */
export function scaleTaxonomy() {
  let seed = 7
  const random = () => (seed = (seed * 1103515245 + 12345) % 2147483648) / 2147483648
  const blooms = ['remember', 'understand', 'apply', 'analyze', 'evaluate', 'create']
  const skills = [...taxonomy.skills]
  const edges = [...taxonomy.skill_prerequisites]
  const byField = new Map<string, string[]>()
  const fieldOf = new Map(taxonomy.subjects.map(subject => [subject.id, subject.subject_field_id]))
  for (const skill of skills) byField.set(fieldOf.get(skill.subject_id)!, [...(byField.get(fieldOf.get(skill.subject_id)!) ?? []), skill.id])
  for (let index = 0; skills.length < SCALE_SKILLS; index++) {
    const subject = taxonomy.subjects[index % taxonomy.subjects.length]!
    const id = `scale_${String(index).padStart(4, '0')}`
    skills.push({ id, name: `${subject.name} practice ${index + 1}`, description: `Generated scale skill ${index + 1}`,
      subject_id: subject.id, bloom_level: blooms[index % blooms.length]!, created_at: '', updated_at: '' })
    const local = byField.get(subject.subject_field_id) ?? []
    const prerequisites = new Set<string>()
    for (let pick = 0; pick < 2 + (index % 2) && local.length; pick++) prerequisites.add(local[Math.floor(random() * local.length)]!)
    if (index % 4 === 0) prerequisites.add(skills[Math.floor(random() * (skills.length - 1))]!.id)
    prerequisites.delete(id)
    for (const prerequisite_id of prerequisites) edges.push({ skill_id: id, prerequisite_id })
    byField.set(subject.subject_field_id, [...local, id])
  }
  return { skills, edges }
}

export interface AtlasBackendOptions {
  /** Remembered skill graph view per profile; a profile left out has never chosen. */
  views?: Partial<Record<string, SkillGraphView>>
}

/**
 * Strict external IPC fixture shared by mounted surfaces and browser journeys.
 * Each profile keeps its own settings; `settings` is the first profile's.
 */
export function atlasBackend(scenario: AtlasScenario | null = null, options: AtlasBackendOptions = {}) {
  const stress = scenario === 'stress'
  const graph = scenario === 'scale' ? scaleTaxonomy() : { skills: taxonomy.skills, edges: taxonomy.skill_prerequisites }
  const skillsPerSubject = new Map<string, number>()
  for (const skill of graph.skills) skillsPerSubject.set(skill.subject_id, (skillsPerSubject.get(skill.subject_id) ?? 0) + 1)
  const prerequisiteCount = new Map<string, number>(), dependentCount = new Map<string, number>()
  for (const edge of graph.edges) {
    prerequisiteCount.set(edge.skill_id, (prerequisiteCount.get(edge.skill_id) ?? 0) + 1)
    dependentCount.set(edge.prerequisite_id, (dependentCount.get(edge.prerequisite_id) ?? 0) + 1)
  }
  const subjects = taxonomy.subjects.map(subject => ({ ...subject, skill_count: skillsPerSubject.get(subject.id) ?? 0 }))
  const fields = taxonomy.subject_fields.map(field => ({ ...field,
    subject_count: subjects.filter(subject => subject.subject_field_id === field.id).length,
    skill_count: subjects.filter(subject => subject.subject_field_id === field.id).reduce((sum, subject) => sum + subject.skill_count, 0) }))
  const skills: SkillInfo[] = graph.skills.map(skill => {
    const subject = subjects.find(item => item.id === skill.subject_id)
    const field = fields.find(item => item.id === subject?.subject_field_id)
    return { ...skill, subject_name: subject?.name ?? null, subject_field_id: field?.id ?? null,
      subject_field_name: field?.name ?? null,
      prerequisite_count: prerequisiteCount.get(skill.id) ?? 0, dependent_count: dependentCount.get(skill.id) ?? 0 }
  })
  const skillById = new Map(skills.map(skill => [skill.id, skill]))
  const edges: SkillGraphEdge[] = graph.edges.map(edge => ({ ...edge,
    skill_name: skillById.get(edge.skill_id)!.name, skill_bloom: skillById.get(edge.skill_id)!.bloom_level,
    prerequisite_name: skillById.get(edge.prerequisite_id)!.name, prerequisite_bloom: skillById.get(edge.prerequisite_id)!.bloom_level }))
  if (stress) {
    skills.push({ id: 'fixture-unassigned', name: 'A very long skill label about community stewardship, collaborative problem solving, and inclusive systems design — نام کی مکمل وضاحت',
      description: 'Long bilingual label and unloaded prerequisite', subject_id: null, subject_name: null,
      subject_field_id: null, subject_field_name: null, bloom_level: 'evaluate', prerequisite_count: 1, dependent_count: 0, created_at: null })
    edges.push({ skill_id: 'fixture-unassigned', skill_name: 'Long label', skill_bloom: 'evaluate',
      prerequisite_id: 'missing', prerequisite_name: 'An unloaded prerequisite', prerequisite_bloom: 'apply' })
  }
  // Illustrative learning content, distinct from the bootstrap taxonomy.
  const courses: Course[] = [
    { id: 'fixture-big-o', title: 'Further algorithm analysis', skill_ids: ['skill_big_o'] },
    { id: 'fixture-ready', title: 'Practising sorting algorithms', skill_ids: ['skill_sorting'] },
    { id: 'fixture-stewardship', title: 'Practising community stewardship', skill_ids: ['fixture-unassigned'] },
  ].map(course => ({
    ...course, kind: 'course',
    description: 'Fixture learning content', author_address: 'stake-instructor', author_name: 'Fixture instructor',
    content_cid: 'fixture-content', thumbnail_cid: null, thumbnail_svg: null, tags: null,
    version: 1, status: 'published', published_at: '2026-10-27', on_chain_tx: null,
    created_at: '2026-10-27', updated_at: '2026-10-27',
  }))
  const chapters: Chapter[] = [{ id: 'fixture-analysis-chapter', course_id: 'fixture-big-o', title: 'Comparing algorithm growth', description: null, position: 0 }]
  const elements: Element[] = [{ id: 'fixture-analysis-reading', chapter_id: 'fixture-analysis-chapter', title: 'Linear and quadratic growth',
    element_type: 'text', content_inline: 'Compare how doubling the input changes the work for a linear scan and a pairwise comparison.',
    content_cid: null, position: 0, duration_seconds: null }]
  const identity: Identity = { stake_address: 'stake-fixture', payment_address: 'payment-fixture', username: null,
    display_name: 'Atlas learner', bio: null, avatar_cid: null, visibility: null, profile_hash: null, created_at: '', updated_at: '' }
  const settingDefaults: Array<Pick<SettingEntry, 'key' | 'kind' | 'default_value'>> = [
    { key: 'learner.targets', kind: 'json', default_value: '[]' },
    { key: 'instructor.graph_prefs', kind: 'json', default_value: '{}' },
    { key: 'ui.skill_graph_view', kind: 'string', default_value: 'simple' },
  ]
  const settingsByProfile = new Map<string, SettingEntry[]>()
  function settingsOf(profile: string): SettingEntry[] {
    let entries = settingsByProfile.get(profile)
    if (!entries) {
      const view = options.views?.[profile]
      entries = settingDefaults.map(({ key, kind, default_value }) => {
        const current_value = key === 'ui.skill_graph_view' && view ? view : default_value
        return { key, scope: 'sync', category: '', label: key, description: '', kind, default_value, current_value, is_default: current_value === default_value }
      })
      settingsByProfile.set(profile, entries)
    }
    return entries
  }
  const settings = settingsOf('profile-a')
  function activeSettings(): SettingEntry[] {
    if (!state.profile) throw new Error('Settings need an unlocked profile')
    return settingsOf(state.profile)
  }
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
      // The credentials page reads anchor state; fixture credentials are never anchored.
      case 'list_credential_anchors': return []
      case 'list_credentials': {
        state.reads++
        if (args?.activeOnly && args.subject !== `did:fixture:${state.profile}`) throw new Error('Atlas credentials must be subject-bound')
        const creds = state.earned ? [credential('skill_big_o', state.level), ...(stress ? [credential('fixture-unassigned', 'invalid')] : []),
          ...(scenario === 'scale' ? skills.filter((_, index) => index % 5 === 0 && index > 0).map((skill, index) => credential(skill.id, index % 6)) : [])] : []
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
      case 'list_settings': return structuredClone(activeSettings())
      case 'set_setting': {
        const entry = activeSettings().find(item => item.key === args?.key)
        if (!entry) throw new Error(`Unknown fixture setting ${args?.key}`)
        entry.current_value = String(args?.value); entry.is_default = entry.current_value === entry.default_value; return null
      }
      case 'plugin:event|listen': return 1
      case 'plugin:event|unlisten': return null
      case 'hydrate_catalog_courses': return 0
      case 'list_courses': return courses
      case 'get_course': return courses.find(course => course.id === args?.courseId) ?? null
      case 'list_chapters': return chapters.filter(chapter => chapter.course_id === args?.courseId)
      case 'list_elements': return elements.filter(element => element.chapter_id === args?.chapterId)
      case 'list_enrollments': return []
      case 'list_opinions': case 'list_derived_states': case 'list_goal_templates': return []
      case 'recompute_all': return 0
      case 'compute_learning_path': return { steps: [], total: 0, earned_count: 0, goal_skill_ids: args?.goalSkillIds ?? [] }
      case 'assessment_plan_goal': return { steps: [] }
      default: state.unexpected.push(command); throw new Error(`Unexpected atlas fixture IPC: ${command}`)
    }
  }
  return { state, invoke, skills, settings, settingsOf }
}
