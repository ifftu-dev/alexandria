import { appendFileSync, readFileSync } from 'node:fs'
import { readFile } from 'node:fs/promises'
import { expect, test, type Frame, type Page } from '@playwright/test'
import {
  evaluate, readBundle, readCase, type AdlCase, type AdlResult, type AdlStep, type AdlVisit,
} from './adlLmsTest'
import {
  commits, lessonFrame, navigate, openLesson, openPlayer, PACKAGE_ORIGIN, savedStates, wrapperFrame,
} from './packageHarness'
import { servePackageOrigin } from './packageOrigin'

// SCORM 2004 conformance at the player seam: the real player, wrapper and
// vendored runtime with its sequencing engine, a fake host, and two kinds of
// package:
//
// - the LMS test packages of the standards body's SCORM 2004 4th Edition
//   test suite (`scorm2004-conformance/adl/`), replayed: a probe lesson
//   stands in for each of the suite's lesson pages and carries out the case's
//   calls, checks, user interface questions and navigation in the order the
//   suite expects (`adlLmsTest.ts` reads the cases as the suite does);
// - purpose-built packages for what the suite leaves to the LMS or does not
//   reach: the probe package (`scorm2004-conformance/package/`) and the 2nd
//   and 3rd Edition packages.
//
// Each test is a row (or several) of `scorm2004-conformance.md`. Expected
// values come from the standard and the test suite, not from the library.
// Where the runtime departs from them, the test pins what it does and the
// matrix marks the row a known deviation or known limit.

const LEARNER = { id: 'learner-for-this-package', name: 'Ada Learner' }
const ADL = new URL('./scorm2004-conformance/adl/', import.meta.url)
const COMMANDS = readBundle(readFileSync(new URL('Commands.properties', ADL), 'latin1'))
const DESCRIPTIONS = JSON.parse(readFileSync(new URL('descriptions.json', ADL), 'utf8')) as Record<string, { tree: Activity }>
interface Activity { id: string, title: string, children?: Activity[] }

const adlCase = (id: string): AdlCase =>
  readCase(id, readFileSync(new URL(`cases/${id}.properties`, ADL), 'latin1'), COMMANDS, LEARNER)

// ---- Replaying the test suite's LMS test ----------------------------------

interface ProbeWindow { probe: { api: Record<string, (...args: string[]) => string> } }

/**
 * Carries out a visit's calls in the lesson, as the suite's `Command`
 * does: `element!value` split at the first `!`, an objective or data store
 * named by id (`&id&`) looked up by index first, then the call and
 * `GetLastError`. Runs inside the lesson frame.
 */
function carryOut(steps: Pick<AdlStep, 'method' | 'param'>[]): AdlResult[] {
  const api = (window as unknown as ProbeWindow).probe.api
  const get = (element: string) => String(api.GetValue!(element))
  const find = (collection: string, id: string) => {
    const count = Number.parseInt(get(`${collection}._count`), 10) || 0
    for (let index = 0; index < count; index++) if (get(`${collection}.${index}.id`) === id) return index
    return -1
  }
  const out: AdlResult[] = []
  let index = 0
  for (const step of steps) {
    if (step.method === 'compareObjIds') {
      const count = Number.parseInt(get('cmi.objectives._count'), 10) || 0
      out.push({ value: '', error: '', index, ids: Array.from({ length: count }, (_, at) => get(`cmi.objectives.${at}.id`)) })
      continue
    }
    const bang = step.param.indexOf('!')
    let element = bang > -1 ? step.param.slice(0, bang) : step.param
    const value = bang > -1 ? step.param.slice(bang + 1) : ''
    if (element.includes('&')) {
      const [head = '', id = '', tail = ''] = element.split('&')
      if (head.includes('data')) index = find('adl.data', id)
      else if (head.includes('objective')) index = find('cmi.objectives', id)
      element = `${head}${index}${tail}`
      if (index === -1) {
        out.push({ value: '', error: '', index, failed: `no objective or data store with the id ${JSON.stringify(id)}` })
        continue
      }
    }
    const method = api[step.method]!
    const returned = step.method === 'SetValue' ? method(element, value)
      : step.method === 'GetLastError' ? method()
        : method(element)
    out.push({ value: String(returned), error: String(api.GetLastError!()), index })
  }
  return out
}

