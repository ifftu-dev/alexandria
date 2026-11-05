import type {
  AppNotification, Chapter, ChatMessage, ChatPreparedMessage, ChatThread, Classroom, ClassroomChannel, ClassroomMessage, Course, Element, Enrollment,
  Goal, GoalTemplate, GuardianAiRequest, GuardianLinkInfo, Identity, SkillInfo, StudioConnection, StudioDocument, TutoringSessionInfo,
  TutoringSessionStatus,
} from '@/types'
import type { SettingEntry } from '@/composables/useSettings'

/**
 * An unlocked learner (who also teaches, so the mode switcher shows) with two
 * goals, a small catalog and one enrolled course of three readings: enough for
 * the motion surfaces to render real data. The chat thread already holds
 * more history than its panel shows, and answers each question at once.
 * Unknown commands are recorded, then
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
    entry('notifications.enabled', 'sync', 'bool', 'true'),
  ]
  const identity: Identity = { stake_address: 'stake-instructor', payment_address: 'payment-fixture', username: null,
    display_name: 'Motion learner', bio: null, avatar_cid: null, visibility: null, profile_hash: null, created_at: '', updated_at: '' }
  const connection: StudioDocument<StudioConnection> = { id: 'connection-fixture', revision: 1, value: {
    id: 'connection-fixture', name: 'Fixture model', endpoint: 'http://127.0.0.1:11434', model: 'fixture-model',
    location: 'local', capability: 'text', enabled: true, has_key: false } }
  const thread: ChatThread = { id: 'thread-fixture', title: 'Sorting', mode: 'ask', connection_id: connection.id, model_id: null,
    revision: 1, reviewed_scope: null, created_at: '2026-11-04', updated_at: '2026-11-04' }
  const chatMessages: ChatMessage[] = []
  const chatMessage = (role: ChatMessage['role'], content: string, replyTo: string | null): ChatMessage => ({
    id: `message-${chatMessages.length + 1}`, thread_id: thread.id, position: chatMessages.length, role, reply_to: replyTo, version: 1,
    content, status: 'complete', error: null, connection_id: connection.id, connection_name: 'Fixture model', model_id: 'fixture-model',
    location: 'device', input_tokens: null, output_tokens: null, sources: [], created_at: '2026-11-04', completed_at: '2026-11-04',
  })
  const prepared = (text: string): ChatPreparedMessage => ({ prepare_id: `prepare-${text.length}`, digest: 'fixture', expires_at: '2026-11-05',
    destination: { connection_id: connection.id, connection_name: 'Fixture model', location: 'device', host: '127.0.0.1', model_id: 'fixture-model' },
    items: [], omitted_turns: 0, estimated_input_tokens: 10, budget_tokens: 1000, output_reserve_tokens: 100, estimate_kind: 'conservative',
    requires_review: false, fits: true })
  for (const topic of ['bubble sort', 'insertion sort', 'merge sort']) {
    const question = chatMessage('user', `How does ${topic} work?`, null)
    chatMessages.push(question)
    chatMessages.push(chatMessage('assistant', `${topic} compares neighbouring items. `.repeat(12), question.id))
  }
  let pendingQuestion = ''

  // A live tutoring call this device is in, with the camera off.
  const call: TutoringSessionStatus = { session_id: 'session-fixture', session_title: 'Sorting help', ticket: 'ticket-fixture',
    peers: [], video_enabled: false, audio_enabled: true, screen_sharing: false, camera_name: null, started_at: Date.now() }
  const callInfo: TutoringSessionInfo = { id: call.session_id, title: call.session_title, ticket: call.ticket, status: 'active',
    created_at: '2026-11-04', ended_at: null, kind: 'tutoring', context_id: null }

  // A classroom with one channel and a short conversation.
  const classroom: Classroom = { id: 'classroom-fixture', name: 'Algorithms club', description: null, icon_emoji: null,
    owner_address: 'stake-instructor', invite_code: null, status: 'active', created_at: '2026-11-01', updated_at: '2026-11-01',
    member_count: 2, my_role: 'owner' }
  const channel: ClassroomChannel = { id: 'channel-general', classroom_id: classroom.id, name: 'general', description: null,
    channel_type: 'text', position: 0, created_at: '2026-11-01' }
  const classroomMessages: ClassroomMessage[] = ['Welcome to the club', 'Bring one sorting puzzle'].map((content, index) => ({
    id: `classroom-message-${index + 1}`, channel_id: channel.id, classroom_id: classroom.id, sender_address: 'stake-instructor',
    sender_name: 'Fixture instructor', content, deleted: false, edited_at: null, sent_at: `2026-11-04T09:0${index}:00Z`, received_at: `2026-11-04T09:0${index}:00Z` }))

  // One linked child, who asked to use a remote model.
  const child: GuardianLinkInfo = { id: 'link-fixture', side: 'guardian', peer_did: 'did:fixture:child-learner-0001', peer_display_name: 'Asha',
    status: 'active', child_birthdate: '2015-03-01', created_at: '2026-11-01', last_sync_at: '2026-11-04T08:00:00Z' }
  const aiRequests: GuardianAiRequest[] = [{ id: 'ai-request-fixture', link_id: child.id, ward_name: 'Asha', connection_name: 'Study helper',
    endpoint: 'https://models.example/v1', model: 'helper-small', status: 'pending', received_at: '2026-11-04', decided_at: null, delivered: false }]

  // The inbox: an unread offer above a read credential notice.
  const notification = (id: string, kind: AppNotification['kind'], createdAt: string, readAt: string | null): AppNotification => ({
    id, kind, title: '', body: '', route: null, source: 'local', created_at: createdAt, read_at: readAt, payload: null })
  const notifications: AppNotification[] = [
    notification('notification-offer', 'offer_sent', '2026-11-04T10:00:00Z', null),
    notification('notification-credential', 'credential_issued', '2026-11-04T09:00:00Z', '2026-11-04T09:30:00Z'),
  ]
  /** A new row the backend stores and announces. */
  function notify(kind: AppNotification['kind']): AppNotification {
    const row = notification(`notification-${notifications.length + 1}`, kind, '2026-11-04T11:00:00Z', null)
    notifications.unshift(row)
    return structuredClone(row)
  }
  /** Another window removed a row; returns the new unread count. */
  function removeNotification(kind: AppNotification['kind']): number {
    const index = notifications.findIndex(item => item.kind === kind)
    if (index >= 0) notifications.splice(index, 1)
    return notifications.filter(item => !item.read_at).length
  }

  const unexpected: string[] = []
  const answers: Record<string, unknown> = {
    get_incompatible_profile_data: null, get_profile_cleanup_required: false, list_profiles: [],
    get_active_profile_id: 'profile-motion', get_profile_session_token: 'session-motion', get_profile: identity,
    get_account_status: { roles: ['learner', 'instructor'], activation_state: 'active', is_minor: false },
    get_local_did: 'did:fixture:motion', 'plugin:event|listen': 1, hydrate_catalog_courses: 0,
    list_courses: courses, list_enrollments: enrollments, list_skills: skills, list_goal_templates: templates,
    get_progress: [], get_course_completion_status: { ready: false, missing_elements: [], required_count: 0, preview: null },
    assessment_plan_goal: { steps: [] }, instructor_inbox: [], search_catalog: [], decision_search: {},
    chat_status: { assessment_active: false }, chat_get_draft: '',
    get_wallet_info: { stake_address: 'stake-instructor', payment_address: 'payment-fixture', has_mnemonic_backup: true },
    tutoring_list_sessions: [callInfo], tutoring_peers: [], tutoring_send_chat: null,
    classroom_list: [classroom], classroom_get: classroom, classroom_list_channels: [channel], classroom_get_active_call: null, classroom_list_join_requests: [], classroom_subscribe: null,
    classroom_list_members: [{ classroom_id: classroom.id, stake_address: 'stake-instructor', role: 'owner', display_name: 'Fixture instructor', joined_at: '2026-11-01' }],
    studio_get_course: { id: 'studio-course-sorting', revision: 1, value: { audience: '', prerequisites: '', outcome: '', initial_prompt: '',
      sources: [], tutor: { enabled: false, guidance: 'socratic', initial_prompt: '' } } },
    studio_list_lesson_feedback: [], get_course_completion_policy: null,
    guardian_list_links: [child], guardian_get_child_activity: [], guardian_sync_now: 0,
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
      case 'create_element': {
        const req = args?.req as { title: string, element_type: string }
        const created: Element = { id: `element-${elements.length + 1}`, chapter_id: String(args?.chapterId), title: req.title,
          element_type: req.element_type, content_inline: null, content_cid: null, position: elements.length, duration_seconds: null }
        elements.push(created)
        return structuredClone(created)
      }
      case 'resolve_goal': return { label: 'Data engineer', goal_skill_ids: ['skill-searching'], suggestions: [], resolution_provenance: 'template', taxonomy_version: 'fixture' }
      case 'studio_list_connections': return [connection]
      case 'chat_create_thread': return thread
      case 'chat_list_threads': return [thread]
      case 'chat_prepare_message':
        pendingQuestion = String(args?.text ?? '')
        return prepared(pendingQuestion)
      case 'chat_start_run': {
        const question = chatMessage('user', pendingQuestion, null)
        chatMessages.push(question)
        const answer = chatMessage('assistant', 'A stable sort keeps equal items in their original order.', question.id)
        chatMessages.push(answer)
        return { run_id: `run-${answer.id}`, thread_id: thread.id, user_message: question, assistant_message: answer, existing: false }
      }
      case 'chat_get_thread': return { thread, messages: structuredClone(chatMessages), bindings: [], practice: [], memory_proposals: [], artifacts: [], active_run_id: null }
      case 'tutoring_status': return structuredClone(call)
      case 'tutoring_toggle_audio': call.audio_enabled = Boolean(args?.enable); return call.audio_enabled
      case 'tutoring_toggle_video': call.video_enabled = Boolean(args?.enable); return call.video_enabled
      case 'classroom_get_messages': {
        // "Load earlier messages" asks for what came before the oldest shown.
        if (!args?.beforeId) return structuredClone(classroomMessages)
        return [{ ...classroomMessages[0]!, id: 'classroom-message-earlier', content: 'An earlier hello', sent_at: '2026-11-04T08:00:00Z', received_at: '2026-11-04T08:00:00Z' }]
      }
      case 'classroom_send_message': {
        const sent: ClassroomMessage = { ...classroomMessages[0]!, id: `classroom-message-${classroomMessages.length + 1}`,
          sender_address: 'stake-instructor', content: String(args?.content), sent_at: '2026-11-04T09:30:00Z', received_at: '2026-11-04T09:30:00Z' }
        classroomMessages.push(sent)
        return structuredClone(sent)
      }
      case 'guardian_list_ai_requests': return structuredClone(aiRequests)
      case 'guardian_decide_ai_request': {
        const found = aiRequests.find(item => item.id === args?.requestId)
        if (found) Object.assign(found, { status: args?.approve ? 'approved' : 'declined', decided_at: '2026-11-04' })
        return null
      }
      case 'notifications_list': return structuredClone(notifications)
      case 'notifications_unread_count': return notifications.filter(item => !item.read_at).length
      case 'compute_learning_path': return { steps: [], total: 1, earned_count: 0, goal_skill_ids: args?.goalSkillIds ?? [] }
    }
    if (command in answers) return structuredClone(answers[command])
    unexpected.push(command)
    return command.startsWith('list_') || command.endsWith('_list') ? [] : null
  }
  return { invoke, unexpected, notify, removeNotification }
}
