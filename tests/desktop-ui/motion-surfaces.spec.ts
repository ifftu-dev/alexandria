import { expect, test, type Locator } from '@playwright/test'
import { expectMotion, expectPressFeedback, indicator, motionAfter, motionOnArrival, open, pressedStyle, reachableAfter, setMode } from './motionHelpers'

for (const mode of ['System', 'Reduced', 'Off'] as const) {
  test(`press feedback reaches controls beyond the standard button (${mode})`, async ({ page }) => {
    await open(page)
    await setMode(page, mode)
    const sidebar = page.locator('aside')

    await expectPressFeedback(page, sidebar.getByRole('button', { name: 'Goals', exact: true }), mode)
    await expect(page.locator('output')).toHaveText('/goals')
    await expectPressFeedback(page, page.getByRole('radio', { name: 'Learner' }), mode)

    await page.getByRole('button', { name: 'Open search' }).click()
    const search = page.getByRole('dialog', { name: /search/i })
    await search.getByRole('textbox').fill('Sorting')
    await expectPressFeedback(page, search.getByRole('button', { name: /Sorting algorithms/ }), mode)
    await expect(page.locator('output')).toHaveText('/courses/course-sorting')

    await sidebar.getByRole('button', { name: 'Home', exact: true }).click()
    await expectPressFeedback(page, page.getByRole('button', { name: /Algorithms foundations/ }), mode)

    await page.goto('/tests/desktop-ui/?screen=motion-surfaces&path=/courses')
    await setMode(page, mode)
    await expectPressFeedback(page, page.getByRole('button', { name: /Tutorials/ }), mode)
    await expectPressFeedback(page, page.getByRole('link', { name: /Recursion in ten minutes/ }), mode)

    await page.goto('/tests/desktop-ui/?screen=motion-surfaces&path=/learn/course-sorting')
    await setMode(page, mode)
    await expectPressFeedback(page, page.getByRole('button', { name: /Insertion sort/ }), mode)
    await expect(page.getByRole('heading', { name: 'Insertion sort' })).toBeVisible()

    await page.goto('/tests/desktop-ui/?screen=motion-surfaces&path=/goals')
    await setMode(page, mode)
    await page.getByTestId('add-goal').click()
    await expectPressFeedback(page, page.getByRole('dialog').getByRole('button', { name: 'Close', exact: true }), mode)
    await expect(page.getByRole('dialog')).toHaveCount(0)
  })
}

for (const mode of ['System', 'Reduced', 'Off'] as const) {
  test(`navigation selection indicators follow the motion setting (${mode})`, async ({ page }) => {
    await open(page)
    await setMode(page, mode)
    const sidebar = page.locator('aside')
    const goals = sidebar.getByRole('button', { name: 'Goals', exact: true })
    expectMotion(await motionAfter(goals, sidebar), mode)
    await expect(goals).toHaveAttribute('aria-current', 'page')
    await expect(sidebar.getByRole('button', { name: 'Home', exact: true })).not.toHaveAttribute('aria-current', 'page')
    if (mode !== 'System') expect(await indicator(goals)).toBe('1')
    await expect.poll(() => indicator(goals)).toBe('1')

    const instructor = page.getByRole('radio', { name: 'Instructor' })
    expectMotion(await motionAfter(instructor, page.getByRole('radiogroup')), mode)
    await expect(instructor).toHaveAttribute('aria-checked', 'true')
    await expect.poll(() => indicator(instructor)).toBe('1')
    await expect(page.locator('output')).toHaveText('/instructor')

    await page.setViewportSize({ width: 390, height: 844 })
    const tabs = page.locator('nav').filter({ has: page.getByRole('button', { name: 'Live Tutoring' }) }).last()
    const tutoring = tabs.getByRole('button', { name: 'Live Tutoring' })
    expectMotion(await motionAfter(tutoring, tabs), mode)
    await expect(tutoring).toHaveAttribute('aria-current', 'page')
    if (mode !== 'System') expect(await indicator(tutoring)).toBe('1')
  })
}

for (const mode of ['System', 'Reduced', 'Off'] as const) {
  test(`search opens, brings results in and closes under the motion setting (${mode})`, async ({ page }) => {
    await open(page)
    await setMode(page, mode)
    expectMotion(await motionOnArrival(page, () => page.getByRole('button', { name: 'Open search' }).click(), '[role="dialog"]', ''), mode)
    const search = page.getByRole('dialog', { name: 'Search' })
    await expect(search).toBeVisible()
    await expect(search.getByRole('textbox')).toBeFocused()

    const entry = await motionOnArrival(page, () => search.getByRole('textbox').fill('Sort'), '[role="dialog"] button', 'Sorting algorithms')
    expectMotion(entry, mode)
    await expect(search.getByRole('button', { name: /Sorting algorithms/ })).toHaveCSS('opacity', '1')

    // Closing never holds the page: the next pointer lands at once.
    const goals = page.locator('aside').getByRole('button', { name: 'Goals', exact: true })
    expect(await reachableAfter(search.getByRole('textbox'), goals, input => {
      input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }))
    })).toBe(true)
    await expect(search).toHaveCount(0)
  })
}