/** The lesson frames under the wrapper, with what each probe reported. */
async function lessons(page: Page): Promise<{ frame: Frame, act: string | null, load: string }[]> {
  const found: { frame: Frame, act: string | null, load: string }[] = []
  for (const frame of page.frames()) {
    if (!frame.parentFrame()?.url().endsWith('/_alexandria/wrapper.html') || !frame.url().startsWith(PACKAGE_ORIGIN)) continue
    const values = await frame.locator('#seen[data-ready]').getAttribute('data-values', { timeout: 200 }).catch(() => null)
    if (!values) {
      found.push({ frame, act: null, load: '' })
      continue
    }
    const seen = JSON.parse(values) as { act: string | null, load: string }
    found.push({ frame, act: seen.act, load: seen.load })
  }
  return found
}

type Settled = { kind: 'lesson', frame: Frame, act: string | null, load: string } | { kind: 'phase', phase: string }

/**
 * Waits until a new lesson has loaded (not the one whose load is
 * `previous`) or, when `want` is `end`, the session has ended. A player that
 * shows no lesson for `idle` milliseconds has settled on that state.
 */
async function settle(page: Page, previous: string | null, want: 'lesson' | 'end' = 'lesson', idle = 3000): Promise<Settled> {
  const deadline = Date.now() + 15_000
  let idleSince: number | null = null
  for (;;) {
    const phase = await page.locator('[data-package-sequencing]').getAttribute('data-package-sequencing', { timeout: 100 }).catch(() => null)
    const found = await lessons(page)
    const fresh = found.filter(lesson => lesson.load && lesson.load !== previous)
    if (found.length === 1 && fresh.length === 1) return { kind: 'lesson', ...fresh[0]! }
    if (phase && found.length === 0) {
      if (want === 'end' && phase === 'ended') return { kind: 'phase', phase }
      idleSince ??= Date.now()
      if (Date.now() - idleSince > idle) return { kind: 'phase', phase }
    } else {
      idleSince = null
    }
    if (Date.now() > deadline) return { kind: 'phase', phase: phase ?? 'nothing' }
    await page.waitForTimeout(100)
  }
}

function titles(tree: Activity): Map<string, string> {
  const out = new Map<string, string>()
  const walk = (activity: Activity) => {
    out.set(activity.title.trim(), activity.id)
    for (const child of activity.children ?? []) walk(child)
  }
  walk(tree)
  return out
}

/**
 * The answer a tester would give to one of the suite's user interface
 * questions, read off the player. A control the package hides counts as
 * disabled: the suite's own answers treat it so (CM-01 and OB-04 hide
 * Continue and expect "a disabled Continue").
 */
async function answer(page: Page, question: string): Promise<'Y' | 'N'> {
  const at = question.indexOf('@')
  const key = at < 0 ? question : question.slice(0, at)
  const title = at < 0 ? '' : question.slice(at + 1)
  const yes = (value: boolean) => (value ? 'Y' : 'N')
  const control = async (name: string, enabled: boolean) => {
    const button = page.locator(`[data-package-nav="${name}"]`)
    const usable = (await button.count()) > 0 && (await button.isEnabled())
    return yes(usable === enabled)
  }
  const toc = page.locator('[data-package-toc]')
  const entry = toc.getByText(title, { exact: true })
  switch (key) {
    case 'continue.enabled': return control('continue', true)
    case 'continue.disabled': return control('continue', false)
    case 'previous.enabled': return control('previous', true)
    case 'previous.disabled': return control('previous', false)
    case 'toc.enabled': return yes((await toc.locator('button[data-activity]:not([disabled])').count()) > 0)
    case 'toc.enabled.selectable': {
      const button = toc.locator('button[data-activity]').filter({ has: page.getByText(title, { exact: true }) })
      return yes((await button.count()) > 0 && (await button.first().isEnabled()))
    }
    case 'toc.visible': return yes((await entry.count()) > 0)
    // SX-09: "Click the button below to continue."
    case 'click.button': return 'Y'
    default: throw new Error(`unknown user interface question ${question}`)
  }
}

/** What the tester does for one of the suite's current user instructions. */
type Action = { kind: 'continue' | 'previous' | 'suspendAll' | 'exitAll' | 'relaunch' } | { kind: 'choice', title: string } | { kind: 'unsupported', why: string }

