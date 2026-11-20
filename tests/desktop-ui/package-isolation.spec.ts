import { expect, test, type Frame, type Page } from '@playwright/test'
import {
  called, lesson, openLaunches, openLesson, openPlayer, PACKAGE_ORIGIN, seen, navigate, type Served,
} from './packageHarness'

// Isolation at the player seam: a hostile SCORM 1.2 package
// (`package-isolation/package/`) whose lessons each try one escape, run in
// the real player, wrapper and runtime in Chromium and WebKit. Each test
// asserts the escape failed by what the browser did (requests, windows,
// peer connections, bridge calls the harness saw), not by what the hostile
// lesson says about itself. See `packageHarness.ts` for what the harness
// mirrors from the scheme handler and the app's own policy.

const HOSTILE = 'hostile.example'

async function waitDone(page: Page) {
  const values = lesson(page).locator('#seen')
  await expect(values, { message: `the lesson did not finish: ${await values.getAttribute('data-values').catch(() => null)}` })
    .toHaveAttribute('data-done', '', { timeout: 15000 })
}

/**
 * Presses the lesson's button for `way` and waits until the attempt has
 * run: the lesson reported it, or the browser already replaced the lesson's
 * document (a refused navigation of its own frame leaves an error page). A
 * navigation or window the browser let through would start from that same
 * task; the short wait afterwards lets it reach the harness.
 */
async function attempt(page: Page, frame: Frame, way: string) {
  const lessonUrl = frame.url()
  await frame.getByRole('button', { name: way, exact: true }).click()
  await expect.poll(async () => frame.isDetached() || frame.url() !== lessonUrl
    || way in await frame.evaluate(() => JSON.parse(document.getElementById('seen')?.getAttribute('data-values') ?? '{}') as object)
      .catch(() => ({}))).toBe(true)
  await page.waitForTimeout(ATTEMPT_SETTLE_MS)
}
const ATTEMPT_SETTLE_MS = 200

/** The lesson's own account of each attempt, for the failure message. */
async function attempts(page: Page) {
  const values = await seen(page)
  return Object.entries(values).map(([how, result]) => `${how}: ${JSON.stringify(result)}`).join('\n')
}

test('no request leaves the package origin, from the lesson, a worker or any window the lesson can reach', async ({ page }) => {
  const served = await openPlayer(page, { fixture: 'package-isolation' })
  await openLesson(page, 'network')
  // Done only once every window was tried and the worker reported its attempts.
  await waitDone(page)
  expect(served.refused, await attempts(page)).toEqual([])
  expect(served.sockets).toEqual([])
})

test('no peer connection can be made from any window the lesson can reach', async ({ page }) => {
  const served = await openPlayer(page, { fixture: 'package-isolation' })
  await openLesson(page, 'peer')
  await waitDone(page)
  expect(served.peers, await attempts(page)).toEqual([])
  expect(Object.values(await seen(page))).not.toContain('peer connection made')
})

test('the command bridge is gone from every window the lesson can reach, frames it made itself included', async ({ page }) => {
  const served = await openPlayer(page, { fixture: 'package-isolation' })
  await openLesson(page, 'bridge')
  await waitDone(page)
  expect(served.bridge, await attempts(page)).toEqual([])
})

test('control: without the bootstrap the same lessons do reach the bridge and a peer connection', async ({ page }) => {
  // Proves the two checks above can see an escape: the harness's stand-in
  // bridge and reporting peer connection are really there to be found.
  const served = await openPlayer(page, { fixture: 'package-isolation', bootstrap: false })
  await openLesson(page, 'peer')
  await waitDone(page)
  expect(served.peers.length).toBeGreaterThan(0)
  await openLesson(page, 'bridge')
  await waitDone(page)
  expect(served.bridge.length).toBeGreaterThan(0)
})

