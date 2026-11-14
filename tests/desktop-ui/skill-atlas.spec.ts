import { expect, test, type Page } from '@playwright/test'

for (const theme of ['light', 'dark']) {
  test(`${theme} atlas presents the whole taxonomy and restores explicit exploration`, async ({ page }, testInfo) => {
    await page.goto(`/tests/desktop-ui/?screen=skill-atlas&view=detailed&theme=${theme}`)
    await expect(page.getByRole('heading', { name: '35 earned · 68 Ready · 103 loaded skills' })).toBeVisible()
    const rows = page.getByRole('list', { name: 'Full-label skill list' }).getByRole('button')
    await expect(page.locator('.atlas-list summary')).toContainText('103 matching skills')
    await expect(rows).toHaveCount(50)
    await page.getByRole('button', { name: 'Show 50 more skills', exact: true }).click()
    await expect(rows.nth(50)).toBeFocused()
    await page.getByRole('button', { name: 'Show 3 more skills', exact: true }).click()
    await expect(rows).toHaveCount(103)
    await page.screenshot({ path: testInfo.outputPath('overview.png'), fullPage: true })
    await page.getByRole('button', { name: 'Explore Computer Science', exact: true }).click()
    await page.getByRole('searchbox', { name: 'Search all skills' }).fill('Big-O')
    const result = page.getByRole('list', { name: 'Full-label skill list' }).getByRole('button').first()
    await result.focus()
    await page.keyboard.press('Enter')
    const inspector = page.getByRole('complementary', { name: 'Skill inspector' })
    await expect(inspector.getByRole('heading', { name: 'Big-O Analysis' })).toBeVisible()
    await expect(page.locator('output')).toHaveText('')
    await page.getByRole('button', { name: 'Zoom in', exact: true }).click()
    await page.getByRole('button', { name: 'Focus dependents', exact: true }).click()
    await inspector.getByRole('button', { name: 'Collapse inspector', exact: true }).click()
    await expect(page.getByRole('status').filter({ hasText: 'Showing dependents of Big-O Analysis · 1 step' })).toBeVisible()
    await page.screenshot({ path: testInfo.outputPath('selected.png'), fullPage: true })
    await inspector.getByRole('button', { name: 'Expand inspector', exact: true }).click()
    await page.getByRole('button', { name: 'Remount explorer', exact: true }).click()
    await expect(page.getByRole('searchbox')).toHaveValue('Big-O')
    await expect(page.getByRole('button', { name: 'Focus dependents', exact: true })).toHaveAttribute('aria-pressed', 'true')
    await inspector.getByRole('button', { name: 'Open skill', exact: true }).click()
    await expect(page.locator('output')).toHaveText('open-skill: skill_big_o')
    await inspector.getByRole('button', { name: 'Find learning opportunities', exact: true }).click()
    await expect(page.locator('output')).toHaveText('learn-skill: skill_big_o')
    expect(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth)).toBe(false)
  })
}

test('narrow atlas supports a collapsible inspector, full long labels and reduced motion', async ({ page }, testInfo) => {
  await page.setViewportSize({ width: 390, height: 844 })
  await page.emulateMedia({ reducedMotion: 'reduce' })
  await page.goto('/tests/desktop-ui/?screen=skill-atlas&view=detailed&theme=dark&scenario=stress')
  await page.getByRole('searchbox').fill('stewardship')
  await page.getByRole('list', { name: 'Full-label skill list' }).getByRole('button').click()
  const inspector = page.getByRole('complementary', { name: 'Skill inspector' })
  await expect(inspector.getByRole('heading', { name: /A very long skill label/ })).toBeVisible()
  await expect(inspector.getByText('Earned level unavailable', { exact: true })).toBeVisible()
  await expect(inspector.getByText('An unloaded prerequisite · Not loaded', { exact: true })).toBeVisible()
  await page.screenshot({ path: testInfo.outputPath('mobile-expanded.png'), fullPage: true })
  await inspector.getByRole('button', { name: 'Collapse inspector', exact: true }).click()
  await expect(inspector.getByRole('button', { name: 'Expand inspector', exact: true })).toHaveAttribute('aria-expanded', 'false')
  await page.screenshot({ path: testInfo.outputPath('mobile-collapsed.png'), fullPage: true })
  expect(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth)).toBe(false)
})

