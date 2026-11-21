import { readFile } from 'node:fs/promises'
import { expect, test, type Frame, type Page } from '@playwright/test'
import {
  commits, commitTimes, handlerLimits, navigate, openLesson, openPlayer, silentWav, wrapperFrame,
} from './packageHarness'

// SCORM 1.2 run-time conformance at the player seam: the real player,
// wrapper and vendored runtime, a probe lesson (`scorm12-conformance/`) that
// the tests drive call by call, and a fake host. Each test is a row (or
// several) of `scorm12-conformance.md`; expected values come from the SCORM
// 1.2 run-time environment and the standards body's 1.2 test suite
// validators, not from the library. Where the library departs from them on
// purpose, the test pins what it does and the matrix marks the row a known
// deviation.

type Call = [string, ...string[]]
interface Result { call: Call, value: string, error: string }

interface ProbeWindow { probe: { run: (calls: Call[]) => Result[] } }

async function run(frame: Frame, calls: Call[]): Promise<Result[]> {
  return frame.evaluate(list => (window as unknown as ProbeWindow).probe.run(list), calls)
}

/** Runs the calls and returns `value/error` per call, for compact assertions. */
async function answers(frame: Frame, calls: Call[]): Promise<string[]> {
  return (await run(frame, calls)).map(result => `${result.value}/${result.error}`)
}

const opened = new WeakSet<Page>()

/** A commit interval for checks that nothing more is written: three of them pass quickly. */
const SHORT_COMMIT_INTERVAL_MS = 200

/**
 * Opens a probe lesson before LMSInitialize: `probe` carries every value a
 * manifest can give a lesson, `plain` none. The player opens once per page.
 */
async function probe(page: Page, lessonId: 'probe' | 'plain' = 'probe'): Promise<Frame> {
  if (!opened.has(page)) {
    await openPlayer(page, { fixture: 'scorm12-conformance' })
    opened.add(page)
  }
  return openLesson(page, lessonId, lessonId === 'probe' ? 'probe.html' : 'probe.html?plain')
}

async function started(page: Page, lessonId: 'probe' | 'plain' = 'probe'): Promise<Frame> {
  const frame = await probe(page, lessonId)
  expect(await answers(frame, [['LMSInitialize', '']])).toEqual(['true/0'])
  return frame
}

const set = (element: string, value: string): Call => ['LMSSetValue', element, value]

/** Seconds in a CMITimespan (`HHHH:MM:SS.SS`, hours of two to four digits), or null if it is not one. */
function seconds(span: string): number | null {
  const match = /^(\d{2,4}):(\d{2}):(\d{2})(\.\d{1,2})?$/.exec(span)
  return match ? Number(match[1]) * 3600 + Number(match[2]) * 60 + Number(match[3]) + Number(match[4] ?? 0) : null
}
const get = (element: string): Call => ['LMSGetValue', element]
const sorted = (list: string) => list.split(',').sort()

const lastCommit = async (page: Page, activity = 'probe') =>
  (await commits(page)).filter(commit => commit.activity_id === activity).slice(-1)[0]

// API calls and error codes

test('API: the lesson finds the API by the published discovery algorithm', async ({ page }) => {
  const frame = await probe(page)
  expect(await frame.evaluate(() => typeof (window as unknown as ProbeWindow).probe.run)).toBe('function')
  expect(await frame.locator('#seen').getAttribute('data-values')).toBe('{"found":true}')
})

test('API: LMSInitialize starts a session once; an argument is refused with 201 and a second call with 101', async ({ page }) => {
  const frame = await probe(page)
  expect(await answers(frame, [
    ['LMSInitialize', 'x'],
    ['LMSInitialize', ''],
    ['LMSInitialize', ''],
  ])).toEqual(['false/201', 'true/0', 'false/101'])
})

test('API: every call but the error functions fails with 301 before LMSInitialize', async ({ page }) => {
  const frame = await probe(page)
  expect(await answers(frame, [
    get('cmi.core.student_id'),
    set('cmi.core.lesson_location', 'x'),
    ['LMSCommit', ''],
    ['LMSFinish', ''],
  ])).toEqual(['/301', 'false/301', 'false/301', 'false/301'])
})

