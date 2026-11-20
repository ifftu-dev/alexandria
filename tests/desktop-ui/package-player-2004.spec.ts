import { readFile } from 'node:fs/promises'
import { expect, test, type FrameLocator, type Page } from '@playwright/test'
import type { PackageCommit, PackageSequencingState } from '@/types'

// The package player with a SCORM 2004 package: the real course player, the
// real wrapper page, the vendored SCORM 2004 runtime and its sequencing
// engine, a three-lesson package with a sequencing rule written for these
// checks, and a fake host bridge. Served as in `package-player.spec.ts`: the
// package origin through `page.route`, with the bootstrap linked into every
// HTML page and the scheme handler's own policy headers. A request to
// anywhere else fails the test.

const PACKAGE_ORIGIN = 'http://scorm-package.test'
const serving = new URL('../../src-tauri/src/package_serving/', import.meta.url)
const packageFiles = new URL('./package-player-2004/package/', import.meta.url)
const reserved: Record<string, URL> = {
  'bootstrap.js': new URL('bootstrap.js', serving),
  'clear.html': new URL('clear.html', serving),
  'wrapper.html': new URL('wrapper.html', serving),
  'wrapper.js': new URL('wrapper.js', serving),
  'scorm2004.min.js': new URL('../../resources/scorm-again/scorm2004.min.js', serving),
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

async function open(page: Page, query = '') {
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
  await page.goto(`/tests/desktop-ui/?screen=package-player-2004${query}`)
  await expect(page.locator('[data-package-player]')).toBeVisible()
  return { refused }
}

interface PackageFixture {
  commits: PackageCommit[]
  sequencing: PackageSequencingState[]
  called: (command: string) => number
  navigate: (path: string) => Promise<void>
}

const commits = (page: Page) =>
  page.evaluate(() => (window as unknown as { packageFixture: PackageFixture }).packageFixture.commits)
const savedStates = (page: Page) =>
  page.evaluate(() => (window as unknown as { packageFixture: PackageFixture }).packageFixture.sequencing)
const called = (page: Page, command: string) =>
  page.evaluate((name) => (window as unknown as { packageFixture: PackageFixture }).packageFixture.called(name), command)
const navigate = (page: Page, path: string) =>
  page.evaluate((to) => (window as unknown as { packageFixture: PackageFixture }).packageFixture.navigate(to), path)

const lesson = (page: Page): FrameLocator => page.frameLocator('[data-package-frame]').frameLocator('#lesson')
const control = (page: Page, name: string) => page.locator(`[data-package-nav="${name}"]`)
const toc = (page: Page, id: string) => page.locator(`[data-package-toc] [data-activity="${id}"]`)

/** What the running lesson was given, once it has started. */
async function seen(page: Page, lessonId: string) {
  await expect(lesson(page).locator(`body[data-lesson="${lessonId}"] #seen[data-ready]`)).toBeAttached()
  return JSON.parse(await lesson(page).locator('#seen').getAttribute('data-values') ?? '{}') as Record<string, string>
}

test('a lesson the package locks cannot be reached until the lesson before it is passed', async ({ page }) => {
  const { refused } = await open(page)

  expect(await seen(page, 'intro')).toMatchObject({
    'initialised': 'true',
    'cmi.entry': 'ab-initio',
    'cmi.learner_id': 'learner-for-this-package',
    'cmi.learner_name': 'Ada Learner',
    'cmi.launch_data': 'route=b',
    'cmi.mode': 'normal',
  })
  await expect(page.locator('[data-package-toc] [data-activity]')).toHaveText([/Introduction/, /Evacuation drill/, /Check/])
  await expect(toc(page, 'intro')).toHaveAttribute('aria-current', 'true')

  // Locked: neither the table of contents nor continue reaches the drill.
  await expect(toc(page, 'drill')).toBeDisabled()
  await expect(control(page, 'continue')).toBeDisabled()
  await expect(control(page, 'previous')).toBeDisabled()

  await lesson(page).locator('#pass').click()
  await expect(control(page, 'continue')).toBeEnabled()

  await control(page, 'continue').click()
  expect(await seen(page, 'drill')).toMatchObject({ 'cmi.entry': 'ab-initio' })
  await expect(toc(page, 'drill')).toHaveAttribute('aria-current', 'true')
  await expect(toc(page, 'intro')).toHaveAttribute('data-complete', 'true')
  // The drill asks the player not to draw its previous control.
  await expect(control(page, 'previous')).toHaveCount(0)
  await expect(control(page, 'continue')).toBeVisible()

  // The introduction's session ended with its own termination and was saved.
  const intro = (await commits(page)).filter(commit => commit.activity_id === 'intro')
  expect(intro.slice(-1)[0]).toMatchObject({ session_ended: true, runtime: { cmi: { success_status: 'passed' } } })
  expect(refused).toEqual([])
})

test('suspend and resume restore the sequencing engine and the lesson where it was left', async ({ page }) => {
  const { refused } = await open(page)
  await seen(page, 'intro')
  await lesson(page).locator('#pass').click()
  await lesson(page).locator('#next').click()
  await seen(page, 'drill')
  await lesson(page).locator('#bookmark').click()

  await control(page, 'suspend').click()
  await expect(page.locator('[data-package-sequencing="ended"]')).toBeVisible()
  await expect.poll(async () => (await savedStates(page)).length).toBeGreaterThan(0)

  // Resume in the same player: the drill comes back as it was.
  await page.locator('[data-package-resume]').click()
  expect(await seen(page, 'drill')).toMatchObject({
    'cmi.entry': 'resume',
    'cmi.location': 'drill-2',
    'cmi.suspend_data': 'seen=1,2',
  })

  // Leave the course: the player suspends the package and hands the engine's
  // state to the host before the attempt is ended.
  const before = (await savedStates(page)).length
  await navigate(page, '/elsewhere')
  await expect(page.getByText('Elsewhere')).toBeVisible()
  await expect.poll(() => called(page, 'package_attempt_terminate')).toBe(1)
  expect((await savedStates(page)).length).toBeGreaterThan(before)

  // Back again: a new wrapper and a new engine, restored from that state.
  await navigate(page, '/learn/course-safety')
  expect(await seen(page, 'drill')).toMatchObject({ 'cmi.entry': 'resume', 'cmi.location': 'drill-2' })
  await expect(toc(page, 'intro')).toHaveAttribute('data-complete', 'true')
  // The global objective the introduction wrote still unlocks the drill.
  await expect(toc(page, 'drill')).toBeEnabled()
  await expect(control(page, 'previous')).toHaveCount(0)
  expect(refused).toEqual([])
})

test('the package completes when the sequencing engine reports the course completed', async ({ page }) => {
  const { refused } = await open(page)
  await seen(page, 'intro')
  await lesson(page).locator('#pass').click()
  await lesson(page).locator('#next').click()
  await seen(page, 'drill')
  await lesson(page).locator('#complete').click()
  await lesson(page).locator('#next').click()
  expect(await seen(page, 'quiz')).toMatchObject({ 'cmi.scaled_passing_score': '0.8' })
  await expect(page.locator('[data-package-result]')).not.toContainText('Completed')

  await lesson(page).locator('#pass').click()
  await lesson(page).locator('#next').click()

  await expect(page.locator('[data-package-sequencing="ended"]')).toBeVisible()
  await expect.poll(async () => (await savedStates(page)).slice(-1)[0]?.root.completion_status).toBe('completed')
  const result = page.locator('[data-package-result]')
  await expect(result).toContainText('Completed')
  await expect(result).toContainText(/self-reported/i)
  expect(await called(page, 'claim_course_completion')).toBe(0)
  expect(refused).toEqual([])
})

test('when starting delivers nothing the learner chooses a lesson', async ({ page }) => {
  const { refused } = await open(page, '&flow=off')
  await expect(page.locator('[data-package-sequencing="choose"]')).toBeVisible()
  await expect(control(page, 'continue')).toBeDisabled()
  await expect(toc(page, 'drill')).toBeDisabled()

  await toc(page, 'intro').click()
  expect(await seen(page, 'intro')).toMatchObject({ 'cmi.entry': 'ab-initio' })
  await expect(page.locator('[data-package-sequencing]')).toHaveCount(0)
  expect(refused).toEqual([])
})
