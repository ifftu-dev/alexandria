import type {
  Chapter, Cmi5Launch, Course, Element, ElementProgress, Enrollment, OpenedPackageLaunch, Package, PackageActivity,
  PackageActivityResult, PackageAttempt, PackageAttemptProgress, PackageDescription,
} from '@/types'
import description from './package-player/cmi5/description.json'

/** The origin the package scheme answers at in these checks; the spec serves it with `page.route`. */
export const PACKAGE_ORIGIN = 'http://cmi5-package.test'
/** The loopback port the fake session endpoint answers at; the spec routes it to `endpoint`. */
export const ENDPOINT_PORT = 47125

const HASH = 'cd'.repeat(32)
const ENROLLMENT = 'enrollment-forklift'
const ELEMENT = 'element-forklift'
const LEARNER_ID = 'learner-for-this-cmi5-package'
const CMI5 = 'https://w3id.org/xapi/cmi5/context/categories/cmi5'
const VERBS = 'http://adlnet.gov/expapi/verbs/'
const MOVE_ON: Record<string, string> = {
  passed: 'Passed', completed: 'Completed', completed_and_passed: 'CompletedAndPassed',
  completed_or_passed: 'CompletedOrPassed', not_applicable: 'NotApplicable',
}

interface Session {
  id: string
  attemptId: string
  unit: PackageActivity
  token: string
  fetchCode: string
  fetched: boolean
  registration: string
  activityId: string
  verbs: string[]
  ended: 'terminated' | 'abandoned' | null
}

/** One request to the fake endpoint, as the spec's route hands it over. */
export interface EndpointRequest {
  method: string
  url: string
  authorization: string | null
  version: string | null
  body: string | null
}

export interface EndpointAnswer {
  status: number
  body: string
}

/**
 * A fake host bridge for the package player over the cmi5 fixture package,
 * and a fake session endpoint in its place on the loopback address. The
 * session rules themselves are the Rust host's and are tested there; this
 * keeps only enough of them (one-use token, `initialized` first and once,
 * nothing after `terminated`, moveOn satisfaction) for the player's side of
 * a session to be watched end to end.
 */