test('API: LMSCommit and LMSFinish take only an empty argument (201)', async ({ page }) => {
  const frame = await started(page)
  expect(await answers(frame, [['LMSCommit', 'x'], ['LMSFinish', 'x'], ['LMSCommit', '']])).toEqual(['false/201', 'false/201', 'true/0'])
})

test('API: after LMSFinish every call but the error functions fails with 301 and nothing more reaches the host', async ({ page }) => {
  // A short commit interval, so waiting out a few of them is quick.
  await openPlayer(page, { fixture: 'scorm12-conformance', commitIntervalMs: SHORT_COMMIT_INTERVAL_MS })
  opened.add(page)
  const frame = await started(page)
  expect(await answers(frame, [set('cmi.core.lesson_location', 'before'), ['LMSFinish', '']])).toEqual(['true/0', 'true/0'])
  await expect.poll(async () => (await lastCommit(page))?.session_ended).toBe(true)
  const writes = (await commits(page)).length
  expect(await answers(frame, [
    get('cmi.core.lesson_location'),
    set('cmi.core.lesson_location', 'after'),
    ['LMSCommit', ''],
    ['LMSFinish', ''],
  ])).toEqual(['/301', 'false/301', 'false/301', 'false/301'])
  const [description] = await run(frame, [['LMSGetErrorString', '301']])
  expect(description?.value).toMatch(/not initiali[sz]ed/i)
  await page.waitForTimeout(3 * SHORT_COMMIT_INTERVAL_MS)
  expect((await commits(page)).length).toBe(writes)
})

test('API: LMSGetLastError reports the last call and LMSGetErrorString and LMSGetDiagnostic describe every code', async ({ page }) => {
  const frame = await started(page)
  expect(await answers(frame, [set('cmi.core.student_id', 'x'), ['LMSGetLastError']])).toEqual(['false/403', '403/403'])
  expect(await answers(frame, [get('cmi.core.lesson_location'), ['LMSGetLastError']])).toEqual(['/0', '0/0'])
  const words: Record<string, RegExp> = {
    0: /no error/i, 101: /general/i, 201: /invalid argument/i, 202: /children/i, 203: /count|array/i,
    301: /not initiali[sz]ed/i, 401: /not implemented/i, 402: /keyword/i, 403: /read only/i,
    404: /write only/i, 405: /data type/i,
  }
  for (const [code, pattern] of Object.entries(words)) {
    const [text] = await run(frame, [['LMSGetErrorString', code]])
    expect(text?.value, code).toMatch(pattern)
  }
  const [, diagnostic] = await run(frame, [set('cmi.core.score.raw', 'abc'), ['LMSGetDiagnostic', '405']])
  expect(diagnostic?.value.length).toBeGreaterThan(0)
})

test('known deviation: LMSGetDiagnostic still describes an earlier error after a later call succeeds', async ({ page }) => {
  const frame = await started(page)
  expect(await answers(frame, [set('cmi.core.score.raw', 'abc'), get('cmi.core.lesson_location'), ['LMSGetDiagnostic', '']]))
    .toEqual(['false/405', '/0', 'Incorrect Data Type/0'])
})

// cmi.core

test('core: the host-supplied elements read back and refuse writes with 403', async ({ page }) => {
  const frame = await started(page)
  const host: Record<string, string> = {
    'cmi.core.student_id': 'learner-for-this-package',
    'cmi.core.student_name': 'Ada Learner',
    'cmi.core.credit': 'credit',
    'cmi.core.entry': 'ab-initio',
    'cmi.core.lesson_mode': 'normal',
    'cmi.core.lesson_status': 'not attempted',
  }
  for (const [element, value] of Object.entries(host)) {
    expect(await answers(frame, [get(element)]), element).toEqual([`${value}/0`])
  }
  // A timespan, read in the library's own spelling (`00:00:00`).
  const [total] = await run(frame, [get('cmi.core.total_time')])
  expect(seconds(total!.value)).toBe(0)
  for (const element of [...Object.keys(host).filter(name => name !== 'cmi.core.lesson_status'), 'cmi.core.total_time']) {
    expect(await answers(frame, [set(element, 'x')]), element).toEqual(['false/403'])
  }
})

