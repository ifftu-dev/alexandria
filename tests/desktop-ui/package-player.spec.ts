import { expect, test, type Frame, type Page } from '@playwright/test'
import {
  called, commits, failCommits, lessonFrame, navigate, openLaunches, openLesson, openPlayer, seenIn, wrapperFrame,
} from './packageHarness'

// The package player at the player seam: the real course player, the real
// wrapper page and the vendored SCORM 1.2 runtime, a small multi-lesson
// package written for these checks, and a fake host bridge. The package
// origin is served by `packageHarness.ts`, with the bootstrap linked into
// every HTML page and the content policy headers the scheme handler sends,
// so the browser enforces what the app would. A request to anywhere else
// fails the test.

/** The welcome lesson, once it is the only lesson frame and has started (optionally under `prefix`). */
const welcome = (page: Page, prefix?: string) => openLesson(page, 'welcome', 'welcome/index.html', prefix)
const quiz = (page: Page) => openLesson(page, 'quiz', 'quiz/index.html')

async function seenValue(frame: Frame, name: string) {
  return JSON.parse(await frame.locator('#seen').getAttribute('data-values') ?? '{}')[name] as string | undefined
}

test('a SCORM 1.2 lesson launches with the host values, saves coalesced state and resumes where it was left', async ({ page }) => {
  const served = await openPlayer(page, { fixture: 'package-player' })

  const label = page.locator('[data-package-label]')
  await expect(label).toContainText('Fixture instructor')
  await expect(label).toContainText('recovery phrase')
  await expect(page.locator('[data-package-toc] [data-activity]')).toHaveText([
    /Welcome to the workshop/, /Safety check/, /Summary sheet/,
  ])

  const [first] = await openLaunches(page)
  const lesson = await welcome(page)
  expect(await seenIn(lesson)).toMatchObject({
    'initialised': 'true',
    'cmi.core.student_id': 'learner-for-this-package',
    'cmi.core.student_name': 'Ada Learner',
    'cmi.core.entry': 'ab-initio',
    'cmi.core.lesson_location': '',
  })

  // Two pages in quick succession: two commits from the lesson, one write.
  await lesson.locator('#next').click()
  await lesson.locator('#next').click()
  await expect.poll(async () => (await commits(page)).length, { timeout: 6000 }).toBe(1)
  const [saved] = await commits(page)
  expect(saved?.activity_id).toBe('welcome')
  expect(saved?.session_ended).toBe(false)
  expect(saved?.runtime.cmi).toMatchObject({ core: { lesson_location: 'page-3' }, suspend_data: '1,2,3' })

  // Suspend: the finishing commit is sent at once.
  await lesson.locator('#leave').click()
  await expect.poll(async () => (await commits(page)).length).toBe(2)
  expect((await commits(page))[1]).toMatchObject({ activity_id: 'welcome', session_ended: true })

  // Leave the course and come back: the launch is closed and the attempt
  // ended, then the lesson resumes from its bookmark and saved state.
  await navigate(page, '/elsewhere')
  await expect(page.getByText('Elsewhere')).toBeVisible()
  await expect.poll(() => called(page, 'package_attempt_terminate')).toBe(1)
  expect(await called(page, 'package_launch_close')).toBe(1)
  await navigate(page, '/learn/course-safety')

  await expect.poll(async () => (await openLaunches(page)).filter(prefix => prefix !== first).length).toBe(1)
  const [second] = await openLaunches(page)
  expect(await seenIn(await welcome(page, second))).toMatchObject({
    'cmi.core.entry': 'resume',
    'cmi.core.lesson_location': 'page-3',
    'cmi.suspend_data': '1,2,3',
  })
  expect(served.refused).toEqual([])
})

test('a lesson left without finishing still hands over its state when the player closes', async ({ page }) => {
  await openPlayer(page, { fixture: 'package-player' })
  const lesson = await welcome(page)
  await lesson.locator('#next').click()
  // Leave before the coalescing interval runs out.
  await navigate(page, '/elsewhere')

  await expect.poll(async () => (await commits(page)).slice(-1)[0]?.session_ended, { timeout: 6000 }).toBe(true)
  expect((await commits(page)).slice(-1)[0]?.runtime.cmi).toMatchObject({ core: { lesson_location: 'page-2' } })
})

test('pending state is handed over at once when the app goes to the background', async ({ page }) => {
  await openPlayer(page, { fixture: 'package-player' })
  const lesson = await welcome(page)
  await lesson.locator('#next').click()

  await wrapperFrame(page).evaluate(() => {
    Object.defineProperty(document, 'visibilityState', { configurable: true, value: 'hidden' })
    document.dispatchEvent(new Event('visibilitychange'))
  })
  // Well inside the two-second coalescing interval.
  await expect.poll(async () => (await commits(page)).length, { timeout: 1000 }).toBe(1)
  expect((await commits(page))[0]).toMatchObject({ activity_id: 'welcome', session_ended: false })
})