for (const mode of ['System', 'Reduced', 'Off'] as const) {
  test(`catalog and Home cards enter, leave and move under the motion setting (${mode})`, async ({ page }) => {
    await open(page, '/goals')
    await setMode(page, mode)
    const sidebar = page.locator('aside')
    const entry = await motionOnArrival(page, () => sidebar.getByRole('button', { name: 'Home', exact: true }).click(), 'main a', 'Sorting algorithms')
    expectMotion(entry, mode)
    const continueCard = page.getByRole('link', { name: /Sorting algorithms/ }).first()
    await expect(continueCard).toHaveCSS('opacity', '1')
    await expect(page.getByRole('button', { name: /Algorithms foundations/ })).toHaveCSS('opacity', '1')

    await page.goto('/tests/desktop-ui/?screen=motion-surfaces&path=/courses')
    await setMode(page, mode)
    const grid = page.locator('main [data-catalog-grid]')
    await expect(grid.getByRole('link')).toHaveCount(3)
    expectMotion(await motionAfter(page.getByRole('button', { name: /Tutorials/ }), grid), mode)
    await expect(grid.getByRole('link')).toHaveCount(1)
    await expect(grid.getByRole('link', { name: /Recursion in ten minutes/ })).toHaveCSS('opacity', '1')
    expectMotion(await motionAfter(page.getByRole('button', { name: /All/ }), grid), mode)
    await expect(grid.getByRole('link')).toHaveCount(3)
  })
}

for (const mode of ['System', 'Reduced', 'Off'] as const) {
  test(`goal cards enter, and adding or removing a goal confirms itself (${mode})`, async ({ page }) => {
    await open(page)
    await setMode(page, mode)
    const entry = await motionOnArrival(page, () => page.locator('aside').getByRole('button', { name: 'Goals', exact: true }).click(), '[data-goal-card]', 'Algorithms foundations')
    expectMotion(entry, mode)
    const cards = page.locator('[data-goal-card]')
    await expect(cards).toHaveCount(2)

    const algorithms = cards.filter({ hasText: 'Algorithms foundations' })
    const remove = algorithms.getByRole('button', { name: 'Remove' })
    await expect(remove).toHaveCSS('opacity', '1')
    expectMotion(await motionAfter(remove, page.locator('[data-goal-grid]')), mode)
    await expect(page.getByRole('status').filter({ hasText: 'Removed “Algorithms foundations” from your goals.' })).toBeVisible()
    await expect(cards).toHaveCount(1)
    await expect(cards.filter({ hasText: 'Recursive thinking' })).toHaveCSS('opacity', '1')

    await page.getByTestId('add-goal').click()
    const dialog = page.getByRole('dialog')
    await dialog.getByRole('combobox').selectOption('data-engineer')
    await dialog.getByRole('button', { name: 'Set as goal' }).click()
    await expect(dialog.getByRole('status').filter({ hasText: '“Data engineer” is now one of your goals.' })).toBeVisible()
    await dialog.getByTestId('add-goal-done').click()
    await expect(cards).toHaveCount(2)
    await expect(cards.filter({ hasText: 'Data engineer' })).toHaveCSS('opacity', '1')
  })
}

/**
 * Click a lesson control and report how far the arriving lesson started from
 * its resting place along the inline axis (two frames in), and the motion it ran.
 */
async function lessonStep(action: Locator) {
  return action.evaluate(el => new Promise<{ shift: number, motion: number }>(resolve => {
    (el as HTMLElement).click()
    requestAnimationFrame(() => requestAnimationFrame(async () => {
      const body = document.querySelector('[data-lesson-body]')
      if (!body) return resolve({ shift: Number.NaN, motion: Number.NaN })
      const animations = body.getAnimations().filter(a => a.playState === 'running' && Number(a.effect?.getComputedTiming().duration) > 1)
      const start = body.getBoundingClientRect().left
      await Promise.all(animations.map(a => a.finished.catch(() => undefined)))
      resolve({ shift: Math.round(start - body.getBoundingClientRect().left), motion: animations.length })
    }))
  }))
}

for (const mode of ['System', 'Reduced', 'Off'] as const) {
  test(`lesson steps arrive from the reading direction under the motion setting (${mode})`, async ({ page }) => {
    await open(page, '/learn/course-sorting')
    await setMode(page, mode)
    await expect(page.getByRole('heading', { name: 'Bubble sort' })).toBeVisible()

    const forward = await lessonStep(page.getByRole('button', { name: /Insertion sort/ }))
    await expect(page.getByRole('heading', { name: 'Insertion sort' })).toBeVisible()
    const back = await lessonStep(page.getByRole('button', { name: 'Previous', exact: true }))
    await expect(page.getByRole('heading', { name: 'Bubble sort' })).toHaveCSS('opacity', '1')
    if (mode === 'System') {
      expect(forward.shift).toBeGreaterThan(0)
      expect(back.shift).toBeLessThan(0)
    } else {
      expect([forward, back]).toEqual([{ shift: 0, motion: 0 }, { shift: 0, motion: 0 }])
    }
  })
}

test('in a right-to-left locale the next lesson arrives from the left', async ({ page }) => {
  await page.goto('/tests/desktop-ui/?screen=motion-surfaces&locale=ur&path=/learn/course-sorting')
  await expect(page.getByRole('heading', { name: 'Bubble sort' })).toBeVisible()
  expect((await lessonStep(page.getByRole('button', { name: /Insertion sort/ }))).shift).toBeLessThan(0)
})

test('the OS reduce preference swaps press scaling and list motion for immediate changes', async ({ page }) => {
  await page.emulateMedia({ reducedMotion: 'reduce' })
  await open(page, '/courses')
  await expect(page.locator('html')).toHaveAttribute('data-motion', 'reduced')
  const chip = page.getByRole('button', { name: /Tutorials/ })
  expect(await pressedStyle(page, chip)).toEqual(expect.objectContaining({ scale: 'none', opacity: '0.72' }))
  await expect(chip).toHaveAttribute('aria-pressed', 'true')
  expect(await motionAfter(page.getByRole('button', { name: /All/ }), page.locator('[data-catalog-grid]'))).toBe(0)
})