function actions(instruction: string): Action[] {
  const [name = '', argument = ''] = instruction.split('~')
  const choose = (title: string): Action => title.startsWith('the activity that corresponds to the root')
    ? { kind: 'unsupported', why: 'the table of contents offers no entry for the root of the activity tree' }
    : { kind: 'choice', title }
  switch (name) {
    case '': case 'AutoTest': case 'HasEnded': case 'OKEnd': return []
    case 'Continue': return [{ kind: 'continue' }]
    case 'Previous': return [{ kind: 'previous' }]
    case 'Choice': case 'ChoiceExit': return [choose(argument)]
    case 'Exit': case 'TRIGExitAll': return [{ kind: 'exitAll' }]
    case 'APIt1': return [choose('API Implementation Test 1')]
    case 'APIt2': return [choose('API Implementation Test 2')]
    case 'DMIt1': case 'DMIt2': case 'DMIt3': case 'DMIt4': return [choose(`Data Model Implementation Test ${name.slice(-1)}`)]
    case 'DMBx': return [{ kind: 'relaunch' }]
    case 'DMBxAct': return [{ kind: 'relaunch' }, choose('Data Model Behavior Test 1')]
    case 'RelaunchCM10': return [{ kind: 'relaunch' }, choose('Activity 4')]
    case 'RelaunchCM11': case 'ExitCM09ba': return [{ kind: 'exitAll' }, { kind: 'relaunch' }]
    case 'SuspendCM09ca': return [{ kind: 'suspendAll' }, { kind: 'relaunch' }]
    case 'ContinueExitCM8': return [{ kind: 'continue' }, { kind: 'relaunch' }]
    case 'RootSX4b': return [choose('the activity that corresponds to the root')]
    case 'exitlaunch': case 'LaunchDDMb':
      return [{ kind: 'unsupported', why: 'the case continues in another package, and each package keeps its own data' }]
    default:
      // "Relaunch the package after you log out" and "the attempt should
      // end; relaunch the package".
      if (/^re-?launch|^autoendrelaunch/i.test(name)) return [{ kind: 'relaunch' }]
      throw new Error(`unknown instruction ${instruction}`)
  }
}

async function act(page: Page, action: Action, ids: Map<string, string>): Promise<string | null> {
  const click = async (name: string) => {
    const button = page.locator(`[data-package-nav="${name}"]`)
    if (!(await button.waitFor({ timeout: 5000 }).then(() => true, () => false))) return `the player shows no ${name} control`
    if (!(await expect(button).toBeEnabled({ timeout: 3000 }).then(() => true, () => false))) return `the player's ${name} control is disabled`
    await button.click()
    return null
  }
  switch (action.kind) {
    case 'continue': return click('continue')
    case 'previous': return click('previous')
    case 'suspendAll': return click('suspend')
    case 'exitAll': {
      const refused = await click('exit')
      if (refused) return refused
      await page.locator('[data-package-exit-confirm]').click()
      return null
    }
    case 'choice': {
      const id = ids.get(action.title)
      const entry = page.locator(`[data-package-toc] [data-activity="${id}"]`)
      if (!id || !(await entry.waitFor({ timeout: 5000 }).then(() => true, () => false))) return `the table of contents has no entry for ${action.title}`
      if (!(await expect(entry).toBeEnabled({ timeout: 3000 }).then(() => true, () => false))) return `the table of contents entry for ${action.title} is disabled`
      await entry.click()
      return null
    }
    case 'relaunch':
      await navigate(page, '/elsewhere')
      await expect(page.getByText('Elsewhere')).toBeVisible()
      await navigate(page, '/learn/course-safety')
      await expect(page.locator('[data-package-player]')).toBeVisible()
      return null
    case 'unsupported': return action.why
  }
}

/**
 * Replays one of the suite's LMS test cases in the player and returns every
 * way it failed the suite's evaluation, in order; empty when it passes.
 * A wrong delivery or an instruction the player cannot carry out stops it.
 */