export function cmi5Backend() {
  const packageDescription = description as unknown as PackageDescription
  const course: Course = {
    id: 'course-forklift', title: 'Forklift basics', kind: 'course', description: 'A cmi5 course', author_address: 'stake-instructor',
    author_name: 'Fixture instructor', content_cid: null, thumbnail_cid: null, thumbnail_svg: null, tags: null, skill_ids: [],
    version: 1, status: 'published', published_at: '2026-11-01', on_chain_tx: null, created_at: '2026-11-01', updated_at: '2026-11-01',
  }
  const enrollment: Enrollment = { id: ENROLLMENT, course_id: course.id, course_document_cid: null, course_document_version: null,
    completion_policy_json: null, enrolled_at: '2026-11-01', completed_at: null, status: 'active', updated_at: '2026-11-01' }
  const chapter: Chapter = { id: 'chapter-forklift', course_id: course.id, title: 'The package', description: null, position: 0 }
  const element: Element = { id: ELEMENT, chapter_id: chapter.id, title: 'Forklift basics', element_type: 'package',
    content_cid: HASH, content_inline: null, position: 0, duration_seconds: null }
  const stored: Package = { hash: HASH, standard: 'cmi5', title: packageDescription.title, description: packageDescription,
    archive_bytes: 4096, location: 'draft', imported_at: '2026-11-01T00:00:00Z', last_opened_at: null }

  const calls: { command: string, args: unknown }[] = []
  const statements: { unit: string, verb: string, status: number }[] = []
  const progress = new Map<string, ElementProgress>()
  const attempt: PackageAttempt = { id: 'attempt-1', enrollment_id: ENROLLMENT, element_id: ELEMENT, package_hash: HASH,
    attempt_number: 1, status: 'in_progress', started_at: '2026-11-01', ended_at: null, updated_at: '2026-11-01' }
  /** Per unit, the cmi5 defined verbs it used in the registration. */
  const used = new Map<string, Set<string>>()
  const sessions = new Map<string, Session>()
  let launches = 0
  let count = 0

  const units = (): PackageActivity[] => {
    const found: PackageActivity[] = []
    const walk = (activity: PackageActivity) => {
      if (activity.launch) found.push(activity)
      for (const child of activity.children ?? []) walk(child)
    }
    walk(packageDescription.tree)
    return found
  }

  const satisfied = (unit: PackageActivity) => {
    const verbs = used.get(unit.id) ?? new Set()
    switch (unit.cmi5?.move_on) {
      case 'passed': return verbs.has('passed')
      case 'completed': return verbs.has('completed')
      case 'completed_and_passed': return verbs.has('completed') && verbs.has('passed')
      case 'completed_or_passed': return verbs.has('completed') || verbs.has('passed')
      default: return true
    }
  }

  function rollUp(): PackageAttemptProgress {
    const complete = units().every(satisfied)
    if (complete) attempt.status = 'completed'
    const before = progress.get(ELEMENT)
    const next: ElementProgress = { id: 'progress-forklift', enrollment_id: ENROLLMENT, element_id: ELEMENT,
      status: complete || before?.status === 'completed' ? 'completed' : 'in_progress', score: null, time_spent: 0,
      completed_at: complete ? new Date().toISOString() : null, updated_at: new Date().toISOString() }
    progress.set(ELEMENT, next)
    return { attempt: { ...attempt }, element_progress: next, package_completed: complete, cache_dropped: false }
  }

  function launch(args: Record<string, unknown>): Cmi5Launch {
    const unit = units().find(item => item.id === args.activityId)
    if (!unit?.launch) throw new Error(`not_found: activity ${String(args.activityId)}`)
    for (const open of sessions.values()) if (!open.ended) end(open.id)
    count += 1
    const session: Session = {
      id: `00000000-0000-4000-8000-${String(count).padStart(12, '0')}`, attemptId: attempt.id, unit,
      token: `token-${count}`, fetchCode: `code-${count}`, fetched: false,
      registration: '11111111-2222-4333-8444-555555555555', activityId: `urn:fixture:${unit.id}`, verbs: [], ended: null,
    }
    sessions.set(session.id, session)
    const endpoint = `http://127.0.0.1:${ENDPOINT_PORT}/`
    const actor = { objectType: 'Agent', account: { homePage: 'urn:fixture', name: LEARNER_ID } }
    const cmi5 = `endpoint=${encodeURIComponent(`${endpoint}xapi/`)}&fetch=${encodeURIComponent(`${endpoint}fetch/${session.fetchCode}`)}`
      + `&actor=${encodeURIComponent(JSON.stringify(actor))}&registration=${session.registration}`
      + `&activityId=${encodeURIComponent(session.activityId)}`
    const own = unit.launch.parameters ?? ''
    const query = own.startsWith('?') ? `${own}&${cmi5}` : `?${cmi5}`
    const ownWindow = unit.cmi5?.launch_method === 'own_window'
    return { session_id: session.id, attempt_id: attempt.id, activity_id: unit.id,
      url: `${PACKAGE_ORIGIN}/${String(args.prefix)}/${unit.launch.path}${query}`, own_window: ownWindow, focus_forced: false }
  }

  function end(sessionId: unknown): PackageAttemptProgress {
    const session = sessions.get(String(sessionId))
    if (!session) throw new Error(`not_found: cmi5 session ${String(sessionId)}`)
    if (!session.ended) {
      session.ended = 'abandoned'
      statements.push({ unit: session.unit.id, verb: 'abandoned', status: 200 })
    }
    return rollUp()
  }

  const answer = (status: number, body: unknown): EndpointAnswer => ({ status, body: body === undefined ? '' : JSON.stringify(body) })

  /** The fake session endpoint. */
  function endpoint(request: EndpointRequest): EndpointAnswer {
    const url = new URL(request.url)
    const live = [...sessions.values()].find(session => !session.ended)
    if (!live) return answer(403, { error: 'the session is not open' })
    if (url.pathname.startsWith('/fetch/')) {
      if (request.method !== 'POST') return answer(405, { error: 'POST' })
      if (url.pathname !== `/fetch/${live.fetchCode}`) return answer(200, { 'error-code': '2' })
      if (live.fetched) return answer(200, { 'error-code': '1', 'error-text': 'already returned' })
      live.fetched = true
      return answer(200, { 'auth-token': live.token })
    }
    if (request.authorization !== `Basic ${live.token}`) return answer(401, { error: 'token' })
    if (!request.version) return answer(400, { error: 'version' })
    const resource = url.pathname.replace(/^\/xapi\//, '')
    if (resource === 'activities/state' && url.searchParams.get('stateId') === 'LMS.LaunchData') {
      const unit = live.unit
      return answer(200, {
        contextTemplate: {
          contextActivities: { grouping: [{ objectType: 'Activity', id: unit.id }] },
          extensions: { 'https://w3id.org/xapi/cmi5/context/extensions/sessionid': live.id },
        },
        launchMode: 'Normal',
        moveOn: MOVE_ON[unit.cmi5?.move_on ?? 'not_applicable'],
        ...(unit.launch?.mastery_score !== undefined ? { masteryScore: unit.launch.mastery_score } : {}),
        ...(unit.launch?.launch_data ? { launchParameters: unit.launch.launch_data } : {}),
        returnURL: `${PACKAGE_ORIGIN}/launch-${launches}/_alexandria/returned.html`,
      })
    }
    if (resource === 'agents/profile') return answer(200, { languagePreference: 'en-US', audioPreference: 'on' })
    if (resource === 'statements' && request.method === 'POST') {
      const statement = JSON.parse(request.body ?? '{}') as { verb?: { id?: string }, context?: { contextActivities?: { category?: { id: string }[] } } }
      const verb = (statement.verb?.id ?? '').replace(VERBS, '')
      const defined = statement.context?.contextActivities?.category?.some(item => item.id === CMI5) ?? false
      const refuse = live.ended
        || (live.verbs.length === 0 && verb !== 'initialized')
        || (defined && live.verbs.includes(verb))
      const status = refuse ? 403 : 200
      statements.push({ unit: live.unit.id, verb, status })
      if (refuse) return answer(403, { error: 'refused by the cmi5 session rules' })
      if (defined) {
        live.verbs.push(verb)
        used.set(live.unit.id, (used.get(live.unit.id) ?? new Set()).add(verb))
        if (verb === 'terminated') live.ended = 'terminated'
      }
      return answer(200, [crypto.randomUUID()])
    }
    return answer(404, { error: 'not here' })
  }

  async function invoke(command: string, args: Record<string, unknown> = {}): Promise<unknown> {
    calls.push({ command, args: command === 'package_launch_open' ? { hash: args.hash } : structuredClone(args) })
    switch (command) {
      case 'get_active_profile_id': return 'profile-a'
      case 'get_profile_session_token': return 'session-a'
      case 'get_profile_cleanup_required': return false
      case 'get_account_status': return { account_roles: ['learner'], activation_state: 'active' }
      case 'get_course': return args.courseId === course.id ? course : null
      case 'list_chapters': return args.courseId === course.id ? [chapter] : []
      case 'list_elements': return args.chapterId === chapter.id ? [element] : []
      case 'list_enrollments': return [enrollment]
      case 'get_progress': return [...progress.values()]
      case 'get_course_completion_status':
        return { ready: false, missing_elements: [], required_count: 0, preview: null, unmet_elements: [] }
      case 'claim_course_completion':
        throw new Error('no credential: package results are self-reported and this course has no assessed element')
      case 'package_get': return args.hash === HASH ? structuredClone(stored) : null
      case 'package_launch_open': {
        launches += 1
        const opened: OpenedPackageLaunch = { prefix: `launch-${launches}`, package_hash: HASH, base_url: `${PACKAGE_ORIGIN}/launch-${launches}/` }
        return opened
      }
      case 'package_launch_close': return true
      case 'package_attempt_open': return { ...attempt }
      case 'package_attempt_activities':
        return units().map((unit): PackageActivityResult => ({
          activity_id: unit.id, started: used.has(unit.id) || [...sessions.values()].some(s => s.unit.id === unit.id),
          complete: used.has(unit.id) && satisfied(unit), score: null,
        }))
      case 'package_cmi5_launch': return launch(args)
      case 'package_cmi5_end': return end(args.sessionId)
      case 'package_cmi5_focus': return true
      case 'package_attempt_terminate': return rollUp()
      default:
        if (command.startsWith('list_') || command.endsWith('_list')) return []
        return null
    }
  }

  return { invoke, endpoint, statements, called: (command: string) => calls.filter(call => call.command === command).length }
}
