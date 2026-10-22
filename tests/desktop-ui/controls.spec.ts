import { expect, test } from '@playwright/test'
import { PNG } from 'pngjs'

function luminance(rgb: number[]): number {
  const linear = rgb.map((value) => {
    const channel = value / 255
    return channel <= 0.04045 ? channel / 12.92 : ((channel + 0.055) / 1.055) ** 2.4
  })
  return (linear[0] ?? 0) * 0.2126 + (linear[1] ?? 0) * 0.7152 + (linear[2] ?? 0) * 0.0722
}

for (const theme of ['light', 'dark']) {
  test(`${theme} form controls stay legible when the OS uses the opposite theme`, async ({ page }) => {
    await page.emulateMedia({ colorScheme: theme === 'dark' ? 'light' : 'dark' })
    await page.goto(`/tests/desktop-ui/?theme=${theme}`)
    const select = page.getByRole('combobox')
    await expect(select).toBeVisible()
    const foreground = await select.evaluate((element) => getComputedStyle(element).color)
    const pixels = PNG.sync.read(await select.screenshot())
    const offset = (Math.floor(pixels.height / 2) * pixels.width + Math.floor(pixels.width * 0.75)) * 4
    const background = [pixels.data[offset] ?? 0, pixels.data[offset + 1] ?? 0, pixels.data[offset + 2] ?? 0]
    const text = (foreground.match(/[\d.]+/g) ?? []).slice(0, 3).map(Number)
    const light = Math.max(luminance(text), luminance(background))
    const dark = Math.min(luminance(text), luminance(background))
    expect((light + 0.05) / (dark + 0.05)).toBeGreaterThanOrEqual(4.5)
    await select.selectOption('software-engineer')
    await expect(select).toHaveValue('software-engineer')
    await select.focus()
    await page.keyboard.press('Tab')
    await expect(page.getByRole('button', { name: 'Set as goal' })).toBeFocused()
    await expect(page.getByLabel('Invalid name')).toHaveAccessibleDescription('Enter a display name')
  })
}

test('goal picker fits a narrow tiled desktop window', async ({ page }) => {
  await page.setViewportSize({ width: 420, height: 510 })
  await page.goto('/tests/desktop-ui/?theme=dark')
  await expect(page.getByRole('combobox')).toBeVisible()
  const overflow = await page.evaluate(() => document.documentElement.scrollWidth > innerWidth)
  expect(overflow).toBe(false)
})

test('a long dialog fits a short window and keeps its actions reachable', async ({ page }) => {
  await page.setViewportSize({ width: 900, height: 600 })
  await page.goto('/tests/desktop-ui/?theme=dark')
  await page.getByRole('button', { name: 'Open long dialog' }).focus()
  await page.keyboard.press('Enter')
  const dialog = page.getByRole('dialog', { name: 'Long settings dialog' })
  await expect(dialog).toBeVisible()
  const bounds = await dialog.boundingBox()
  expect(bounds).not.toBeNull()
  expect(bounds?.y).toBeGreaterThanOrEqual(0)
  expect((bounds?.y ?? 0) + (bounds?.height ?? 0)).toBeLessThanOrEqual(600)
  await expect(dialog.getByRole('button', { name: 'Save settings' })).toBeInViewport()
  await expect(page.getByLabel('Dialog display name')).toBeFocused()
  await page.keyboard.press('Shift+Tab')
  await expect(dialog.getByRole('button', { name: 'Close' })).toBeFocused()
  await page.keyboard.press('Shift+Tab')
  await expect(dialog.getByRole('button', { name: 'Save settings' })).toBeFocused()
  await page.keyboard.press('Tab')
  await expect(dialog.getByRole('button', { name: 'Close' })).toBeFocused()
  await page.keyboard.press('Escape')
  await expect(dialog).not.toBeVisible()
  await expect(page.getByRole('button', { name: 'Open long dialog' })).toBeFocused()
  await expect(page.locator('body')).not.toHaveCSS('overflow', 'hidden')
})

test('Escape closes only the top dialog and preserves the outer scroll lock', async ({ page }) => {
  await page.goto('/tests/desktop-ui/')
  await page.getByRole('button', { name: 'Open long dialog' }).click()
  await page.getByRole('button', { name: 'Open nested dialog' }).focus()
  await page.keyboard.press('Enter')
  await expect(page.getByRole('dialog', { name: 'Nested dialog', exact: true })).toBeVisible()
  await page.keyboard.press('Escape')
  await expect(page.getByRole('dialog', { name: 'Nested dialog', exact: true })).not.toBeVisible()
  await expect(page.getByRole('dialog', { name: 'Long settings dialog' })).toBeVisible()
  await expect(page.getByRole('button', { name: 'Open nested dialog' })).toBeFocused()
  await expect(page.locator('body')).toHaveCSS('overflow', 'hidden')
  await page.keyboard.press('Escape')
  await expect(page.getByRole('dialog')).not.toBeVisible()
  await expect(page.locator('body')).not.toHaveCSS('overflow', 'hidden')
})

test('a dialog covers compact navigation and its backdrop dismisses it', async ({ page }) => {
  await page.setViewportSize({ width: 420, height: 600 })
  await page.goto('/tests/desktop-ui/')
  await page.getByRole('button', { name: 'Open long dialog' }).click()
  const dialog = page.getByRole('dialog', { name: 'Long settings dialog' })
  await expect(dialog).toBeVisible()
  const navigationCovered = await page.getByRole('navigation').evaluate((nav) => {
    const bounds = nav.getBoundingClientRect()
    const top = document.elementFromPoint(bounds.x + bounds.width / 2, bounds.y + bounds.height / 2)
    return Boolean(top?.closest('[data-backdrop]'))
  })
  expect(navigationCovered).toBe(true)
  await dialog.getByText('Setting 1:', { exact: false }).click()
  await expect(dialog).toBeVisible()
  await page.locator('[data-backdrop]').click({ position: { x: 4, y: 4 } })
  await expect(dialog).not.toBeVisible()
})