test('the package completes through the player, its result is labelled self-reported, and no credential is claimed', async ({ page }) => {
  const served = await openPlayer(page, { fixture: 'package-player' })
  const lesson = await welcome(page)
  await lesson.locator('#done').click()
  await expect(page.locator('[data-activity="welcome"]')).toHaveAttribute('data-complete', 'true')

  const quizLesson = await quiz(page)
  expect(await seenIn(quizLesson)).toMatchObject({
    'cmi.core.entry': 'ab-initio',
    'cmi.launch_data': 'questions=2',
    'cmi.student_data.mastery_score': '80',
  })
  await quizLesson.locator('#pass').click()
  // The runtime applied the manifest's mastery score when the lesson finished.
  await expect.poll(async () => (await commits(page)).filter(commit => commit.activity_id === 'quiz').slice(-1)[0]?.runtime.cmi)
    .toMatchObject({ core: { lesson_status: 'passed', score: { raw: '90' } } })
  await expect(page.locator('[data-activity="quiz"]')).toHaveAttribute('data-complete', 'true')
  // The package moves on only when the learner does.
  await expect(page.locator('[data-activity="quiz"]')).toHaveAttribute('aria-current', 'true')

  await page.locator('[data-activity="summary"]').click()
  await expect((await lessonFrame(page, 'summary.html')).locator('#summary')).toBeVisible()

  const result = page.locator('[data-package-result]')
  await expect(result).toContainText('90%')
  await expect(result).toContainText(/self-reported/i)
  await expect(page.locator('[data-package-course-complete]')).toContainText(/self-reported/i)
  expect(await called(page, 'claim_course_completion')).toBe(0)
  await expect(page.getByText(/Couldn.t finish/i)).toHaveCount(0)
  expect(served.refused).toEqual([])
})

test('focus mode fills the window, a full-screen request becomes focus mode, and the label stays in view', async ({ page }) => {
  await openPlayer(page, { fixture: 'package-player' })
  const lesson = await welcome(page)
  const label = page.locator('[data-package-label]')
  const uncovered = async () => {
    const box = await label.boundingBox()
    if (!box) return false
    return page.evaluate(({ x, y }) => {
      const hit = document.elementFromPoint(x, y)
      return Boolean(hit?.closest('[data-package-label]'))
    }, { x: box.x + box.width / 2, y: box.y + box.height / 2 })
  }
  expect(await uncovered()).toBe(true)

  await lesson.locator('#enlarge').click()
  await expect.poll(() => seenValue(lesson, 'fullscreen')).toBe('NotAllowedError')
  const player = page.locator('[data-package-player]')
  await expect(player).toHaveAttribute('data-focus', 'true')
  const viewport = page.viewportSize()!
  expect(await player.boundingBox()).toEqual({ x: 0, y: 0, width: viewport.width, height: viewport.height })
  await expect(label).toBeInViewport({ ratio: 1 })
  expect(await uncovered()).toBe(true)

  await label.getByRole('button').click()
  await expect(player).toHaveAttribute('data-focus', 'false')
  await expect(label).toBeInViewport({ ratio: 1 })
  expect(await uncovered()).toBe(true)
})

test('the player says which blocked thing the content asked for', async ({ page }) => {
  const served = await openPlayer(page, { fixture: 'package-player' })
  const lesson = await welcome(page)
  const notices = page.locator('[data-package-blocked]')

  await lesson.locator('#remote').click()
  await expect(notices).toContainText('images.example.com')
  await lesson.locator('#alert').click()
  await expect(notices).toContainText(/dialog/i)
  await lesson.locator('#glossary').click()
  await expect(lesson.locator('#glossary')).toBeVisible()
  await expect(notices).toContainText(/pop-up/i)
  expect(served.refused).toEqual([])
})

test('a failed save shows a player-level error and the next save clears it', async ({ page }) => {
  await openPlayer(page, { fixture: 'package-player' })
  const lesson = await welcome(page)
  await failCommits(page, true)
  await lesson.locator('#leave').click()
  const error = page.locator('[data-package-save-error]')
  await expect(error).toBeVisible()
  expect(await commits(page)).toEqual([])

  await failCommits(page, false)
  await (await quiz(page)).locator('#fail').click()
  await expect(error).toBeHidden()
})

test('a fresh attempt starts the package from the beginning', async ({ page }) => {
  await openPlayer(page, { fixture: 'package-player' })
  const lesson = await welcome(page)
  await lesson.locator('#next').click()
  await lesson.locator('#leave').click()
  await expect.poll(async () => (await commits(page)).length).toBeGreaterThan(0)
  const [first] = await openLaunches(page)

  await page.locator('[data-package-fresh]').click()
  await page.locator('[data-package-fresh-confirm]').click()

  // The second attempt's lesson, under its own launch, not the first attempt's still on screen.
  await expect(page.locator('[data-package-attempt]')).toContainText('2')
  await expect.poll(() => called(page, 'package_attempt_fresh')).toBe(1)
  await expect.poll(async () => (await openLaunches(page)).filter(prefix => prefix !== first).length).toBe(1)
  const [second] = await openLaunches(page)
  expect(await seenIn(await welcome(page, second))).toMatchObject({ 'cmi.core.entry': 'ab-initio', 'cmi.core.lesson_location': '' })
})
