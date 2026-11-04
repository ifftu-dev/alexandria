import { expect, test, type Locator, type Page } from '@playwright/test'

type Mode = 'System' | 'Reduced' | 'Off'

async function open(page: Page, path = '/home') {
  await page.goto(`/tests/desktop-ui/?screen=motion-surfaces&path=${encodeURIComponent(path)}`)
  await expect(page.locator('output')).toHaveText(path)
}

async function setMode(page: Page, mode: Mode) {
  await page.getByRole('button', { name: mode, exact: true }).click()
  await expect(page.locator('html')).toHaveAttribute('data-motion', mode === 'System' ? 'full' : mode.toLowerCase())
}

/** Hold the pointer on a control and read its settled pressed style, then release. */
async function pressedStyle(page: Page, target: Locator) {
  await target.scrollIntoViewIfNeeded()
  const box = (await target.boundingBox())!
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2)
  await page.mouse.down()
  let style = { scale: '', opacity: '', pointerEvents: '' }
  await expect.poll(async () => {
    const next = await target.evaluate(el => {
      const computed = getComputedStyle(el)
      return { scale: computed.scale, opacity: computed.opacity, pointerEvents: computed.pointerEvents }
    })
    const settled = JSON.stringify(next) === JSON.stringify(style)
    style = next
    return settled
  }).toBe(true)
  await page.mouse.up()
  return style
}

/** Full motion presses inward; Reduced and Off dim in place instead. */
async function expectPressFeedback(page: Page, target: Locator, mode: Mode) {
  const style = await pressedStyle(page, target)
  expect(style.pointerEvents).not.toBe('none')
  if (mode === 'System') {
    expect(Number(style.scale)).toBeLessThan(1)
  } else {
    expect(style.scale).toBe('none')
    expect(Number(style.opacity)).toBeLessThan(1)
  }
}

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

/**
 * Click `action` through the DOM and, two frames later, report the motion the
 * change started inside `surface` (the whole document by default): every
 * running animation or transition longer than the 0.01 ms Reduced/Off floor.
 */
async function motionAfter(action: Locator, surface?: Locator): Promise<number> {
  const scope = surface ? await surface.elementHandle() : null
  return action.evaluate((el, scoped) => new Promise<number>(resolve => {
    (el as HTMLElement).click()
    requestAnimationFrame(() => requestAnimationFrame(() => {
      const animations = scoped ? scoped.getAnimations({ subtree: true }) : document.getAnimations()
      resolve(animations.filter(a => a.playState === 'running' && Number(a.effect?.getComputedTiming().duration) > 1).length)
    }))
  }), scope)
}

/** Full motion moves; Reduced and Off change state with no motion at all. */
function expectMotion(count: number, mode: Mode) {
  if (mode === 'System') expect(count).toBeGreaterThan(0)
  else expect(count).toBe(0)
}

/** The selection indicator's opacity on the next frame. */
const indicator = (target: Locator) => target.evaluate(el => new Promise<string>(resolve => {
  requestAnimationFrame(() => resolve(getComputedStyle(el, '::before').opacity))
}))

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

/**
 * Start `act`, and once an element matching `selector` with `text` arrives,
 * report (two frames later) the motion running inside `scope`.
 */
async function motionOnArrival(page: Page, act: () => Promise<void>, selector: string, text: string, scope: string): Promise<number> {
  const arrival = page.evaluate(([selector, text, scope]) => new Promise<number>(resolve => {
    const arrived = () => [...document.querySelectorAll(selector)].some(el => el.textContent?.includes(text))
    const report = () => requestAnimationFrame(() => requestAnimationFrame(() => {
      const root = document.querySelector(scope)
      const animations = root ? root.getAnimations({ subtree: true }) : []
      resolve(animations.filter(a => a.playState === 'running' && Number(a.effect?.getComputedTiming().duration) > 1).length)
    }))
    if (arrived()) return report()
    const observer = new MutationObserver(() => { if (arrived()) { observer.disconnect(); report() } })
    observer.observe(document.body, { subtree: true, childList: true, characterData: true })
  }), [selector, text, scope] as const)
  await act()
  return arrival
}

/** Whether a pointer at the centre of `target` reaches it, one frame after `act` runs in the page. */
async function reachableAfter(actor: Locator, target: Locator, act: (el: Element) => void): Promise<boolean> {
  const handle = await target.elementHandle()
  return actor.evaluate((el, [target, source]) => new Promise<boolean>(resolve => {
    new Function('el', `(${source})(el)`)(el)
    requestAnimationFrame(() => {
      const box = target!.getBoundingClientRect()
      resolve(target!.contains(document.elementFromPoint(box.x + box.width / 2, box.y + box.height / 2)))
    })
  }), [handle, act.toString()] as const)
}

for (const mode of ['System', 'Reduced', 'Off'] as const) {
  test(`search opens, brings results in and closes under the motion setting (${mode})`, async ({ page }) => {
    await open(page)
    await setMode(page, mode)
    expectMotion(await motionAfter(page.getByRole('button', { name: 'Open search' })), mode)
    const search = page.getByRole('dialog', { name: 'Search' })
    await expect(search).toBeVisible()
    await expect(search.getByRole('textbox')).toBeFocused()

    const entry = await motionOnArrival(page, () => search.getByRole('textbox').fill('Sort'), '[role="dialog"] button', 'Sorting algorithms', '[role="dialog"]')
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
