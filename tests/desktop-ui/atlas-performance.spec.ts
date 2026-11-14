import { expect, test } from '@playwright/test'

declare global { interface Window { atlasLongTasks?: number[] } }

/** The longest main-thread task panning may cause. */
const LONG_TASK_BUDGET_MS = 50

test('a 1,500-skill atlas pans without a main-thread task over 50 ms', async ({ page, browserName }, testInfo) => {
  test.skip(browserName !== 'chromium', 'The Long Tasks API (PerformanceObserver "longtask") exists only in Chromium.')
  await page.goto('/tests/desktop-ui/?screen=personal-atlas&view=detailed&scenario=scale')
  await page.getByRole('tab', { name: 'My Graph', exact: true }).click()
  const atlas = page.locator('.fixture-page .atlas')
  await expect(atlas.getByRole('heading', { name: /1,?500 loaded skills/ })).toBeVisible()
  await atlas.getByRole('button', { name: 'Explore Computer Science', exact: true }).click()
  const map = atlas.locator('.atlas-map')
  await map.scrollIntoViewIfNeeded()
  await page.evaluate(() => new Promise<void>(resolve => requestAnimationFrame(() => requestAnimationFrame(() => resolve()))))
  await page.evaluate(() => {
    window.atlasLongTasks = []
    new PerformanceObserver(list => { for (const entry of list.getEntries()) window.atlasLongTasks!.push(entry.duration) })
      .observe({ type: 'longtask' })
  })
  const box = await map.boundingBox()
  if (!box) throw new Error('The atlas map has no box')
  const centre = { x: box.x + box.width / 2, y: box.y + box.height / 2 }
  const startCamera = await map.getAttribute('data-camera')
  // Down through the field, across into its neighbours, and back up.
  for (const [dx, dy] of [[0, -240], [0, -240], [-300, 0], [-300, 0], [300, 120], [0, 240]]) {
    await page.mouse.move(centre.x, centre.y)
    await page.mouse.down()
    await page.mouse.move(centre.x + dx!, centre.y + dy!, { steps: 24 })
    await page.mouse.up()
  }
  await page.evaluate(() => new Promise<void>(resolve => setTimeout(resolve, 250)))
  expect(await map.getAttribute('data-camera')).not.toBe(startCamera)
  const longTasks = await page.evaluate(() => window.atlasLongTasks ?? [])
  const longest = Math.max(0, ...longTasks)
  testInfo.annotations.push({ type: 'longest pan task (ms)', description: longest.toFixed(1) })
  console.log(`atlas pan: ${longTasks.length} long tasks, longest ${longest.toFixed(1)} ms`)
  expect(longest).toBeLessThanOrEqual(LONG_TASK_BUDGET_MS)
})