test('core: _children lists the standard elements and a keyword cannot be set (402)', async ({ page }) => {
  const frame = await started(page)
  const [core, score] = await run(frame, [get('cmi.core._children'), get('cmi.core.score._children')])
  expect(sorted(core!.value)).toEqual(sorted('student_id,student_name,lesson_location,credit,lesson_status,entry,score,total_time,lesson_mode,exit,session_time'))
  expect(sorted(score!.value)).toEqual(['max', 'min', 'raw'])
  expect(await answers(frame, [set('cmi.core._children', 'x'), set('cmi.core.score._children', 'x')])).toEqual(['false/402', 'false/402'])
})

test('core: _count on an element that is not a list fails with 203 and _children on a leaf with 202', async ({ page }) => {
  const frame = await started(page)
  expect(await answers(frame, [get('cmi.core._count'), get('cmi.core.student_id._children')])).toEqual(['/203', '/202'])
})

test('core: lesson_location keeps up to 255 characters and refuses more (405)', async ({ page }) => {
  const frame = await started(page)
  expect(await answers(frame, [set('cmi.core.lesson_location', 'page-7'), get('cmi.core.lesson_location')])).toEqual(['true/0', 'page-7/0'])
  expect(await answers(frame, [set('cmi.core.lesson_location', 'a'.repeat(255)), set('cmi.core.lesson_location', 'a'.repeat(256))]))
    .toEqual(['true/0', 'false/405'])
})

test('core: lesson_status takes its vocabulary and refuses anything else, not attempted included (405)', async ({ page }) => {
  const frame = await started(page)
  for (const value of ['passed', 'completed', 'failed', 'incomplete', 'browsed']) {
    expect(await answers(frame, [set('cmi.core.lesson_status', value), get('cmi.core.lesson_status')]), value).toEqual(['true/0', `${value}/0`])
  }
  for (const value of ['not attempted', 'Passed', 'done', '']) {
    expect(await answers(frame, [set('cmi.core.lesson_status', value)]), value).toEqual(['false/405'])
  }
})

test('core: score raw, min and max take a number from 0 to 100 or blank and refuse anything else (405)', async ({ page }) => {
  const frame = await started(page)
  for (const element of ['cmi.core.score.raw', 'cmi.core.score.min', 'cmi.core.score.max']) {
    for (const value of ['0', '85', '100', '42.5']) {
      expect(await answers(frame, [set(element, value), get(element)]), `${element}=${value}`).toEqual(['true/0', `${value}/0`])
    }
    for (const value of ['150', '-1', 'abc']) {
      expect(await answers(frame, [set(element, value)]), `${element}=${value}`).toEqual(['false/405'])
    }
  }
  expect(await answers(frame, [set('cmi.core.score.raw', ''), get('cmi.core.score.raw')])).toEqual(['true/0', '/0'])
})

test('core: exit and session_time are write-only (404) and refuse values outside their formats (405)', async ({ page }) => {
  const frame = await started(page)
  for (const value of ['time-out', 'suspend', 'logout', '']) {
    expect(await answers(frame, [set('cmi.core.exit', value)]), value).toEqual(['true/0'])
  }
  expect(await answers(frame, [set('cmi.core.exit', 'quit'), get('cmi.core.exit')])).toEqual(['false/405', '/404'])
  expect(await answers(frame, [set('cmi.core.session_time', '0000:30:00'), set('cmi.core.session_time', '00:05:10.5')])).toEqual(['true/0', 'true/0'])
  expect(await answers(frame, [set('cmi.core.session_time', '30 minutes'), set('cmi.core.session_time', '00:30'), get('cmi.core.session_time')]))
    .toEqual(['false/405', 'false/405', '/404'])
})