async function replay(page: Page, test: AdlCase): Promise<string[]> {
  const packages = test.id === 'DDM' ? ['DDMa', 'DDMb'] : [test.id]
  const description = DESCRIPTIONS[packages[0]!]!
  const ids = titles(description.tree)
  const failures: string[] = []
  await openPlayer(page, { adl: packages[0]! })

  let previous: string | null = null
  const start = test.instructions.start ?? '.'
  for (let index = 0; index < test.visits.length; index++) {
    const visit: AdlVisit = test.visits[index]!
    let settled = await settle(page, previous)
    if (index === 0 && start !== '.' && (settled.kind !== 'lesson' || settled.act !== visit.act)) {
      // "Start the package by choosing <start> from the table of contents."
      const refused = await act(page, { kind: 'choice', title: start }, ids)
      if (refused) return [...failures, `${visit.key}: ${refused}`]
      settled = await settle(page, settled.kind === 'lesson' ? settled.load : previous)
    }
    if (settled.kind !== 'lesson') return [...failures, `${visit.key}: expected activity ${visit.act}, the player delivered nothing (${settled.phase})`]
    if (settled.act !== visit.act) return [...failures, `${visit.key}: expected activity ${visit.act}, the player delivered activity ${settled.act}`]
    previous = settled.load

    if (visit.questions.length) {
      const given: string[] = []
      for (const question of visit.questions) given.push(await answer(page, question))
      visit.questions.forEach((question, at) => {
        if (given[at] !== visit.answers[at]) failures.push(`${visit.key}: ${question}: answered ${given[at]}, expected ${visit.answers[at]}`)
      })
    }
    const results = await settled.frame.evaluate(carryOut, visit.steps.map(step => ({ method: step.method, param: step.param })))
    visit.steps.forEach((step, at) => {
      const failed = evaluate(step, results[at]!)
      if (failed) failures.push(`${visit.key}: ${failed}`)
    })
    // The suite's lesson always terminates when it is done; the wrapper then
    // takes its frame away (and the engine may deliver the next activity).
    const terminated = await settled.frame.evaluate(() => (window as unknown as ProbeWindow).probe.api.Terminate!('')).catch(() => 'gone')
    let moved = false
    if (terminated === 'true') {
      const done = previous
      await expect.poll(async () => (await lessons(page)).every(lesson => lesson.load !== done), { timeout: 15_000 }).toBe(true)
      // The lesson shows its instruction and terminates at once, so when its
      // own navigation request already delivered something or ended the
      // session, the tester has nothing to navigate; a relaunch still
      // happens.
      const after = await settle(page, done, 'end', 1500)
      moved = after.kind === 'lesson' || after.phase === 'ended'
    }

    let relaunched = false
    for (const action of actions(visit.instruction)) {
      if (action.kind === 'relaunch') relaunched = true
      if (moved && !relaunched) continue
      const refused = await act(page, action, ids)
      if (refused) return [...failures, `${visit.key}: ${visit.instruction}: ${refused}`]
      if (action.kind === 'relaunch') previous = null
    }
  }
  // The case is over: the attempt on the activity tree has ended and nothing
  // more is delivered.
  const last = await settle(page, previous, 'end')
  if (last.kind === 'lesson') failures.push(`after the last activity the player delivered activity ${last.act}`)
  else if (last.phase !== 'ended') failures.push(`after the last activity the player shows ${last.phase}, not an ended session`)
  return failures
}

const ADL_CASES = Object.keys(DESCRIPTIONS)
  .map(id => (id === 'DDMa' ? 'DDM' : id)).filter(id => id !== 'DDMb')

/**
 * The cases the player does not pass yet, each with why and every failure
 * pinned, so a fix (or a new failure) shows: `adl-known-failures.json`. A
 * case listed there passes only while it fails in exactly the pinned way.
 * Rewrite the file with `ADL_RECORD=<file> npx playwright test
 * scorm2004-conformance -g "ADL LMS"`, which writes each case's failures
 * there as JSON lines.
 */
interface KnownFailure { row: string, why: string, failures: string[] }
const KNOWN = JSON.parse(readFileSync(new URL('./scorm2004-conformance/adl-known-failures.json', import.meta.url), 'utf8')) as Record<string, KnownFailure>

for (const id of ADL_CASES) {
  test(`ADL LMS test package ${id}`, async ({ page, browserName }) => {
    test.setTimeout(120_000)
    const failures = await replay(page, adlCase(id))
    if (process.env.ADL_RECORD) appendFileSync(process.env.ADL_RECORD, `${JSON.stringify({ id, browserName, failures })}\n`)
    expect(failures).toEqual(KNOWN[id]?.failures ?? [])
  })
}

// ---- Purpose-built packages ------------------------------------------------

type Call = [string, ...string[]]
interface ProbeRun { probe: { run: (calls: Call[]) => { value: string, error: string }[] } }

/** Runs the calls in the probe lesson and returns `value/error` per call. */
async function answers(frame: Frame, calls: Call[]): Promise<string[]> {
  const results = await frame.evaluate(list => (window as unknown as ProbeRun).probe.run(list), calls)
  return results.map(result => `${result.value}/${result.error}`)
}
const get = (element: string): Call => ['GetValue', element]
const set = (element: string, value: string): Call => ['SetValue', element, value]

/** Seconds in an ISO 8601 duration as the runtime writes one. */
function seconds(text: string): number {
  const match = /^P(?:(\d+)D)?(?:T(?:(\d+)H)?(?:(\d+)M)?(?:(\d+(?:\.\d+)?)S)?)?$/.exec(text)
  if (!match) throw new Error(`not a duration: ${text}`)
  return Number(match[1] ?? 0) * 86400 + Number(match[2] ?? 0) * 3600 + Number(match[3] ?? 0) * 60 + Number(match[4] ?? 0)
}

/** Opens a lesson of the probe package (or another 2004 fixture) from the table of contents. */
async function probeLesson(page: Page, id: 'probe' | 'plain' | 'noid'): Promise<Frame> {
  return openLesson(page, id, id === 'probe' ? 'probe.html' : `probe.html?${id}`)
}

