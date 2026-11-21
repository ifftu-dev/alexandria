import { expect, test, type Page } from '@playwright/test'
import { open } from './motionHelpers'

/** The open chapter sheet's box, from its Close chapters button. */
async function sheetBox(page: Page) {
  await open(page, '/learn/course-sorting')
  await page.getByRole('button', { name: 'Open chapters' }).click()
  const close = page.getByRole('button', { name: 'Close chapters' })
  await expect(close).toBeVisible()
  // The sheet is the scrolling card that holds the sticky header.
  const sheet = close.locator('xpath=ancestor::div[contains(@class, "overflow-y-auto")][1]')
  await expect.poll(async () => (await sheet.boundingBox())?.y).toBeLessThan(page.viewportSize()!.height)
  await page.waitForTimeout(400) // let the slide-up finish
  return (await sheet.boundingBox())!
}

test('the chapter sheet spans a phone screen', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 })
  const box = await sheetBox(page)
  expect(Math.round(box.x)).toBe(0)
  expect(Math.round(box.width)).toBe(390)
})

test('the chapter sheet is a centred card in a narrow desktop window', async ({ page }) => {
  // The Mac window's minimum width: past the phone layout, short of the chapter sidebar.
  await page.setViewportSize({ width: 900, height: 700 })
  const box = await sheetBox(page)
  expect(box.width).toBeLessThanOrEqual(28 * 16 + 1)
  expect(Math.abs(box.x + box.width / 2 - 450)).toBeLessThanOrEqual(1)
  expect(box.y + box.height).toBeLessThan(700)
})