test('known deviation: a timespan may have more than four digits of hours where the standard allows at most 9999', async ({ page }) => {
  const frame = await started(page)
  expect(await answers(frame, [set('cmi.core.session_time', '9999:00:00'), set('cmi.core.session_time', '10000:00:00')])).toEqual(['true/0', 'true/0'])
})

test('data model: an unknown group fails with 401', async ({ page }) => {
  const frame = await started(page)
  expect(await answers(frame, [get('cmi.bogus'), set('cmi.bogus.x', '1'), get('adl.nav.request')])).toEqual(['/401', 'false/401', '/401'])
})

test('known deviation: an unknown element in a known group fails with 401 where the 1.2 test suite expects 201', async ({ page }) => {
  const frame = await started(page)
  expect(await answers(frame, [get('cmi.core.bogus'), set('cmi.core.bogus', '1')])).toEqual(['/401', 'false/401'])
})

// cmi.suspend_data, cmi.launch_data, cmi.comments

test('suspend_data keeps 4096 characters and reads them back', async ({ page }) => {
  const frame = await started(page)
  const value = 'abc,'.repeat(1024)
  expect(await answers(frame, [set('cmi.suspend_data', value)])).toEqual(['true/0'])
  const [read] = await run(frame, [get('cmi.suspend_data')])
  expect(read).toMatchObject({ value, error: '0' })
})

test('known deviation: suspend_data accepts up to 64000 characters where the standard caps it at 4096', async ({ page }) => {
  const frame = await started(page)
  expect(await answers(frame, [set('cmi.suspend_data', 'x'.repeat(4097)), set('cmi.suspend_data', 'x'.repeat(64000)), set('cmi.suspend_data', 'x'.repeat(64001))]))
    .toEqual(['true/0', 'true/0', 'false/405'])
})

test('launch_data is the manifest data from the LMS, read-only, and empty without one', async ({ page }) => {
  const frame = await started(page)
  expect(await answers(frame, [get('cmi.launch_data'), set('cmi.launch_data', 'x')])).toEqual(['chapter=3&mode=practice/0', 'false/403'])
  const plain = await started(page, 'plain')
  expect(await answers(plain, [get('cmi.launch_data')])).toEqual(['/0'])
})

test('comments take up to 4096 characters and comments_from_lms is read-only', async ({ page }) => {
  const frame = await started(page)
  expect(await answers(frame, [set('cmi.comments', 'Clear and short.'), get('cmi.comments')])).toEqual(['true/0', 'Clear and short./0'])
  expect(await answers(frame, [set('cmi.comments', 'c'.repeat(4096)), set('cmi.comments', 'c'.repeat(4097))])).toEqual(['true/0', 'false/405'])
  expect(await answers(frame, [get('cmi.comments_from_lms'), set('cmi.comments_from_lms', 'x')])).toEqual(['/0', 'false/403'])
})

// cmi.objectives

test('objectives: every element round trips, is counted and validated', async ({ page }) => {
  const frame = await started(page)
  const [children, empty] = await run(frame, [get('cmi.objectives._children'), get('cmi.objectives._count')])
  expect(sorted(children!.value)).toEqual(['id', 'score', 'status'])
  expect(empty).toMatchObject({ value: '0', error: '0' })
  expect(await answers(frame, [
    set('cmi.objectives.0.id', 'obj-1'),
    set('cmi.objectives.0.score.raw', '80'),
    set('cmi.objectives.0.score.min', '0'),
    set('cmi.objectives.0.score.max', '100'),
    set('cmi.objectives.0.status', 'passed'),
    get('cmi.objectives.0.id'),
    get('cmi.objectives.0.score.raw'),
    get('cmi.objectives.0.score.min'),
    get('cmi.objectives.0.score.max'),
    get('cmi.objectives.0.status'),
    get('cmi.objectives._count'),
  ])).toEqual(['true/0', 'true/0', 'true/0', 'true/0', 'true/0', 'obj-1/0', '80/0', '0/0', '100/0', 'passed/0', '1/0'])
  const [scoreChildren] = await run(frame, [get('cmi.objectives.0.score._children')])
  expect(sorted(scoreChildren!.value)).toEqual(['max', 'min', 'raw'])
  expect(await answers(frame, [set('cmi.objectives.0.status', 'done'), set('cmi.objectives.0.score.raw', '101')])).toEqual(['false/405', 'false/405'])
  // A new entry is added at the end of the list, not past it.
  expect(await answers(frame, [set('cmi.objectives.5.id', 'obj-6')])).toEqual(['false/201'])
  expect(await answers(frame, [set('cmi.objectives._children', 'x'), set('cmi.objectives._count', '3')])).toEqual(['false/402', 'false/402'])
})