test('Urdu RTL retains full earned meaning on narrow screens', async ({ page }, testInfo) => {
  await page.setViewportSize({ width: 390, height: 844 })
  await page.goto('/tests/desktop-ui/?screen=skill-atlas&view=detailed&theme=light&scenario=stress&locale=ur')
  await page.getByRole('searchbox').fill('stewardship')
  const result = page.getByRole('list').getByRole('button').filter({ hasText: 'A very long skill label' })
  await result.click()
  await expect(page.locator('html')).toHaveAttribute('dir', 'rtl')
  const atlasBounds = await page.getByRole('region').boundingBox()
  expect(atlasBounds).not.toBeNull()
  expect((atlasBounds?.x ?? 0) + (atlasBounds?.width ?? 0)).toBeLessThanOrEqual(390)
  expect(atlasBounds?.x).toBeGreaterThanOrEqual(0)
  await expect(page.getByRole('complementary').getByRole('heading', { name: /A very long skill label/ })).toBeVisible()
  expect(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth)).toBe(false)
  await page.screenshot({ path: testInfo.outputPath('mobile-rtl.png'), fullPage: true })
})

test('new evidence has no visible-duration animation with system reduced motion', async ({ page }) => {
  await page.emulateMedia({ reducedMotion: 'reduce' })
  await page.goto('/tests/desktop-ui/?screen=skill-atlas&view=detailed&theme=dark')
  await page.getByRole('button', { name: 'Explore Computer Science', exact: true }).click()
  await page.getByRole('button', { name: 'Simulate evidence update', exact: true }).click()
  await expect(page.getByRole('status').filter({ hasText: 'New evidence highlighted' })).toBeVisible()
  // Reduced motion retains 0.01ms CSS transitions for lifecycle events. A
  // browser may expose them until its next frame; reject visible-duration
  // motion immediately instead of racing that frame or waiting out a defect.
  expect(await page.getByRole('region', { name: 'Skill atlas' }).evaluate(element =>
    element.getAnimations({ subtree: true }).filter(animation =>
      Number(animation.effect?.getComputedTiming().duration) > 0.01,
    ).length,
  )).toBe(0)
  await expect(page.getByRole('status').filter({ hasText: 'New evidence highlighted' })).toHaveCount(0)
})

test('only skills whose prerequisites are all earned are emphasised as ready next', async ({ page }, testInfo) => {
  await page.goto('/tests/desktop-ui/?screen=skill-atlas&view=detailed&theme=light')
  const list = page.getByRole('list', { name: 'Full-label skill list' })
  await page.getByRole('searchbox', { name: 'Search all skills' }).fill('Algorithms')
  // Sorting needs Arrays and Big-O, both earned; Greedy needs Sorting too; Graph needs neither of its two.
  const sorting = list.getByRole('button', { name: 'Sorting Algorithms, not yet earned, ready next, learning-outcome Bloom level 3 Apply', exact: true })
  await expect(sorting).toContainText('Ready next')
  await expect(list.getByRole('button', { name: 'Greedy Algorithms, not yet earned, Ready, learning-outcome Bloom level 4 Analyze', exact: true })).toBeVisible()
  await expect(list.getByRole('button', { name: 'Graph Algorithms, not yet earned, Ready, learning-outcome Bloom level 3 Apply', exact: true })).toBeVisible()
  await page.getByRole('searchbox', { name: 'Search all skills' }).fill('Python')
  // No prerequisites: Ready, never ready next.
  await expect(list.getByRole('button', { name: 'Python, not yet earned, Ready, learning-outcome Bloom level 3 Apply', exact: true })).toBeVisible()
  await page.getByRole('searchbox', { name: 'Search all skills' }).fill('Sorting')
  await sorting.click()
  const map = page.getByRole('group', { name: 'Skill map' })
  const node = map.getByRole('button', { name: 'Sorting Algorithms, not yet earned, ready next, learning-outcome Bloom level 3 Apply', exact: true })
  await expect(node).toBeVisible()
  await expect(node).toContainText('Ready next')
  const greedy = map.locator('.map-node').filter({ hasText: 'Greedy Algorithms' })
  const style = (locator: typeof node) => locator.evaluate(element => {
    const computed = getComputedStyle(element)
    return { border: computed.borderInlineStartWidth, background: computed.backgroundColor }
  })
  const [next, ready] = [await style(node), await style(greedy)]
  expect(next.border).toBe('7px')
  expect(ready.border).toBe('4px')
  expect(next.background).not.toBe(ready.background)
  await expect(page.getByRole('complementary', { name: 'Skill inspector' }).locator('.inspector-status')).toHaveText('◎ Ready next')
  await page.screenshot({ path: testInfo.outputPath('ready-next.png'), fullPage: true })
})

/** The fixture scrolls inside its own frame, so a full-page shot first grows the viewport to the content. */
async function shootWhole(page: Page, path: string) {
  const viewport = page.viewportSize()!
  const height = await page.locator('.fixture-atlas').evaluate(element => element.scrollHeight)
  await page.setViewportSize({ width: viewport.width, height: Math.min(height + 56, 8000) })
  await page.screenshot({ animations: 'disabled', path })
  await page.setViewportSize(viewport)
}

