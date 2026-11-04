import { expect, test } from '@playwright/test'

test('fresh launch is interactive without waiting, has no idle decorative animation, and respects device settings', async ({ page }) => {
  await page.goto('/tests/desktop-ui/?screen=motion')
  const create = page.getByRole('button', { name: 'Create account', exact: true })
  await expect(create).toBeVisible()
  await create.click()
  await expect(page.locator('[data-opening]')).toHaveCount(0)
  await page.getByRole('button', { name: 'Off', exact: true }).click()
  await expect(page.locator('html')).toHaveAttribute('data-motion', 'off')
  await page.reload()
  await expect(create).toBeVisible()
  await expect(page.locator('[data-opening]')).toHaveCount(0)
  await page.getByRole('button', { name: 'System', exact: true }).click()
  await page.reload()
  await expect.poll(() => page.evaluate(() => document.getAnimations().filter(a => a.playState === 'running').length)).toBe(0)
})

test('OS reduction, profile selection and password focus work at compact widths', async ({ page }) => {
  await page.emulateMedia({ reducedMotion: 'reduce' })
  await page.setViewportSize({ width: 390, height: 844 })
  await page.goto('/tests/desktop-ui/?screen=motion&existing=1')
  await expect(page.locator('html')).toHaveAttribute('data-motion', 'reduced')
  await page.getByRole('button', { name: /Test learner/ }).click()
  await expect(page.locator('input[type="password"]')).toBeFocused()
  await page.getByRole('button', { name: 'Back', exact: true }).click()
  await expect(page.getByRole('button', { name: /Test learner/ })).toBeVisible()
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true)
})

test('slow initialization settles the opening without pretending the app is ready', async ({ page }) => {
  await page.goto('/tests/desktop-ui/?screen=motion&boot=2500')
  await expect(page.getByRole('status')).toHaveText('Initializing')
  await expect.poll(() => page.evaluate(() => document.getAnimations().filter(a => a.playState === 'running').length)).toBe(0)
  await expect(page.getByRole('button', { name: 'Create account', exact: true })).toBeVisible()
})

test('long course completion stays dismissible at compact widths without navigating', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 })
  await page.goto('/tests/desktop-ui/?screen=personal-atlas')
  const show = page.getByRole('button', { name: 'Show course completion' })
  await show.focus()
  await show.press('Enter')
  const dialog = page.getByRole('dialog')
  const close = dialog.getByRole('button', { name: 'Close', exact: true })
  await expect(close).toBeInViewport()
  await dialog.evaluate(el => { el.scrollTop = el.scrollHeight })
  await expect(close).toBeInViewport()
  await close.click()
  await expect(dialog).toHaveCount(0)
  await expect(page.locator('output')).toHaveText('/skills')
  await expect(show).toBeFocused()
  await show.click()
  await page.locator('.celebrate-backdrop').click({ position: { x: 4, y: 4 } })
  await expect(dialog).toHaveCount(0)
  await expect(page.locator('output')).toHaveText('/skills')
})

test('progress indicators keep turning slowly in Reduced and stand still beside their text in Off', async ({ page }) => {
  await page.goto('/tests/desktop-ui/?screen=motion&progress=1')
  const specimens = page.locator('[data-progress-specimens]')
  const spinner = specimens.locator('svg[data-progress]')
  const buttonSpinner = specimens.locator('button [data-progress]')
  const pending = specimens.getByRole('button', { name: 'Save draft' })
  const style = (target: typeof spinner) => target.evaluate(el => {
    const computed = getComputedStyle(el)
    return { name: computed.animationName, duration: computed.animationDuration, iterations: computed.animationIterationCount, running: el.getAnimations().some(a => a.playState === 'running') }
  })

  await page.getByRole('button', { name: 'Reduced', exact: true }).click()
  for (const target of [spinner, buttonSpinner]) {
    expect(await style(target)).toEqual(expect.objectContaining({ duration: '3s', iterations: 'infinite', running: true }))
  }
  await expect(pending).toHaveAttribute('aria-busy', 'true')
  // The fixed gap belongs to the spinner arc alone: an ordinary stroked
  // shape (here the spinner's own track) keeps a solid stroke.
  const track = specimens.locator('[role="status"] .app-spinner__track')
  const arc = specimens.locator('[role="status"] .app-spinner__arc')
  await expect(track).toHaveCSS('stroke-dasharray', 'none')
  await expect(arc).toHaveCSS('stroke-dasharray', '30px, 60px')

  await page.getByRole('button', { name: 'Off', exact: true }).click()
  for (const target of [spinner, buttonSpinner]) {
    expect(await style(target)).toEqual(expect.objectContaining({ name: 'none', running: false }))
  }
  await expect(track).toHaveCSS('stroke-dasharray', 'none')
  await expect(arc).toHaveCSS('stroke-dasharray', '30px, 60px')
  await expect(specimens.getByRole('status')).toHaveText('Saving draft')
  await expect(pending.getByText('Save draft')).toHaveCSS('opacity', '1')
  await page.getByRole('button', { name: 'System', exact: true }).click()
})

test('the OS reduce preference slows progress instead of freezing it', async ({ page }) => {
  await page.emulateMedia({ reducedMotion: 'reduce' })
  await page.goto('/tests/desktop-ui/?screen=motion&progress=1')
  const spinner = page.locator('[data-progress-specimens] svg[data-progress]')
  await expect.poll(() => spinner.evaluate(el => [getComputedStyle(el).animationDuration, el.getAnimations().some(a => a.playState === 'running')])).toEqual(['3s', true])
})
