import { expect, test, type Browser, type Page } from '@playwright/test'

/** The longest main-thread task panning may cause. */
const LONG_TASK_BUDGET_MS = 50

interface TraceEvent { name: string; ph: string; pid: number; tid: number; dur?: number; tdur?: number; args?: { name?: string } }

/**
 * Runs `gesture` under a Chromium trace and returns the page's main-thread
 * tasks: each one's CPU time (thread time) and wall time, in milliseconds.
 * The budget holds the CPU time, which the machine's other work (other specs,
 * other builds) does not stretch, so the check means the same on a loaded
 * machine as on an idle one; wall time is reported beside it.
 */
async function mainThreadTasks(browser: Browser, page: Page, gesture: () => Promise<void>): Promise<{ cpu: number[]; wall: number[] }> {
  await browser.startTracing(page, { categories: ['devtools.timeline', 'disabled-by-default-devtools.timeline'] })
  await gesture()
  const trace = JSON.parse((await browser.stopTracing()).toString()) as { traceEvents: TraceEvent[] }
  const main = new Set(trace.traceEvents.filter(event => event.ph === 'M' && event.name === 'thread_name' && event.args?.name === 'CrRendererMain')
    .map(event => `${event.pid}:${event.tid}`))
  const tasks = trace.traceEvents.filter(event => event.ph === 'X' && event.name === 'RunTask' && main.has(`${event.pid}:${event.tid}`))
  return { cpu: tasks.map(event => (event.tdur ?? event.dur ?? 0) / 1000), wall: tasks.map(event => (event.dur ?? 0) / 1000) }
}

function report(label: string, tasks: { cpu: number[]; wall: number[] }, testInfo: { annotations: { type: string; description?: string }[] }): number {
  const longest = Math.max(0, ...tasks.cpu), longestWall = Math.max(0, ...tasks.wall)
  testInfo.annotations.push({ type: `longest ${label} task CPU (ms)`, description: longest.toFixed(1) }, { type: `longest ${label} task wall (ms)`, description: longestWall.toFixed(1) })
  console.log(`atlas ${label}: ${tasks.cpu.length} main-thread tasks, longest ${longest.toFixed(1)} ms CPU (${longestWall.toFixed(1)} ms wall)`)
  expect(tasks.cpu.length).toBeGreaterThan(0)
  return longest
}

test('a 1,500-skill atlas pans without a main-thread task over 50 ms', async ({ page, browser, browserName }, testInfo) => {
  test.skip(browserName !== 'chromium', 'Main-thread task timing comes from a Chromium trace.')
  await page.goto('/tests/desktop-ui/?screen=personal-atlas&view=detailed&scenario=scale')
  await page.getByRole('tab', { name: 'My Graph', exact: true }).click()
  const atlas = page.locator('.fixture-page .atlas')
  await expect(atlas.getByRole('heading', { name: /1,?500 loaded skills/ })).toBeVisible()
  await atlas.getByRole('button', { name: 'Explore Computer Science', exact: true }).click()
  const map = atlas.locator('.atlas-map')
  await map.scrollIntoViewIfNeeded()
  await page.evaluate(() => new Promise<void>(resolve => requestAnimationFrame(() => requestAnimationFrame(() => resolve()))))
  const box = await map.boundingBox()
  if (!box) throw new Error('The atlas map has no box')
  const centre = { x: box.x + box.width / 2, y: box.y + box.height / 2 }
  const startCamera = await map.getAttribute('data-camera')
  const tasks = await mainThreadTasks(browser, page, async () => {
    // Down through the field, across into its neighbours, and back up.
    for (const [dx, dy] of [[0, -240], [0, -240], [-300, 0], [-300, 0], [300, 120], [0, 240]]) {
      await page.mouse.move(centre.x, centre.y)
      await page.mouse.down()
      await page.mouse.move(centre.x + dx!, centre.y + dy!, { steps: 24 })
      await page.mouse.up()
    }
    await page.evaluate(() => new Promise<void>(resolve => setTimeout(resolve, 250)))
  })
  expect(await map.getAttribute('data-camera')).not.toBe(startCamera)
  expect(report('pan', tasks, testInfo)).toBeLessThanOrEqual(LONG_TASK_BUDGET_MS)
})

test('a 1,500-skill Web view pans and zooms without a main-thread task over 50 ms', async ({ page, browser, browserName }, testInfo) => {
  test.skip(browserName !== 'chromium', 'Main-thread task timing comes from a Chromium trace.')
  await page.goto('/tests/desktop-ui/?screen=personal-atlas&view=web&scenario=scale')
  await page.getByRole('tab', { name: 'My Graph', exact: true }).click()
  const atlas = page.locator('.fixture-page .atlas')
  await expect(atlas.getByRole('heading', { name: /1,?500 loaded skills/ })).toBeVisible()
  const map = atlas.locator('.atlas-map.is-web')
  await expect(map.locator('.web-dot')).toHaveCount(1500)
  await map.scrollIntoViewIfNeeded()
  await page.evaluate(() => new Promise<void>(resolve => requestAnimationFrame(() => requestAnimationFrame(() => resolve()))))
  const box = await map.boundingBox()
  if (!box) throw new Error('The Web map has no box')
  // A fitted camera leaves a margin, so a corner is background: dragging it pans.
  const corner = { x: box.x + 8, y: box.y + 8 }
  const startCamera = await map.getAttribute('data-camera')
  // Clicked by position, found before tracing: a role query, or a locator's
  // checks before a click, run scripts on the page's main thread (one
  // computes the accessible name of every dot's button), which is the test's
  // work, not the app's.
  const centreOf = async (name: string) => {
    const button = (await atlas.getByRole('button', { name, exact: true }).boundingBox())!
    return { x: button.x + button.width / 2, y: button.y + button.height / 2 }
  }
  const zoomIn = await centreOf('Zoom in'), zoomOut = await centreOf('Zoom out')
  const tasks = await mainThreadTasks(browser, page, async () => {
    for (const [dx, dy] of [[200, 120], [240, 60], [120, 200]]) {
      await page.mouse.move(corner.x, corner.y)
      await page.mouse.down()
      await page.mouse.move(corner.x + dx!, corner.y + dy!, { steps: 24 })
      await page.mouse.up()
    }
    await page.mouse.click(zoomIn.x, zoomIn.y)
    await page.mouse.click(zoomOut.x, zoomOut.y)
    // Long enough for the dots' buttons to finish moving after the zoom.
    await page.evaluate(() => new Promise<void>(resolve => setTimeout(resolve, 600)))
  })
  expect(await map.getAttribute('data-camera')).not.toBe(startCamera)
  expect(report('Web pan', tasks, testInfo)).toBeLessThanOrEqual(LONG_TASK_BUDGET_MS)
})
