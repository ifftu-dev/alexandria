import type {
  Chapter, Course, Element, ElementProgress, Enrollment, OpenedPackageLaunch, Package, PackageActivity, PackageActivityResult,
  PackageAttempt, PackageAttemptProgress, PackageCommit, PackageDescription, PackageLaunchInit,
} from '@/types'
import playerDescription from './package-player/description.json'
import isolationDescription from './package-isolation/description.json'
import scorm12Description from './scorm12-conformance/description.json'

/** The origin the package scheme answers at in these checks; the spec serves it with `page.route`. */
export const PACKAGE_ORIGIN = 'http://scorm-package.test'

const HASH = 'ab'.repeat(32)
const ENROLLMENT = 'enrollment-safety'
const ELEMENT = 'element-safety'
const LEARNER_ID = 'learner-for-this-package'

/** The fixture packages a check can load, each a directory under `tests/desktop-ui/`. */
const DESCRIPTIONS = {
  'package-player': playerDescription,
  'package-isolation': isolationDescription,
  'scorm12-conformance': scorm12Description,
} as const
export type PackageFixtureName = keyof typeof DESCRIPTIONS
export const isPackageFixtureName = (name: unknown): name is PackageFixtureName =>
  typeof name === 'string' && Object.prototype.hasOwnProperty.call(DESCRIPTIONS, name)

/** A CMITimespan, the way the host writes `total_time`. */
function timespan(seconds: number): string {
  const centis = Math.round(Math.max(0, seconds) * 100)
  const whole = Math.floor(centis / 100)
  const pad = (value: number, width: number) => String(value).padStart(width, '0')
  const text = `${pad(Math.floor(whole / 3600), 4)}:${pad(Math.floor(whole / 60) % 60, 2)}:${pad(whole % 60, 2)}`
  return centis % 100 ? `${text}.${pad(centis % 100, 2)}` : text
}

function seconds(span: unknown): number {
  const match = /^(\d+):(\d+):(\d+(?:\.\d+)?)$/.exec(String(span ?? '').trim())
  return match ? Number(match[1]) * 3600 + Number(match[2]) * 60 + Number(match[3]) : 0
}

type Cmi = Record<string, unknown> & { core?: Record<string, unknown> }

interface StoredActivity {
  cmi: Cmi
  complete: boolean
  score: number | null
  /** Time of finished sessions, as the host accumulates it. */
  totalSeconds: number
}

/**
 * A fake host bridge for the package player: an unlocked learner enrolled in
 * a course whose only element is the SCORM 1.2 fixture package. It keeps
 * attempts and run-time state the way the real host does (ticket 05's rules,
 * in miniature) so a test can launch, save, leave, come back and finish.
 * Every command is recorded; unknown ones answer an empty list or null.
 */