/** A commit interval for checks that nothing more is written: three of them pass quickly. */
const SHORT_COMMIT_INTERVAL_MS = 200

const lastCommit = async (page: Page, activity: string) =>
  (await commits(page)).filter(commit => commit.activity_id === activity).slice(-1)[0]

test('host values: a lesson reads the learner, credit, mode, entry and every value its manifest gives it', async ({ page }) => {
  await openPlayer(page, { fixture: 'scorm2004-conformance' })
  const frame = await probeLesson(page, 'probe')
  expect(await answers(frame, [
    ['Initialize', ''],
    get('cmi.learner_id'), get('cmi.learner_name'), get('cmi.credit'), get('cmi.mode'), get('cmi.entry'),
    get('cmi.launch_data'), get('cmi.scaled_passing_score'), get('cmi.time_limit_action'), get('cmi.completion_threshold'),
  ])).toEqual([
    'true/0', `${LEARNER.id}/0`, `${LEARNER.name}/0`, 'credit/0', 'normal/0', 'ab-initio/0',
    'chapter=3&mode=practice/0', '0.7/0', 'exit,message/0', '0.6/0',
  ])
  const [maxTime, total] = await answers(frame, [get('cmi.max_time_allowed'), get('cmi.total_time')])
  expect(seconds(maxTime!.split('/')[0]!)).toBe(30 * 60)
  expect(seconds(total!.split('/')[0]!)).toBe(0)
  // The manifest's objectives with an identifier are the lesson's objectives.
  const [count, first, second] = await answers(frame, [get('cmi.objectives._count'), get('cmi.objectives.0.id'), get('cmi.objectives.1.id')])
  expect(count).toBe('2/0')
  expect([first, second].sort()).toEqual(['probe-extra/0', 'probe-pass/0'])
  for (const element of ['cmi.learner_id', 'cmi.learner_name', 'cmi.credit', 'cmi.mode', 'cmi.entry', 'cmi.launch_data',
    'cmi.scaled_passing_score', 'cmi.max_time_allowed', 'cmi.time_limit_action', 'cmi.completion_threshold', 'cmi.total_time']) {
    expect(await answers(frame, [set(element, 'x')]), element).toEqual(['false/404'])
  }
})

test('host values: a lesson its manifest gives nothing reads nothing, not the values of the lesson before it', async ({ page }) => {
  await openPlayer(page, { fixture: 'scorm2004-conformance' })
  const before = await probeLesson(page, 'probe')
  await answers(before, [['Initialize', ''], set('cmi.comments_from_learner.0.comment', 'from the probe'), ['Terminate', '']])
  const frame = await probeLesson(page, 'plain')
  expect(await answers(frame, [
    ['Initialize', ''],
    get('cmi.launch_data'), get('cmi.scaled_passing_score'), get('cmi.max_time_allowed'), get('cmi.time_limit_action'),
    get('cmi.objectives._count'), get('cmi.comments_from_learner._count'),
  ])).toEqual(['true/0', '/0', '/403', '/403', 'continue,no message/0', '0/0', '0/0'])
})

test('the host receives the whole run-time state on commit and on terminate', async ({ page }) => {
  await openPlayer(page, { fixture: 'scorm2004-conformance' })
  const frame = await probeLesson(page, 'probe')
  await answers(frame, [
    ['Initialize', ''],
    set('cmi.location', 'page-2'),
    set('cmi.suspend_data', 'state'),
    set('cmi.interactions.0.id', 'q-1'),
    set('cmi.interactions.0.type', 'true-false'),
    set('cmi.objectives.0.success_status', 'passed'),
    ['Commit', ''],
  ])
  await expect.poll(async () => (await lastCommit(page, 'probe'))?.runtime.cmi, { timeout: 5000 })
    .toMatchObject({ location: 'page-2', suspend_data: 'state' })
  expect((await lastCommit(page, 'probe'))?.session_ended).toBe(false)
  await answers(frame, [set('cmi.session_time', 'PT2M'), ['Terminate', '']])
  await expect.poll(async () => (await lastCommit(page, 'probe'))?.session_ended).toBe(true)
  const runtime = JSON.stringify((await lastCommit(page, 'probe'))?.runtime)
  expect(runtime).toContain('q-1')
  expect(runtime).toContain('PT2M')
})

