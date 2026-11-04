import type { Chapter, Course, Element, Enrollment, Goal, GoalTemplate, Identity, SkillInfo } from '@/types'
import type { SettingEntry } from '@/composables/useSettings'

/**
 * An unlocked learner (who also teaches, so the mode switcher shows) with two
 * goals, a small catalog and one enrolled course of three readings: enough for
 * the motion surfaces to render real data. Unknown commands are recorded, then
 * answered with an empty list (`list_*`, `*_list`) or null.
 */
export function motionBackend() {
  const course = (id: string, title: string, kind: 'course' | 'tutorial'): Course => ({
    id, title, kind, description: 'Fixture learning content', author_address: 'stake-instructor', author_name: 'Fixture instructor',
    content_cid: null, thumbnail_cid: null, thumbnail_svg: null, tags: null, skill_ids: ['skill-sorting'],
    version: 1, status: 'published', published_at: '2026-11-01', on_chain_tx: null, created_at: '2026-11-01', updated_at: '2026-11-01',
  })
  const courses = [
    course('course-sorting', 'Sorting algorithms', 'course'),
    course('course-graphs', 'Graph search', 'course'),
    course('tutorial-recursion', 'Recursion in ten minutes', 'tutorial'),
  ]
  const enrollments: Enrollment[] = [{ id: 'enrollment-sorting', course_id: 'course-sorting', course_document_cid: null, course_document_version: null,
    completion_policy_json: null, enrolled_at: '2026-11-01', completed_at: null, status: 'active', updated_at: '2026-11-01' }]
  const chapters: Chapter[] = [{ id: 'chapter-sorting', course_id: 'course-sorting', title: 'Comparison sorts', description: null, position: 0 }]
  const elements: Element[] = ['Bubble sort', 'Insertion sort', 'Merge sort'].map((title, position) => ({
    id: `reading-${position + 1}`, chapter_id: 'chapter-sorting', title, element_type: 'text',
    content_inline: `${title} compares neighbouring items and moves the larger one along.`, content_cid: null, position, duration_seconds: null,
  }))
  const skills: SkillInfo[] = ['Sorting', 'Searching', 'Recursion'].map(name => ({
    id: `skill-${name.toLowerCase()}`, name, description: null, subject_id: null, subject_name: 'Computer Science',
    subject_field_id: null, subject_field_name: null, bloom_level: 'apply', prerequisite_count: 0, dependent_count: 0, created_at: null,
  }))
  const templates: GoalTemplate[] = [{ id: 'template-data-engineer', kind: 'job_role', key: 'data-engineer', label: 'Data engineer', skill_ids: ['skill-searching'], ratified: true }]
  const goals: Goal[] = [
    { id: 'goal-algorithms', label: 'Algorithms foundations', goal_skill_ids: ['skill-sorting'], created_at: '2026-11-01' },
    { id: 'goal-recursion', label: 'Recursive thinking', goal_skill_ids: ['skill-recursion'], created_at: '2026-11-02' },
  ]
  const entry = (key: string, scope: SettingEntry['scope'], kind: SettingEntry['kind'], value: string): SettingEntry => ({
    key, scope, category: '', label: key, description: '', kind, default_value: value, current_value: value, is_default: true,
  })
  const settings: SettingEntry[] = [
    entry('learner.targets', 'sync', 'json', JSON.stringify(goals)),
    entry('ui.active_mode', 'device', 'string', 'learner'),
    entry('ui.sidebar_sections', 'sync', 'json', '{}'),
  ]
  const identity: Identity = { stake_address: 'stake-fixture', payment_address: 'payment-fixture', username: null,
    display_name: 'Motion learner', bio: null, avatar_cid: null, visibility: null, profile_hash: null, created_at: '', updated_at: '' }
  const unexpected: string[] = []
  const answers: Record<string, unknown> = {
    get_incompatible_profile_data: null, get_profile_cleanup_required: false, list_profiles: [],
    get_active_profile_id: 'profile-motion', get_profile_session_token: 'session-motion', get_profile: identity,
    get_account_status: { roles: ['learner', 'instructor'], activation_state: 'active', is_minor: false },
    get_local_did: 'did:fixture:motion', 'plugin:event|listen': 1, hydrate_catalog_courses: 0,
    list_courses: courses, list_enrollments: enrollments, list_skills: skills, list_goal_templates: templates,
    get_progress: [], get_course_completion_status: { ready: false, missing_elements: [], required_count: 0, preview: null },
    assessment_plan_goal: { steps: [] }, instructor_inbox: [], search_catalog: [], decision_search: {},
  }

  async function invoke(command: string, args?: Record<string, unknown>): Promise<unknown> {
    switch (command) {
      case 'list_settings': return structuredClone(settings)
      case 'set_setting': {
        const found = settings.find(item => item.key === args?.key)
        if (found) found.current_value = String(args?.value)
        return null
      }
      case 'get_course': return courses.find(item => item.id === args?.courseId) ?? null
      case 'list_chapters': return chapters.filter(item => item.course_id === args?.courseId)
      case 'list_elements': return elements.filter(item => item.chapter_id === args?.chapterId)
      case 'resolve_goal': return { label: 'Data engineer', goal_skill_ids: ['skill-searching'], suggestions: [], resolution_provenance: 'template', taxonomy_version: 'fixture' }
      case 'compute_learning_path': return { steps: [], total: 1, earned_count: 0, goal_skill_ids: args?.goalSkillIds ?? [] }
    }
    if (command in answers) return structuredClone(answers[command])
    unexpected.push(command)
    return command.startsWith('list_') || command.endsWith('_list') ? [] : null
  }
  return { invoke, unexpected }
}
