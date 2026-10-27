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