test('API: after Terminate every call but the error functions fails with the after-termination codes and nothing reaches the host', async ({ page }) => {
  // A short commit interval, so waiting out a few of them is quick.
  await openPlayer(page, { fixture: 'scorm2004-conformance', commitIntervalMs: SHORT_COMMIT_INTERVAL_MS })
  const frame = await probeLesson(page, 'probe')
  // The lesson's frame goes once it terminates; a late call (from its unload
  // handler, or a page that kept the object) reaches the API it was given.
  const wrapper = wrapperFrame(page)
  await wrapper.evaluate(() => { (window as unknown as { held: unknown }).held = (window as unknown as { API_1484_11: unknown }).API_1484_11 })
  expect(await answers(frame, [['Initialize', ''], set('cmi.location', 'before'), ['Terminate', '']])).toEqual(['true/0', 'true/0', 'true/0'])
  await expect.poll(async () => (await lastCommit(page, 'probe'))?.session_ended).toBe(true)
  const writes = (await commits(page)).length
  const late = await wrapper.evaluate((calls) => {
    const api = (window as unknown as { held: Record<string, (...args: string[]) => string> }).held
    return calls.map(([name, ...args]) => `${api[name!]!(...args)}/${api.GetLastError!()}`)
  }, [get('cmi.location'), set('cmi.location', 'after'), ['Commit', ''], ['Terminate', ''], ['Initialize', '']] as Call[])
  expect(late).toEqual(['/123', 'false/133', 'false/143', 'false/113', 'false/104'])
  await page.waitForTimeout(3 * SHORT_COMMIT_INTERVAL_MS)
  expect((await commits(page)).length).toBe(writes)
})

test('API: a successful read of completion_status or success_status reports error 0', async ({ page }) => {
  await openPlayer(page, { fixture: 'scorm2004-conformance' })
  const frame = await probeLesson(page, 'probe')
  expect(await answers(frame, [
    ['Initialize', ''],
    set('cmi.score.scaled', 'high'), get('cmi.completion_status'),
    set('cmi.score.scaled', 'high'), get('cmi.success_status'),
  ])).toEqual(['true/0', 'false/406', 'unknown/0', 'false/406', 'unknown/0'])
})

test('a lesson left without terminating still hands over its state when the player closes', async ({ page }) => {
  await openPlayer(page, { fixture: 'scorm2004-conformance' })
  const frame = await probeLesson(page, 'probe')
  await answers(frame, [['Initialize', ''], set('cmi.location', 'page-9')])
  await navigate(page, '/elsewhere')
  await expect(page.getByText('Elsewhere')).toBeVisible()
  await expect.poll(async () => (await lastCommit(page, 'probe'))?.session_ended).toBe(true)
  expect((await lastCommit(page, 'probe'))?.runtime.cmi).toMatchObject({ location: 'page-9' })
  // The engine's state went to the host too, so the attempt resumes.
  expect((await savedStates(page)).length).toBeGreaterThan(0)
})

test('a suspended lesson resumes with its data; a lesson that ends without suspend starts a new attempt with none', async ({ page }) => {
  await openPlayer(page, { fixture: 'scorm2004-conformance' })
  let frame = await probeLesson(page, 'probe')
  await answers(frame, [['Initialize', ''], set('cmi.location', 'page-4'), set('cmi.exit', 'suspend'), set('cmi.session_time', 'PT1M30S'), ['Terminate', '']])
  await probeLesson(page, 'plain')
  frame = await probeLesson(page, 'probe')
  expect(await answers(frame, [['Initialize', ''], get('cmi.entry'), get('cmi.location')])).toEqual(['true/0', 'resume/0', 'page-4/0'])
  const [total] = await answers(frame, [get('cmi.total_time')])
  expect(seconds(total!.split('/')[0]!)).toBe(90)
  // Ended normally: the learner attempt on the lesson is over.
  await answers(frame, [set('cmi.exit', 'normal'), ['Terminate', '']])
  await probeLesson(page, 'plain')
  frame = await probeLesson(page, 'probe')
  expect(await answers(frame, [['Initialize', ''], get('cmi.entry'), get('cmi.location')])).toEqual(['true/0', 'ab-initio/0', '/403'])
})

test('a primary objective without an identifier still sets the passing score and is not among the lesson objectives', async ({ page }) => {
  await openPlayer(page, { fixture: 'scorm2004-conformance' })
  const frame = await probeLesson(page, 'noid')
  expect(await answers(frame, [
    ['Initialize', ''], get('cmi.scaled_passing_score'), get('cmi.objectives._count'),
    set('cmi.score.scaled', '0.9'), get('cmi.success_status'),
  ])).toEqual(['true/0', '0.5/0', '0/0', 'true/0', 'passed/0'])
})