const NAVIGATIONS = [
  'the app window',
  'the package frame',
  'the lesson frame',
  'a link aimed at the app window',
  'a link aimed at the package frame',
  'a form aimed at the app window',
  'a refresh header',
  'a data address in the package frame',
  'window.open',
  'the package frame from a new frame',
  'the app window from a new frame',
]

for (const way of NAVIGATIONS) {
  test(`no frame leaves the package origin: ${way}`, async ({ page }) => {
    const served = await openPlayer(page, { fixture: 'package-isolation' })
    const appUrl = page.url()
    await attempt(page, await openLesson(page, 'navigation'), way)
    expect(served.refused).toEqual([])
    expect(page.url()).toBe(appUrl)
    const left = page.frames().map(frame => frame.url()).filter(url => url.includes(HOSTILE) || url.startsWith('data:'))
    expect(left).toEqual([])
    await expect(page.locator('[data-package-label]')).toBeVisible()
  })
}

const POPUPS = [
  'window.open to another origin',
  'window.open in the package',
  'a link to a new window',
  'a link to a named window',
  'a form to a new window',
  'window.open from a new frame',
  'window.open from the wrapper',
]

for (const way of POPUPS) {
  test(`no window opens: ${way}`, async ({ page }) => {
    const served = await openPlayer(page, { fixture: 'package-isolation' })
    await attempt(page, await openLesson(page, 'popup'), way)
    expect(served.popups).toEqual([])
    expect(page.context().pages()).toHaveLength(1)
    expect(served.refused).toEqual([])
  })
}

/**
 * Runs the storage lesson in one launch, then in a second launch reached by
 * `next`, and returns what the second one found plus the first one's prefix.
 */
async function acrossLaunches(page: Page, next: 'fresh attempt' | 'leave and return') {
  const served = await openPlayer(page, { fixture: 'package-isolation' })
  await openLesson(page, 'storage')
  await waitDone(page)
  const before = await seen(page)
  // Nothing was there for the first launch either: storage was cleared first.
  expect(before.found).not.toHaveProperty('localStorage')
  const [first] = await openLaunches(page)
  expect(first).toBeTruthy()
  expect(String(before.left)).toContain(`/${first}/`)

  if (next === 'fresh attempt') {
    await page.locator('[data-package-fresh]').click()
    await page.locator('[data-package-fresh-confirm]').click()
  } else {
    await navigate(page, '/elsewhere')
    await expect(page.getByText('Elsewhere')).toBeVisible()
    await expect.poll(() => called(page, 'package_launch_close')).toBe(1)
    await navigate(page, '/learn/course-safety')
  }
  await expect.poll(async () => (await openLaunches(page)).filter(prefix => prefix !== first).length).toBe(1)
  const [second] = await openLaunches(page)
  expect(second).not.toBe(first)
  // The second launch has started the lesson it resumes at before another is picked.
  await expect.poll(() => page.frames().some(frame => frame.url().includes(`/${second}/`)
    && frame.parentFrame()?.url().endsWith('/_alexandria/wrapper.html'))).toBe(true)
  await openLesson(page, 'storage')
  await waitDone(page)
  const after = await seen(page)
  return { served, first: first!, after }
}

for (const next of ['fresh attempt', 'leave and return'] as const) {
  test(`nothing a launch stored reaches the next launch of the package (${next})`, async ({ page }) => {
    const { after } = await acrossLaunches(page, next)
    const found = after.found as Record<string, unknown>
    expect(JSON.stringify(found)).not.toContain('left by')
    for (const store of ['localStorage', 'sessionStorage', 'cookies', 'indexedDB', 'caches', 'package frame name', 'this frame name']) {
      expect(found, store).not.toHaveProperty(store)
    }
    expect(String(after['service worker'])).toMatch(/^(refused|unavailable)/)
  })

  test(`a launch cannot read or reach an earlier launch's address (${next})`, async ({ page }) => {
    const { served, first, after } = await acrossLaunches(page, next)
    expect(JSON.stringify(after.found)).not.toContain(first)
    // The address itself is dead: the earlier launch was closed.
    const status = await page.frames().find(frame => frame.url().includes('/storage.html'))!.evaluate(
      async address => (await fetch(address)).status,
      `${PACKAGE_ORIGIN}/${first}/storage.html`,
    )
    expect(status).toBe(404)
    expect(served.missing).toContain(`${PACKAGE_ORIGIN}/${first}/storage.html`)
  })
}

