import { readFile } from 'node:fs/promises'
import { expect, test, type FrameLocator, type Page } from '@playwright/test'
import type { PackageCommit } from '@/types'

// The package player at the player seam: the real course player, the real
// wrapper page and the vendored SCORM 1.2 runtime, a small multi-lesson
// package written for these checks, and a fake host bridge. The package
// origin is served here with `page.route`, with the bootstrap linked into
// every HTML page and the content policy headers the scheme handler sends,
// so the browser enforces what the app would. A request to anywhere else
// fails the test.

const PACKAGE_ORIGIN = 'http://scorm-package.test'
const serving = new URL('../../src-tauri/src/package_serving/', import.meta.url)
const packageFiles = new URL('./package-player/package/', import.meta.url)
const reserved: Record<string, URL> = {
  'bootstrap.js': new URL('bootstrap.js', serving),
  'clear.html': new URL('clear.html', serving),
  'wrapper.html': new URL('wrapper.html', serving),
  'wrapper.js': new URL('wrapper.js', serving),
  'scorm12.min.js': new URL('../../resources/scorm-again/scorm12.min.js', serving),
}

/** A string constant from the scheme handler, so the headers here stay the app's. */
async function handlerConstant(name: string) {
  const source = await readFile(new URL('handler.rs', serving), 'utf8')
  const found = new RegExp(`const ${name}: &str = "((?:[^"\\\\]|\\\\[\\s\\S])*)";`).exec(source)
  if (!found?.[1]) throw new Error(`${name} not found in handler.rs`)
  return found[1].replace(/\\\n\s*/g, '')
}

const contentType = (path: string) =>
  path.endsWith('.html') ? 'text/html' : path.endsWith('.js') ? 'text/javascript' : 'application/octet-stream'

async function open(page: Page) {
  const policy = await handlerConstant('CONTENT_POLICY')
  const permissions = await handlerConstant('PERMISSIONS_POLICY')
  const refused: string[] = []
  await page.route('**/*', async (route) => {
    const url = new URL(route.request().url())
    if (url.hostname === '127.0.0.1') return route.continue()
    const match = url.origin === PACKAGE_ORIGIN ? /^\/(launch-\d+)\/(.+)$/.exec(url.pathname) : null
    if (!match) {
      refused.push(url.href)
      return route.abort()
    }
    const [, prefix, path] = match as unknown as [string, string, string]
    const file = path.startsWith('_alexandria/') ? reserved[path.slice('_alexandria/'.length)] : new URL(path, packageFiles)
    let body: Buffer
    try {
      if (!file) throw new Error('not reserved')
      body = await readFile(file)
    } catch {
      refused.push(url.href)
      return route.fulfill({ status: 404, body: '' })
    }
    if (path.endsWith('.html')) {
      const tag = `<script src="/${prefix}/_alexandria/bootstrap.js"></script>`
      const html = body.toString('utf8')
      const at = html.indexOf('>', html.toLowerCase().indexOf('<head')) + 1
      body = Buffer.from(html.slice(0, at) + tag + html.slice(at))
    }
    await route.fulfill({
      body,
      contentType: contentType(path),
      headers: { 'Content-Security-Policy': policy, 'Permissions-Policy': permissions },
    })
  })
  await page.goto('/tests/desktop-ui/?screen=package-player')
  await expect(page.locator('[data-package-player]')).toBeVisible()
  return { refused }
}

interface PackageFixture {
  commits: PackageCommit[]
  control: { failCommits: boolean }
  called: (command: string) => number
  navigate: (path: string) => Promise<void>
}

const commits = (page: Page) =>
  page.evaluate(() => (window as unknown as { packageFixture: PackageFixture }).packageFixture.commits)
const called = (page: Page, command: string) =>
  page.evaluate((name) => (window as unknown as { packageFixture: PackageFixture }).packageFixture.called(name), command)
const navigate = (page: Page, path: string) =>
  page.evaluate((to) => (window as unknown as { packageFixture: PackageFixture }).packageFixture.navigate(to), path)