test('known limit: a search for the SCORM 1.2 API from a 2004 lesson climbs past the wrapper and is refused by the app window', async ({ page }) => {
  await openPlayer(page, { fixture: 'scorm2004-conformance' })
  const frame = await probeLesson(page, 'probe')
  const outcome = await frame.evaluate(() => {
    let win: Window = window
    try {
      for (let tries = 0; tries < 8 && !(win as unknown as Record<string, unknown>).API && win.parent !== win; tries++) win = win.parent
      return 'finished without an error'
    } catch (error) {
      return (error as Error).name
    }
  })
  expect(outcome).toBe('SecurityError')
  expect(await wrapperFrame(page).evaluate(() => typeof (window as unknown as Record<string, unknown>).API_1484_11)).toBe('object')
})

/** Runs an edition fixture: the first lesson passes, the second completes, the course completes. */
async function runEdition(page: Page, fixture: 'scorm2004-edition2' | 'scorm2004-edition3', edition: string) {
  await openPlayer(page, { fixture })
  const first = await openLesson(page, 'first', 'probe.html?first')
  await expect(page.locator('[data-package-toc] [data-activity="second"]')).toBeDisabled()
  expect(await answers(first, [['Initialize', ''], get('cmi.launch_data'), get('cmi.scaled_passing_score')]))
    .toEqual(['true/0', `edition=${edition}/0`, '0.8/0'])
  await answers(first, [set('cmi.score.scaled', '0.9'), ['Terminate', '']])
  await expect(page.locator('[data-package-nav="continue"]')).toBeEnabled()
  await page.locator('[data-package-nav="continue"]').click()
  const second = await lessonFrame(page, 'probe.html?second')
  await expect(second.locator('#seen[data-ready]')).toBeAttached()
  return second
}

test('a SCORM 2004 2nd Edition package runs on the same runtime: delivery, a rule on a global objective, completion', async ({ page }) => {
  const second = await runEdition(page, 'scorm2004-edition2', '2')
  expect(await answers(second, [['Initialize', ''], get('cmi.entry'), set('cmi.completion_status', 'completed'), ['Terminate', '']]))
    .toEqual(['true/0', 'ab-initio/0', 'true/0', 'true/0'])
  await page.locator('[data-package-nav="continue"]').click()
  await expect(page.locator('[data-package-sequencing="ended"]')).toBeVisible()
  await expect.poll(async () => (await savedStates(page)).slice(-1)[0]?.root.completion_status).toBe('completed')
})

test('a SCORM 2004 3rd Edition package runs on the same runtime: its completion threshold completes a lesson by progress', async ({ page }) => {
  const second = await runEdition(page, 'scorm2004-edition3', '3')
  expect(await answers(second, [
    ['Initialize', ''], get('cmi.completion_threshold'), set('cmi.progress_measure', '0.8'), get('cmi.completion_status'), ['Terminate', ''],
  ])).toEqual(['true/0', '0.75/0', 'true/0', 'completed/0', 'true/0'])
  await page.locator('[data-package-nav="continue"]').click()
  await expect(page.locator('[data-package-sequencing="ended"]')).toBeVisible()
  await expect.poll(async () => (await savedStates(page)).slice(-1)[0]?.root.completion_status).toBe('completed')
})

test('preview runs a SCORM 2004 package under its sequencing rules through the preview commands only', async ({ page }) => {
  const { refused } = await servePackageOrigin(page, 'scorm2004-edition3')
  await page.goto('/tests/desktop-ui/?screen=course-editor&package=scorm2004-edition3')
  await page.locator('[data-outline-element]', { hasText: 'Workshop safety' }).click()
  await page.locator('[data-package-preview]').click()
  const player = page.locator('[data-package-preview-frame] [data-package-player]')
  await expect(player.locator('[data-package-toc] [data-activity]')).toHaveText([/First lesson/, /Second lesson/])
  await expect(player.locator('[data-package-toc] [data-activity="second"]')).toBeDisabled()

  const first = await lessonFrame(page, 'probe.html?first')
  await expect(first.locator('#seen[data-ready]')).toBeAttached()
  await answers(first, [['Initialize', ''], set('cmi.score.scaled', '0.9'), ['Terminate', '']])
  await player.locator('[data-package-nav="continue"]').click()
  const second = await lessonFrame(page, 'probe.html?second')
  await expect(second.locator('#seen[data-ready]')).toBeAttached()
  expect(await answers(second, [['Initialize', ''], get('cmi.entry')])).toEqual(['true/0', 'ab-initio/0'])

  const editorCalls = (command: string) => page.evaluate(name =>
    (window as unknown as { courseEditorFixture: { called: (command: string) => unknown[] } }).courseEditorFixture.called(name).length, command)
  await expect.poll(() => editorCalls('package_preview_sequencing_save')).toBeGreaterThan(0)
  await page.locator('[data-package-preview]').click()
  await expect(player).toHaveCount(0)
  await expect.poll(() => editorCalls('package_preview_terminate')).toBe(1)
  for (const command of ['package_attempt_open', 'package_attempt_initialise', 'package_attempt_commit',
    'package_attempt_sequencing', 'package_attempt_sequencing_save', 'package_attempt_terminate', 'update_progress']) {
    expect(await editorCalls(command), command).toBe(0)
  }
  expect(refused).toEqual([])
})

