import { createHash } from 'node:crypto'
import { readFile } from 'node:fs/promises'
import { expect, test, type Page } from '@playwright/test'

// The vendored SCORM runtime against the wrapper design: a page at the package
// origin holds the run-time API, a lesson in a nested frame finds it by the
// standard discovery algorithm and calls it synchronously, and the host (a
// page at another origin, standing in for the app) is handed state afterwards.
// Every byte is served from the repository; a request for anything else fails
// the test.

const HOST = 'http://app.test'
const PACKAGE = 'http://scorm-package.test'
const LAUNCH = `${PACKAGE}/launch-7f3a9c`

const fixture = (name: string) => new URL(`./scorm-runtime/${name}`, import.meta.url)
const vendored = (name: string) => new URL(`../../src-tauri/resources/scorm-again/${name}`, import.meta.url)

const served: Record<string, { file: URL, contentType: string }> = {
  [`${HOST}/host.html`]: { file: fixture('host.html'), contentType: 'text/html' },
  [`${LAUNCH}/wrapper.html`]: { file: fixture('wrapper.html'), contentType: 'text/html' },
  [`${LAUNCH}/runtime/scorm12.min.js`]: { file: vendored('scorm12.min.js'), contentType: 'text/javascript' },
  [`${LAUNCH}/content/lesson.html`]: { file: fixture('lesson.html'), contentType: 'text/html' },
  [`${LAUNCH}/content/frameset.html`]: { file: fixture('frameset.html'), contentType: 'text/html' },
}

interface Launch {
  /** The lesson's launch file, relative to the wrapper. */
  entry: string
  /** Run-time data the host supplies before the lesson starts. */
  cmi: Record<string, unknown>
}

interface HostMessage {
  reason: 'commit' | 'terminate'
  cmi: { core: Record<string, unknown>, suspend_data: string }
}

/** Opens the fake host, which launches the wrapper with `launch`. */
async function open(page: Page, launch: Launch) {
  const refused: string[] = []
  await page.route('**/*', async (route) => {
    const entry = served[route.request().url().split(/[?#]/)[0] ?? '']
    if (!entry) {
      refused.push(route.request().url())
      return route.abort()
    }
    await route.fulfill({ body: await readFile(entry.file), contentType: entry.contentType })
  })
  const config = { wrapper: `${LAUNCH}/wrapper.html`, packageOrigin: PACKAGE, launch }
  await page.goto(`${HOST}/host.html#${encodeURIComponent(JSON.stringify(config))}`)
  return { refused }
}

/** What the lesson saw: each API call it made and what came back, in order. */
async function transcript(page: Page, ...frames: string[]) {
  let scope = page.frameLocator('#package')
  for (const frame of frames) scope = scope.frameLocator(frame)
  const text = scope.locator('#transcript[data-done]')
  await expect(text).toBeAttached()
  return JSON.parse(await text.textContent() ?? '') as [string, string][]
}

/** What the host was handed, once `count` messages have arrived. */
async function received(page: Page, count: number) {
  const messages = () => page.evaluate(() => (window as unknown as { received: HostMessage[] }).received)
  await expect.poll(async () => (await messages()).length).toBe(count)
  return messages()
}

const firstLaunch: Launch = {
  entry: 'content/lesson.html',
  cmi: { core: { student_id: 'learner-for-this-package', student_name: 'Learner, Test' } },
}

test('a one-file SCORM 1.2 lesson finds the API from a nested frame and runs a whole session', async ({ page }) => {
  const { refused } = await open(page, firstLaunch)

  expect(await transcript(page, '#lesson')).toEqual([
    ['frames climbed to the API', '1'],
    ['LMSInitialize()', 'true'],
    ['LMSGetValue(cmi.core.student_id)', 'learner-for-this-package'],
    ['LMSGetValue(cmi.core.student_name)', 'Learner, Test'],
    ['LMSGetValue(cmi.core.entry)', ''],
    ['LMSGetValue(cmi.core.lesson_location)', ''],
    ['LMSGetValue(cmi.suspend_data)', ''],
    ['LMSSetValue(cmi.core.lesson_location, page-2)', 'true'],
    ['LMSSetValue(cmi.suspend_data, seen=1,2)', 'true'],
    ['LMSSetValue(cmi.core.lesson_status, completed)', 'true'],
    ['LMSGetValue(cmi.core.lesson_status)', 'completed'],
    ['LMSCommit()', 'true'],
    ['LMSGetLastError()', '0'],
    ['LMSFinish()', 'true'],
    ['LMSGetLastError()', '0'],
  ])

  const [commit, terminate] = await received(page, 2)
  expect(commit?.reason).toBe('commit')
  expect(commit?.cmi.core).toMatchObject({ lesson_status: 'completed', lesson_location: 'page-2' })
  expect(commit?.cmi.suspend_data).toBe('seen=1,2')
  expect(terminate?.reason).toBe('terminate')
  expect(terminate?.cmi.core).toMatchObject({ lesson_status: 'completed', lesson_location: 'page-2' })
  expect(refused).toEqual([])
})

test('a lesson inside its own frameset still finds the API', async ({ page }) => {
  await open(page, { ...firstLaunch, entry: 'content/frameset.html' })

  const calls = await transcript(page, '#lesson', '#inner')
  expect(calls[0]).toEqual(['frames climbed to the API', '2'])
  expect(calls).toContainEqual(['LMSInitialize()', 'true'])
  expect(calls).toContainEqual(['LMSFinish()', 'true'])
  expect((await received(page, 2)).map((message) => message.reason)).toEqual(['commit', 'terminate'])
})

test('a resumed lesson reads the state the host supplied before it started', async ({ page }) => {
  await open(page, {
    entry: 'content/lesson.html',
    cmi: {
      core: {
        student_id: 'learner-for-this-package',
        student_name: 'Learner, Test',
        entry: 'resume',
        lesson_location: 'page-2',
        lesson_status: 'incomplete',
      },
      suspend_data: 'seen=1,2',
    },
  })

  const calls = await transcript(page, '#lesson')
  expect(calls).toContainEqual(['LMSGetValue(cmi.core.entry)', 'resume'])
  expect(calls).toContainEqual(['LMSGetValue(cmi.core.lesson_location)', 'page-2'])
  expect(calls).toContainEqual(['LMSGetValue(cmi.suspend_data)', 'seen=1,2'])
})

test('the vendored runtime and its licence are the pinned files', async () => {
  const pins = (await readFile(vendored('SHA256SUMS'), 'utf8')).trim().split('\n').map((line) => line.split(/\s+/))
  expect(pins.map(([, name]) => name).sort()).toEqual(['LICENSE', 'scorm12.min.js', 'scorm2004.min.js'])
  for (const [digest, name] of pins) {
    const actual = createHash('sha256').update(await readFile(vendored(name ?? ''))).digest('hex')
    expect(actual, name).toBe(digest)
  }
})