const failCommits = (page: Page, fail: boolean) =>
  page.evaluate((value) => { (window as unknown as { packageFixture: PackageFixture }).packageFixture.control.failCommits = value }, fail)

const lesson = (page: Page): FrameLocator => page.frameLocator('[data-package-frame]').frameLocator('#lesson')

/** What the lesson was given, once it has started. */
async function seen(page: Page) {
  const values = lesson(page).locator('#seen[data-ready]')
  await expect(values).toBeAttached()
  return JSON.parse(await values.getAttribute('data-values') ?? '{}') as Record<string, string>
}

async function seenValue(page: Page, name: string) {
  return JSON.parse(await lesson(page).locator('#seen').getAttribute('data-values') ?? '{}')[name] as string | undefined
}

test('a SCORM 1.2 lesson launches with the host values, saves coalesced state and resumes where it was left', async ({ page }) => {
  const { refused } = await open(page)

  const label = page.locator('[data-package-label]')
  await expect(label).toContainText('Fixture instructor')
  await expect(label).toContainText('recovery phrase')
  await expect(page.locator('[data-package-toc] [data-activity]')).toHaveText([
    /Welcome to the workshop/, /Safety check/, /Summary sheet/,
  ])

  expect(await seen(page)).toMatchObject({
    'initialised': 'true',
    'cmi.core.student_id': 'learner-for-this-package',
    'cmi.core.student_name': 'Ada Learner',
    'cmi.core.entry': 'ab-initio',
    'cmi.core.lesson_location': '',
  })

  // Two pages in quick succession: two commits from the lesson, one write.
  await lesson(page).locator('#next').click()
  await lesson(page).locator('#next').click()
  await expect.poll(async () => (await commits(page)).length, { timeout: 6000 }).toBe(1)
  const [saved] = await commits(page)
  expect(saved?.activity_id).toBe('welcome')
  expect(saved?.session_ended).toBe(false)
  expect(saved?.runtime.cmi).toMatchObject({ core: { lesson_location: 'page-3' }, suspend_data: '1,2,3' })

  // Suspend: the finishing commit is sent at once.
  await lesson(page).locator('#leave').click()
  await expect.poll(async () => (await commits(page)).length).toBe(2)
  expect((await commits(page))[1]).toMatchObject({ activity_id: 'welcome', session_ended: true })

  // Leave the course and come back: the launch is closed and the attempt
  // ended, then the lesson resumes from its bookmark and saved state.
  await navigate(page, '/elsewhere')
  await expect(page.getByText('Elsewhere')).toBeVisible()
  await expect.poll(() => called(page, 'package_attempt_terminate')).toBe(1)
  expect(await called(page, 'package_launch_close')).toBe(1)
  await navigate(page, '/learn/course-safety')

  expect(await seen(page)).toMatchObject({
    'cmi.core.entry': 'resume',
    'cmi.core.lesson_location': 'page-3',
    'cmi.suspend_data': '1,2,3',
  })
  expect(refused).toEqual([])
})

test('a lesson left without finishing still hands over its state when the player closes', async ({ page }) => {
  await open(page)
  await seen(page)
  await lesson(page).locator('#next').click()
  // Leave before the coalescing interval runs out.
  await navigate(page, '/elsewhere')

  await expect.poll(async () => (await commits(page)).slice(-1)[0]?.session_ended, { timeout: 6000 }).toBe(true)
  expect((await commits(page)).slice(-1)[0]?.runtime.cmi).toMatchObject({ core: { lesson_location: 'page-2' } })
})

test('pending state is handed over at once when the app goes to the background', async ({ page }) => {
  await open(page)
  await seen(page)
  await lesson(page).locator('#next').click()

  const wrapper = page.frames().find(frame => frame.url().endsWith('/_alexandria/wrapper.html'))
  expect(wrapper).toBeTruthy()
  await wrapper!.evaluate(() => {
    Object.defineProperty(document, 'visibilityState', { configurable: true, value: 'hidden' })
    document.dispatchEvent(new Event('visibilitychange'))
  })
  // Well inside the two-second coalescing interval.
  await expect.poll(async () => (await commits(page)).length, { timeout: 1000 }).toBe(1)
  expect((await commits(page))[0]).toMatchObject({ activity_id: 'welcome', session_ended: false })
})