test('API: before Initialize every call but the error functions fails with its own code; Initialize takes only an empty argument, once', async ({ page }) => {
  await openPlayer(page, { fixture: 'scorm2004-conformance' })
  const frame = await probeLesson(page, 'probe')
  expect(await answers(frame, [
    ['Terminate', ''], get('cmi.location'), set('cmi.location', 'x'), ['Commit', ''],
    ['Initialize', 'x'], ['Initialize', ''], ['Initialize', ''],
  ])).toEqual(['false/112', '/122', 'false/132', 'false/142', 'false/201', 'true/0', 'false/103'])
})

test('known deviation: adl.nav.request_valid.continue answers true where the next activity is disabled', async ({ page }) => {
  await openPlayer(page, { fixture: 'scorm2004-edition2' })
  const first = await openLesson(page, 'first', 'probe.html?first')
  expect(await answers(first, [['Initialize', ''], get('adl.nav.request_valid.continue')])).toEqual(['true/0', 'true/0'])
  // The player's own control follows the engine's look-ahead instead.
  await expect(page.locator('[data-package-nav="continue"]')).toBeDisabled()
})

test('data model: a group name such as cmi or cmi.objectives is no element (401) and setting it changes nothing', async ({ page }) => {
  await openPlayer(page, { fixture: 'scorm2004-conformance' })
  const frame = await probeLesson(page, 'probe')
  expect(await answers(frame, [
    ['Initialize', ''],
    set('cmi', 'x'), set('cmi.objectives', 'x'), set('adl.data', 'x'), get('cmi.objectives'),
    get('cmi.objectives._count'), set('cmi.location', 'still-here'), get('cmi.location'),
  ])).toEqual(['true/0', 'false/401', 'false/401', 'false/401', '/401', '2/0', 'true/0', 'still-here/0'])
})

// ---- The matrix ------------------------------------------------------------

test('every row of the conformance matrix names a test that exists', async ({ browserName }) => {
  test.skip(browserName !== 'chromium', 'reads files only; once is enough')
  const matrix = await readFile(new URL('./scorm2004-conformance.md', import.meta.url), 'utf8')
  const browser = (await Promise.all(['./scorm2004-conformance.spec.ts', './package-player-2004.spec.ts', './package-isolation.spec.ts', './scorm12-conformance.spec.ts']
    .map(path => readFile(new URL(path, import.meta.url), 'utf8')))).join('\n')
  // Content aggregation is tested at the host seam: rows may name a Rust test function.
  const rust = (await Promise.all(['scorm2004_import.rs', 'scorm2004_conformance_packages.rs', 'package_import.rs']
    .map(name => readFile(new URL(`../../crates/alexandria-studio/tests/${name}`, import.meta.url), 'utf8')))).join('\n')
  const lines = matrix.split('\n')
  const rows = lines.filter((line, index) => line.startsWith('| ') && !lines[index + 1]?.startsWith('|---'))
  expect(rows.length).toBeGreaterThan(40)
  const listed = new Set<string>()
  for (const row of rows) {
    const named = [...row.matchAll(/«([^»]+)»/g)].map(match => match[1]!)
    expect(named.length, row).toBeGreaterThan(0)
    for (const title of named) {
      const literal = title.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
      const found = new RegExp(`test\\((['\`])${literal}`).test(browser) || new RegExp(`fn ${literal}\\(`).test(rust)
      expect(found, title).toBe(true)
    }
    // A row naming the test suite's replay lists its cases in backticks:
    // each must exist, and a row with a pinned case says it is known.
    if (!named.includes('ADL LMS test package ')) continue
    const cases = [...row.matchAll(/`([A-Z]{1,3}-?[0-9A-Za-z-]*)`/g)].map(match => match[1]!).filter(id => /^(API|DDM|DMB|DMI|[A-Z]{1,2}-\w+)$/.test(id))
    expect(cases.length, row).toBeGreaterThan(0)
    const status = row.split(' | ')[2] ?? ''
    for (const id of cases) {
      expect(ADL_CASES, `${id} in: ${row.slice(0, 80)}`).toContain(id)
      expect(status.startsWith('known'), `${id}: ${status}`).toBe(Boolean(KNOWN[id]))
      listed.add(id)
    }
  }
  expect(ADL_CASES.filter(id => !listed.has(id)), 'test suite cases no row lists').toEqual([])
})