test('known deviation: an identifier with a space is accepted where the 1.2 test suite refuses it (405)', async ({ page }) => {
  const frame = await started(page)
  expect(await answers(frame, [set('cmi.objectives.0.id', 'obj 1'), set('cmi.interactions.0.id', 'q 1')])).toEqual(['true/0', 'true/0'])
})

// cmi.student_data, cmi.student_preference

test('student_data comes from the manifest and is read-only', async ({ page }) => {
  const frame = await started(page)
  const [children] = await run(frame, [get('cmi.student_data._children')])
  expect(sorted(children!.value)).toEqual(['mastery_score', 'max_time_allowed', 'time_limit_action'])
  const [mastery, maxTime, action] = await run(frame, [
    get('cmi.student_data.mastery_score'),
    get('cmi.student_data.max_time_allowed'),
    get('cmi.student_data.time_limit_action'),
  ])
  expect([mastery?.value, seconds(maxTime!.value), action?.value]).toEqual(['75', 30 * 60, 'exit,message'])
  for (const element of ['mastery_score', 'max_time_allowed', 'time_limit_action']) {
    expect(await answers(frame, [set(`cmi.student_data.${element}`, '1')]), element).toEqual(['false/403'])
  }
  const plain = await started(page, 'plain')
  expect(await answers(plain, [get('cmi.student_data.mastery_score'), get('cmi.student_data.max_time_allowed'), get('cmi.student_data.time_limit_action')]))
    .toEqual(['/0', '/0', '/0'])
})

test('student_preference: audio, language, speed and text round trip within their ranges', async ({ page }) => {
  const frame = await started(page)
  const [children] = await run(frame, [get('cmi.student_preference._children')])
  expect(sorted(children!.value)).toEqual(['audio', 'language', 'speed', 'text'])
  const good: [string, string][] = [
    ['audio', '-1'], ['audio', '0'], ['audio', '100'],
    ['language', 'hi'], ['speed', '-100'], ['speed', '100'], ['text', '-1'], ['text', '0'], ['text', '1'],
  ]
  for (const [element, value] of good) {
    expect(await answers(frame, [set(`cmi.student_preference.${element}`, value), get(`cmi.student_preference.${element}`)]), `${element}=${value}`)
      .toEqual(['true/0', `${value}/0`])
  }
  const bad: [string, string][] = [['audio', '101'], ['audio', '-2'], ['speed', '101'], ['text', '2'], ['audio', 'loud']]
  for (const [element, value] of bad) {
    expect(await answers(frame, [set(`cmi.student_preference.${element}`, value)]), `${element}=${value}`).toEqual(['false/405'])
  }
})

// cmi.interactions