export function packageBackend(fixture: PackageFixtureName = 'package-player') {
  const packageDescription = DESCRIPTIONS[fixture] as unknown as PackageDescription
  const course: Course = {
    id: 'course-safety', title: 'Workshop safety', kind: 'course', description: 'A packaged course', author_address: 'stake-instructor',
    author_name: 'Fixture instructor', content_cid: null, thumbnail_cid: null, thumbnail_svg: null, tags: null, skill_ids: [],
    version: 1, status: 'published', published_at: '2026-11-01', on_chain_tx: null, created_at: '2026-11-01', updated_at: '2026-11-01',
  }
  const enrollment: Enrollment = { id: ENROLLMENT, course_id: course.id, course_document_cid: null, course_document_version: null,
    completion_policy_json: null, enrolled_at: '2026-11-01', completed_at: null, status: 'active', updated_at: '2026-11-01' }
  const chapter: Chapter = { id: 'chapter-safety', course_id: course.id, title: 'The package', description: null, position: 0 }
  const element: Element = { id: ELEMENT, chapter_id: chapter.id, title: 'Workshop safety', element_type: 'package',
    content_cid: HASH, content_inline: null, position: 0, duration_seconds: null }
  const stored: Package = { hash: HASH, standard: 'scorm12', title: packageDescription.title, description: packageDescription,
    archive_bytes: 4096, location: 'draft', imported_at: '2026-11-01T00:00:00Z', last_opened_at: null }

  const calls: { command: string, args: unknown }[] = []
  const commits: PackageCommit[] = []
  /** When each commit reached the host, in milliseconds. */
  const commitTimes: number[] = []
  const progress = new Map<string, ElementProgress>()
  const attempts: PackageAttempt[] = []
  const state = new Map<string, Map<string, StoredActivity>>()
  const openLaunches = new Set<string>()
  const control = { failCommits: false }

  const launchable = (): PackageActivity[] => {
    const found: PackageActivity[] = []
    const walk = (activity: PackageActivity) => {
      if (activity.launch) found.push(activity)
      for (const child of activity.children ?? []) walk(child)
    }
    walk(packageDescription.tree)
    return found
  }

  const now = () => new Date().toISOString()

  function startAttempt(): PackageAttempt {
    const attempt: PackageAttempt = { id: `attempt-${attempts.length + 1}`, enrollment_id: ENROLLMENT, element_id: ELEMENT,
      package_hash: HASH, attempt_number: attempts.length + 1, status: 'in_progress', started_at: now(), ended_at: null, updated_at: now() }
    attempts.push(attempt)
    state.set(attempt.id, new Map())
    return attempt
  }

  function writeProgress(status: string, score: number | null): ElementProgress {
    const before = progress.get(ELEMENT)
    const next: ElementProgress = { id: 'progress-safety', enrollment_id: ENROLLMENT, element_id: ELEMENT,
      status: before?.status === 'completed' ? 'completed' : status, score: score ?? before?.score ?? null, time_spent: 0,
      completed_at: status === 'completed' ? now() : before?.completed_at ?? null, updated_at: now() }
    progress.set(ELEMENT, next)
    return next
  }

  // SCORM 1.2 rollup: complete when every lesson is completed or passed (an
  // asset when launched); score the mean of the reported lesson scores.
  function rollUp(attempt: PackageAttempt): PackageAttemptProgress {
    const activities = state.get(attempt.id)!
    const complete = launchable().every(activity => activities.get(activity.id)?.complete)
    const scores = [...activities.values()].map(item => item.score).filter((score): score is number => score !== null)
    const score = scores.length ? scores.reduce((sum, value) => sum + value, 0) / scores.length : null
    if (complete && attempt.status === 'in_progress') {
      attempt.status = 'completed'
      attempt.ended_at = now()
    }
    return { attempt: { ...attempt }, element_progress: writeProgress(complete ? 'completed' : 'in_progress', score),
      package_completed: complete, cache_dropped: false }
  }

  function current(): PackageAttempt | undefined {
    return [...attempts].reverse().find(attempt => attempt.status !== 'closed')
  }

  function attemptById(id: unknown): PackageAttempt {
    const attempt = attempts.find(item => item.id === id)
    if (!attempt) throw new Error(`not_found: attempt ${String(id)}`)
    return attempt
  }

  function initialise(attemptId: unknown, activityId: unknown): PackageLaunchInit {
    const attempt = attemptById(attemptId)
    const activity = launchable().find(item => item.id === activityId)
    if (!activity?.launch) throw new Error(`not_found: activity ${String(activityId)}`)
    const launch = activity.launch
    const prior = state.get(attempt.id)!.get(activity.id)
    let launched: PackageAttemptProgress | null = null
    if (launch.kind === 'asset' && attempt.status !== 'closed') {
      state.get(attempt.id)!.set(activity.id, { cmi: {}, complete: true, score: null, totalSeconds: 0 })
      launched = rollUp(attempt)
    }
    const priorCore = { ...(prior?.cmi.core ?? {}) }
    const entry = !prior ? 'ab-initio' : priorCore.exit === 'suspend' ? 'resume' : ''
    delete priorCore.session_time
    delete priorCore.exit
    const cmi: Cmi = {
      ...structuredClone(prior?.cmi ?? {}),
      core: { ...priorCore, student_id: LEARNER_ID, student_name: 'Ada Learner', entry, credit: 'credit', lesson_mode: 'normal',
        total_time: timespan(prior?.totalSeconds ?? 0) },
      ...(launch.launch_data ? { launch_data: launch.launch_data } : {}),
      student_data: {
        ...(launch.mastery_score !== undefined ? { mastery_score: String(launch.mastery_score) } : {}),
        ...(launch.max_time_allowed ? { max_time_allowed: launch.max_time_allowed } : {}),
        ...(launch.time_limit_action ? { time_limit_action: launch.time_limit_action } : {}),
      },
    }
    return { attempt_id: attempt.id, activity_id: activity.id, standard: 'scorm12', learner_id: LEARNER_ID, learner_name: 'Ada Learner',
      entry, launch_data: launch.launch_data ?? null, mastery_score: launch.mastery_score ?? null,
      max_time_allowed: launch.max_time_allowed ?? null, time_limit_action: launch.time_limit_action ?? null, runtime: { cmi }, progress: launched }
  }

  function commit(request: PackageCommit): PackageAttemptProgress {
    if (control.failCommits) throw new Error('database: disk I/O error')
    const attempt = attemptById(request.attempt_id)
    if (attempt.status === 'closed') throw new Error(`attempt_closed: ${attempt.id}`)
    commits.push(structuredClone(request))
    commitTimes.push(Date.now())
    const cmi = request.runtime.cmi as Cmi
    const core = cmi.core ?? {}
    const status = String(core.lesson_status ?? '')
    const raw = (core.score as { raw?: string } | undefined)?.raw
    const before = state.get(attempt.id)!.get(request.activity_id)?.totalSeconds ?? 0
    state.get(attempt.id)!.set(request.activity_id, {
      cmi: structuredClone(cmi),
      complete: status === 'completed' || status === 'passed',
      score: raw ? Number(raw) / 100 : null,
      totalSeconds: request.session_ended ? before + seconds(core.session_time) : before,
    })
    return rollUp(attempt)
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
      case 'update_progress': {
        const req = args.req as { status: string, score: number | null }
        return writeProgress(req.status, req.score)
      }
      case 'get_course_completion_status':
        return { ready: false, missing_elements: [], required_count: 0, preview: null, unmet_elements: [] }
      case 'claim_course_completion':
        throw new Error('no credential: package results are self-reported and this course has no assessed element')
      case 'package_get': return args.hash === HASH ? structuredClone(stored) : null
      case 'package_launch_open': {
        // Unguessable, like the host's 128-bit prefixes, so a check can tell
        // whether content learned another launch's address or guessed it.
        const digits = Array.from(crypto.getRandomValues(new Uint32Array(4)), value => String(value).padStart(10, '0')).join('')
        const prefix = `launch-${digits}`
        openLaunches.add(prefix)
        const opened: OpenedPackageLaunch = { prefix, package_hash: HASH, base_url: `${PACKAGE_ORIGIN}/${prefix}/` }
        return opened
      }
      case 'package_launch_close': return openLaunches.delete(String(args.prefix))
      case 'package_attempt_open': {
        const attempt = current() ?? startAttempt()
        writeProgress('in_progress', null)
        return { ...attempt }
      }
      case 'package_attempt_fresh': {
        const previous = current()
        if (previous) previous.status = 'closed'
        return { ...startAttempt() }
      }
      case 'package_attempt_activities': {
        const activities = state.get(attemptById(args.attemptId).id)!
        return launchable().map((activity): PackageActivityResult => {
          const item = activities.get(activity.id)
          return { activity_id: activity.id, started: Boolean(item), complete: item?.complete ?? false, score: item?.score ?? null }
        })
      }
      case 'package_attempt_initialise': return initialise(args.attemptId, args.activityId)
      case 'package_attempt_commit': return commit(args.commit as PackageCommit)
      case 'package_attempt_terminate': return rollUp(attemptById(args.attemptId))
      default:
        if (command.startsWith('list_') || command.endsWith('_list')) return []
        return null
    }
  }

  return { invoke, calls, commits, commitTimes, control, openLaunches, called: (command: string) => calls.filter(call => call.command === command) }
}