test('the package completes through the player, its result is labelled self-reported, and no credential is claimed', async ({ page }) => {
  const { refused } = await open(page)
  await seen(page)
  await lesson(page).locator('#done').click()
  await expect(page.locator('[data-activity="welcome"]')).toHaveAttribute('data-complete', 'true')

  await page.locator('[data-activity="quiz"]').click()
  expect(await seen(page)).toMatchObject({
    'cmi.core.entry': 'ab-initio',
    'cmi.launch_data': 'questions=2',
    'cmi.student_data.mastery_score': '80',
  })
  await lesson(page).locator('#pass').click()
  // The runtime applied the manifest's mastery score when the lesson finished.
  await expect.poll(async () => (await commits(page)).filter(commit => commit.activity_id === 'quiz').slice(-1)[0]?.runtime.cmi)
    .toMatchObject({ core: { lesson_status: 'passed', score: { raw: '90' } } })
  await expect(page.locator('[data-activity="quiz"]')).toHaveAttribute('data-complete', 'true')
  // The package moves on only when the learner does.
  await expect(page.locator('[data-activity="quiz"]')).toHaveAttribute('aria-current', 'true')

  await page.locator('[data-activity="summary"]').click()
  await expect(lesson(page).locator('#summary')).toBeVisible()

  const result = page.locator('[data-package-result]')
  await expect(result).toContainText('90%')
  await expect(result).toContainText(/self-reported/i)
  await expect(page.locator('[data-package-course-complete]')).toContainText(/self-reported/i)
  expect(await called(page, 'claim_course_completion')).toBe(0)
  await expect(page.getByText(/Couldn.t finish/i)).toHaveCount(0)
  expect(refused).toEqual([])
})

test('focus mode fills the window, a full-screen request becomes focus mode, and the label stays in view', async ({ page }) => {
  await open(page)
  await seen(page)
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

  await lesson(page).locator('#enlarge').click()
  await expect.poll(() => seenValue(page, 'fullscreen')).toBe('NotAllowedError')
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
  const { refused } = await open(page)
  await seen(page)
  const notices = page.locator('[data-package-blocked]')

  await lesson(page).locator('#remote').click()
  await expect(notices).toContainText('images.example.com')
  await lesson(page).locator('#alert').click()
  await expect(notices).toContainText(/dialog/i)
  await lesson(page).locator('#glossary').click()
  await expect(lesson(page).locator('#glossary')).toBeVisible()
  await expect(notices).toContainText(/pop-up/i)
  expect(refused).toEqual([])
})

test('a failed save shows a player-level error and the next save clears it', async ({ page }) => {
  await open(page)
  await seen(page)
  await failCommits(page, true)
  await lesson(page).locator('#leave').click()
  const error = page.locator('[data-package-save-error]')
  await expect(error).toBeVisible()
  expect(await commits(page)).toEqual([])

  await failCommits(page, false)
  await page.locator('[data-activity="quiz"]').click()
  await lesson(page).locator('#fail').click()
  await expect(error).toBeHidden()
})

test('a fresh attempt starts the package from the beginning', async ({ page }) => {
  await open(page)
  await seen(page)
  await lesson(page).locator('#next').click()
  await lesson(page).locator('#leave').click()
  await expect.poll(async () => (await commits(page)).length).toBeGreaterThan(0)

  await page.locator('[data-package-fresh]').click()
  await page.locator('[data-package-fresh-confirm]').click()

  expect(await seen(page)).toMatchObject({ 'cmi.core.entry': 'ab-initio', 'cmi.core.lesson_location': '' })
  expect(await called(page, 'package_attempt_fresh')).toBe(1)
  await expect(page.locator('[data-package-attempt]')).toContainText('2')
})