test('interactions: every element is write-only, counted and validated', async ({ page }) => {
  const frame = await started(page)
  const [children] = await run(frame, [get('cmi.interactions._children')])
  expect(sorted(children!.value)).toEqual(sorted('id,objectives,time,type,correct_responses,weighting,student_response,result,latency'))
  expect(await answers(frame, [
    set('cmi.interactions.0.id', 'q-1'),
    set('cmi.interactions.0.objectives.0.id', 'obj-1'),
    set('cmi.interactions.0.time', '12:30:00'),
    set('cmi.interactions.0.type', 'choice'),
    set('cmi.interactions.0.correct_responses.0.pattern', 'a'),
    set('cmi.interactions.0.weighting', '1'),
    set('cmi.interactions.0.student_response', 'a'),
    set('cmi.interactions.0.result', 'correct'),
    set('cmi.interactions.0.latency', '0000:00:05'),
  ])).toEqual(Array(9).fill('true/0'))
  expect(await answers(frame, [
    get('cmi.interactions._count'),
    get('cmi.interactions.0.objectives._count'),
    get('cmi.interactions.0.correct_responses._count'),
  ])).toEqual(['1/0', '1/0', '1/0'])
  for (const element of ['id', 'objectives.0.id', 'time', 'type', 'correct_responses.0.pattern', 'weighting', 'student_response', 'result', 'latency']) {
    expect(await answers(frame, [get(`cmi.interactions.0.${element}`)]), element).toEqual(['/404'])
  }
  for (const value of ['true-false', 'choice', 'fill-in', 'matching', 'performance', 'likert', 'sequencing', 'numeric']) {
    expect(await answers(frame, [set('cmi.interactions.0.type', value)]), value).toEqual(['true/0'])
  }
  for (const value of ['correct', 'wrong', 'unanticipated', 'neutral', '0.5']) {
    expect(await answers(frame, [set('cmi.interactions.0.result', value)]), value).toEqual(['true/0'])
  }
  expect(await answers(frame, [
    set('cmi.interactions.0.type', 'essay'),
    set('cmi.interactions.0.result', 'right'),
    set('cmi.interactions.0.time', 'noon'),
    set('cmi.interactions.0.latency', '5 seconds'),
    set('cmi.interactions.0.weighting', 'heavy'),
    set('cmi.interactions.3.id', 'q-4'),
  ])).toEqual(['false/405', 'false/405', 'false/405', 'false/405', 'false/405', 'false/201'])
})

test('known deviation: interaction responses and patterns are not checked against the interaction type', async ({ page }) => {
  const frame = await started(page)
  expect(await answers(frame, [
    set('cmi.interactions.0.id', 'q-1'),
    set('cmi.interactions.0.type', 'true-false'),
    set('cmi.interactions.0.student_response', 'maybe'),
    set('cmi.interactions.0.correct_responses.0.pattern', 'maybe'),
  ])).toEqual(['true/0', 'true/0', 'true/0', 'true/0'])
})

// What the host receives, status rules and resuming

test('the host receives the whole run-time state on commit and on finish', async ({ page }) => {
  const frame = await started(page)
  await answers(frame, [
    set('cmi.core.lesson_location', 'page-2'),
    set('cmi.suspend_data', 'state'),
    set('cmi.objectives.0.id', 'obj-1'),
    set('cmi.interactions.0.id', 'q-1'),
    ['LMSCommit', ''],
  ])
  await expect.poll(async () => (await lastCommit(page))?.runtime.cmi, { timeout: 5000 }).toMatchObject({
    core: { lesson_location: 'page-2' }, suspend_data: 'state',
  })
  expect((await lastCommit(page))?.session_ended).toBe(false)
  await answers(frame, [set('cmi.core.session_time', '0000:02:00'), ['LMSFinish', '']])
  await expect.poll(async () => (await lastCommit(page))?.session_ended).toBe(true)
  expect(JSON.stringify((await lastCommit(page))?.runtime)).toContain('obj-1')
  expect(JSON.stringify((await lastCommit(page))?.runtime)).toContain('q-1')
})

test('a lesson that never sets its status finishes completed, and the mastery score turns a score into passed or failed', async ({ page }) => {
  const frame = await started(page, 'plain')
  await answers(frame, [['LMSFinish', '']])
  await expect.poll(async () => (await lastCommit(page, 'plain'))?.runtime.cmi).toMatchObject({ core: { lesson_status: 'completed' } })

  const passed = await started(page)
  await answers(passed, [set('cmi.core.score.raw', '90'), ['LMSFinish', '']])
  await expect.poll(async () => (await lastCommit(page))?.runtime.cmi).toMatchObject({ core: { lesson_status: 'passed' } })
})