// Every launch shares the package origin, so clearing its storage is the only
// thing between one launch's data and the next. A launch whose clearing page
// does not answer truthfully, for this request, must not start.
const clearPage = (message: string) => `<!doctype html><html><head><script>
  var nonce = location.hash.slice(1);
  window.top.postMessage(${message}, '*');
</script></head><body></body></html>`

const BAD_CLEARS: Record<string, string> = {
  'it reports that clearing failed': `{ source: 'alexandria-package', type: 'storage-cleared', nonce: nonce, ok: false, failed: ['indexedDB'] }`,
  'its answer lacks the request nonce': `{ source: 'alexandria-package', type: 'storage-cleared', ok: true, failed: [] }`,
  'its answer carries another nonce': `{ source: 'alexandria-package', type: 'storage-cleared', nonce: 'stale', ok: true, failed: [] }`,
  'its answer does not come from the clearing page': `{ type: 'storage-cleared', nonce: nonce, ok: true, failed: [] }`,
}

for (const [why, message] of Object.entries(BAD_CLEARS)) {
  test(`a launch does not start when storage clearing fails: ${why}`, async ({ page }) => {
    await openPlayer(page, { fixture: 'package-isolation', reservedFiles: { 'clear.html': clearPage(message) } })
    await expect(page.locator('[data-package-player] [role="alert"]')).toBeVisible({ timeout: 10000 })
    expect(page.frames().some(frame => frame.url().includes('/_alexandria/wrapper.html'))).toBe(false)
    expect(await called(page, 'package_attempt_initialise')).toBe(0)
  })
}

async function labelUncovered(page: Page): Promise<boolean> {
  const label = page.locator('[data-package-label]')
  const box = await label.boundingBox()
  if (!box) return false
  const points = [0.02, 0.5, 0.98].map(fraction => ({ x: box.x + box.width * fraction, y: box.y + box.height / 2 }))
  return page.evaluate(list => list.every(({ x, y }) => Boolean(document.elementFromPoint(x, y)?.closest('[data-package-label]'))), points)
}

async function frameBelowLabel(page: Page): Promise<boolean> {
  const label = await page.locator('[data-package-label]').boundingBox()
  const frame = await page.locator('[data-package-frame]').boundingBox()
  return Boolean(label && frame && frame.y >= label.y + label.height - 0.5)
}

test('hostile content cannot cover the label, in or out of focus mode', async ({ page }) => {
  await openPlayer(page, { fixture: 'package-isolation' })
  const frame = await openLesson(page, 'label')
  const label = page.locator('[data-package-label]')
  const player = page.locator('[data-package-player]')

  expect(await labelUncovered(page)).toBe(true)
  expect(await frameBelowLabel(page)).toBe(true)
  await expect(label).toContainText('recovery phrase')

  // Every spelling of full screen: the player turns it into focus mode.
  await frame.evaluate(() => (window as unknown as { askForFullScreen: () => void }).askForFullScreen())
  await expect(player).toHaveAttribute('data-focus', 'true')
  await expect(label).toBeInViewport({ ratio: 1 })
  expect(await labelUncovered(page)).toBe(true)
  expect(await frameBelowLabel(page)).toBe(true)
  expect(await page.evaluate(() => document.fullscreenElement)).toBeNull()

  await label.getByRole('button').click()
  await expect(player).toHaveAttribute('data-focus', 'false')
  expect(await labelUncovered(page)).toBe(true)
  expect(await frameBelowLabel(page)).toBe(true)
})

export type { Served }