/** Disc centres and diameters, in page pixels. */
async function discs(page: Page) {
  return page.getByRole('group', { name: 'Skill map' }).locator('.node-disc').evaluateAll(elements => elements.map(element => {
    const box = element.getBoundingClientRect()
    return { x: box.x + box.width / 2, y: box.y + box.height / 2, size: box.width, left: box.left, right: box.right }
  }))
}

/** How many pairs of discs intersect. */
function overlaps(points: Awaited<ReturnType<typeof discs>>): number {
  let count = 0
  for (const [index, a] of points.entries()) {
    for (const b of points.slice(index + 1)) if (Math.hypot(a.x - b.x, a.y - b.y) < (a.size + b.size) / 2 - 0.5) count++
  }
  return count
}

for (const theme of ['light', 'dark']) {
  for (const width of [1280, 390]) {
    test(`${theme} Simple view at ${width}px fits the width with discs apart and keeps selection across a switch`, async ({ page }, testInfo) => {
      const errors: string[] = []
      page.on('pageerror', error => errors.push(error.message))
      await page.setViewportSize({ width, height: 844 })
      await page.goto(`/tests/desktop-ui/?screen=skill-atlas&view=simple&theme=${theme}`)
      const atlas = page.getByRole('region', { name: 'Skill atlas' })
      await expect(atlas.getByRole('button', { name: 'Simple', exact: true })).toHaveAttribute('aria-pressed', 'true')
      await expect(atlas.getByRole('searchbox')).toHaveCount(0)
      await expect(atlas.getByRole('button', { name: 'Zoom in', exact: true })).toHaveCount(0)
      const map = atlas.getByRole('group', { name: 'Skill map' })
      await expect(map.getByRole('button')).toHaveCount(103)
      await expect(atlas.getByText('Ringed: Ready next', { exact: true })).toBeVisible()
      const points = await discs(page)
      expect(points).toHaveLength(103)
      expect(overlaps(points)).toBe(0)
      for (const point of points) {
        expect(point.left).toBeGreaterThanOrEqual(0)
        expect(point.right).toBeLessThanOrEqual(width)
      }
      expect(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth)).toBe(false)
      await shootWhole(page, testInfo.outputPath(`simple-${theme}-${width}.png`))

      const sorting = map.getByRole('button', { name: 'Sorting Algorithms, not yet earned, ready next, learning-outcome Bloom level 3 Apply', exact: true })
      await sorting.click()
      const inspector = page.getByRole('complementary', { name: 'Skill inspector' })
      await expect(inspector.getByRole('heading', { name: 'Sorting Algorithms' })).toBeVisible()
      // Skills outside its one-hop neighbourhood dim.
      expect(await map.locator('.map-node.is-dimmed').count()).toBeGreaterThan(50)
      await shootWhole(page, testInfo.outputPath(`simple-${theme}-${width}-selected.png`))
      await atlas.getByRole('button', { name: 'Detailed', exact: true }).click()
      await expect(atlas.getByRole('status').filter({ hasText: 'Showing the Detailed view' })).toBeAttached()
      await expect(atlas.getByRole('searchbox')).toBeVisible()
      await expect(inspector.getByRole('heading', { name: 'Sorting Algorithms' })).toBeVisible()
      await atlas.getByRole('button', { name: 'Simple', exact: true }).click()
      await expect(atlas.getByRole('searchbox')).toHaveCount(0)
      await expect(sorting).toHaveAttribute('aria-pressed', 'true')
      expect(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth)).toBe(false)
      expect(errors).toEqual([])
    })
  }
}

for (const width of [1280, 390]) {
  test(`stress taxonomy Simple view at ${width}px keeps discs apart and long names inside the map`, async ({ page }, testInfo) => {
    await page.setViewportSize({ width, height: 844 })
    await page.goto('/tests/desktop-ui/?screen=skill-atlas&view=simple&theme=dark&scenario=stress')
    const map = page.getByRole('group', { name: 'Skill map' })
    await expect(map.getByRole('button')).toHaveCount(104)
    expect(overlaps(await discs(page))).toBe(0)
    const mapBox = (await map.boundingBox())!
    const label = (await map.getByRole('button', { name: /A very long skill label/ }).locator('.node-label').boundingBox())!
    expect(label.x).toBeGreaterThanOrEqual(mapBox.x - 1)
    expect(label.x + label.width).toBeLessThanOrEqual(mapBox.x + mapBox.width + 1)
    expect(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth)).toBe(false)
    await shootWhole(page, testInfo.outputPath(`simple-stress-${width}.png`))
  })
}