test('the mastery score overrides the status a lesson reports when it has a score', async ({ page }) => {
  const frame = await started(page)
  await answers(frame, [set('cmi.core.lesson_status', 'completed'), set('cmi.core.score.raw', '60'), ['LMSFinish', '']])
  await expect.poll(async () => (await lastCommit(page))?.runtime.cmi).toMatchObject({ core: { lesson_status: 'failed' } })
})

test('a suspended lesson resumes with entry resume, its saved data and the time of finished sessions', async ({ page }) => {
  const frame = await started(page)
  await answers(frame, [
    set('cmi.core.exit', 'suspend'),
    set('cmi.core.session_time', '0000:01:30'),
    set('cmi.core.lesson_location', 'page-4'),
    set('cmi.suspend_data', 'saved'),
    ['LMSFinish', ''],
  ])
  await expect.poll(async () => (await lastCommit(page))?.session_ended).toBe(true)
  await navigate(page, '/elsewhere')
  await expect(page.getByText('Elsewhere')).toBeVisible()
  await navigate(page, '/learn/course-safety')
  const again = await openLesson(page, 'probe', 'probe.html')
  expect(await answers(again, [
    ['LMSInitialize', ''],
    get('cmi.core.entry'),
    get('cmi.core.lesson_location'),
    get('cmi.suspend_data'),
  ])).toEqual(['true/0', 'resume/0', 'page-4/0', 'saved/0'])
  const [total] = await run(again, [get('cmi.core.total_time')])
  expect(seconds(total!.value)).toBe(90)
})

test('a lesson left without suspending comes back with an empty entry', async ({ page }) => {
  const frame = await started(page)
  await answers(frame, [set('cmi.core.lesson_location', 'page-2'), ['LMSFinish', '']])
  await expect.poll(async () => (await lastCommit(page))?.session_ended).toBe(true)
  await navigate(page, '/elsewhere')
  await expect(page.getByText('Elsewhere')).toBeVisible()
  await navigate(page, '/learn/course-safety')
  const again = await openLesson(page, 'probe', 'probe.html')
  expect(await answers(again, [['LMSInitialize', ''], get('cmi.core.entry')])).toEqual(['true/0', '/0'])
})

test('known limit: a search for the SCORM 2004 API from a 1.2 lesson climbs past the wrapper and is refused by the app window', async ({ page }) => {
  const frame = await started(page)
  const outcome = await frame.evaluate(() => {
    let win: Window = window
    try {
      for (let tries = 0; tries < 8 && !(win as unknown as Record<string, unknown>).API_1484_11 && win.parent !== win; tries++) win = win.parent
      return 'finished without an error'
    } catch (error) {
      return (error as Error).name
    }
  })
  expect(outcome).toBe('SecurityError')
  expect(await wrapperFrame(page).evaluate(() => typeof (window as unknown as Record<string, unknown>).API)).toBe('object')
})

// Commit coalescing and ranged media at the player seam

test('commits are coalesced: one host write per interval with the latest state, and at once on finish', async ({ page }) => {
  const frame = await started(page)
  // Fifteen commits over about two and a half seconds.
  const elapsed = await frame.evaluate(async () => {
    const api = (window as unknown as { probe: { api: Record<string, (...args: string[]) => string> } }).probe.api
    const start = performance.now()
    for (let page = 1; page <= 15; page++) {
      api.LMSSetValue!('cmi.core.lesson_location', `page-${page}`)
      api.LMSCommit!('')
      await new Promise(resolve => setTimeout(resolve, 170))
    }
    return performance.now() - start
  })
  await expect.poll(async () => (await lastCommit(page))?.runtime.cmi, { timeout: 5000 }).toMatchObject({ core: { lesson_location: 'page-15' } })
  // At most one write per two-second interval (the wrapper's default), plus
  // the one the last interval leaves pending. Counted, not timed: the app
  // page may take a write in late when the machine is busy.
  const times = await commitTimes(page)
  expect(times.length).toBeGreaterThan(0)
  expect(times.length).toBeLessThanOrEqual(Math.ceil(elapsed / 2000) + 1)

  const before = times.length
  const finishedAt = Date.now()
  await answers(frame, [set('cmi.core.lesson_location', 'last'), ['LMSFinish', '']])
  // Sooner than the two-second interval: the finish does not wait for it.
  await expect.poll(async () => (await commits(page)).length, { timeout: 1900 }).toBe(before + 1)
  expect((await commitTimes(page)).slice(-1)[0]! - finishedAt).toBeLessThan(1900)
  expect(await lastCommit(page)).toMatchObject({ session_ended: true, runtime: { cmi: { core: { lesson_location: 'last' } } } })
})

test('long media in a package plays by range: every request is ranged, none is sent whole, and seeking asks for a later range', async ({ page, browserName }) => {
  // WebKitGTK's media source opens a file with no range and takes the first
  // answer's length as the file's size, so neither the cap's 416 nor a first
  // chunk plays it. Whether the app's own scheme on Linux behaves the same is
  // a device check (ticket 28).
  test.skip(browserName === 'webkit' && process.platform === 'linux', 'WebKitGTK media opens without a range; Linux device check pending')
  const { wholeFile } = await handlerLimits()
  // Larger than the handler sends whole.
  const served = await openPlayer(page, {
    fixture: 'scorm12-conformance',
    extraFiles: { 'media/long.wav': silentWav(wholeFile + 4 * 1024 * 1024) },
  })
  const frame = await openLesson(page, 'media', 'media.html')
  const duration = await frame.evaluate(() => new Promise<number>((resolve, reject) => {
    const clip = document.getElementById('clip') as HTMLAudioElement
    if (clip.readyState >= 1) resolve(clip.duration)
    clip.addEventListener('loadedmetadata', () => resolve(clip.duration))
    clip.addEventListener('error', () => reject(new Error(`media error ${clip.error?.code}`)))
  }))
  expect(duration).toBeGreaterThan(wholeFile / 8000 - 1)
  await frame.evaluate(() => new Promise<void>((resolve, reject) => {
    const clip = document.getElementById('clip') as HTMLAudioElement
    clip.addEventListener('seeked', () => resolve(), { once: true })
    clip.addEventListener('error', () => reject(new Error(`media error ${clip.error?.code}`)))
    clip.currentTime = clip.duration - 2
  }))
  const media = served.files.filter(file => file.path === 'media/long.wav')
  expect(media.length).toBeGreaterThan(0)
  expect(media.every(file => file.range !== null && file.status === 206)).toBe(true)
  expect(Math.max(...media.map(file => file.start ?? 0))).toBeGreaterThan(wholeFile)
})

// The matrix

test('every row of the conformance matrix names a test that exists', async ({ browserName }) => {
  test.skip(browserName !== 'chromium', 'reads files only; once is enough')
  const matrix = await readFile(new URL('./scorm12-conformance.md', import.meta.url), 'utf8')
  const sources = (await Promise.all(['./scorm12-conformance.spec.ts', './package-isolation.spec.ts', './package-player.spec.ts']
    .map(path => readFile(new URL(path, import.meta.url), 'utf8')))).join('\n')
  const lines = matrix.split('\n')
  // Table rows, without each table's header (the line before `|---`).
  const rows = lines.filter((line, index) => line.startsWith('| ') && !lines[index + 1]?.startsWith('|---'))
  expect(rows.length).toBeGreaterThan(40)
  for (const row of rows) {
    const named = [...row.matchAll(/«([^»]+)»/g)].map(match => match[1]!)
    expect(named.length, row).toBeGreaterThan(0)
    for (const title of named) {
      const literal = title.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
      expect(sources, title).toMatch(new RegExp(`test\\((['\`])${literal}`))
    }
  }
})
