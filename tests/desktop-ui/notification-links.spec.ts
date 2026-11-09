import { expect, test, type Page } from '@playwright/test'

interface NotificationLinks {
  calls: string[]
  path: () => string
  openLink: (url: string) => Promise<void>
}

async function fixture(page: Page) {
  await page.goto('/tests/desktop-ui/?screen=notification-links')
  await expect(page.getByText('Fixture start')).toBeVisible()
}

const open = (page: Page, url: string) =>
  page.evaluate((link) => (window as unknown as { notificationLinks: NotificationLinks }).notificationLinks.openLink(link), url)
const path = (page: Page) =>
  page.evaluate(() => (window as unknown as { notificationLinks: NotificationLinks }).notificationLinks.path())
const calls = (page: Page) =>
  page.evaluate(() => (window as unknown as { notificationLinks: NotificationLinks }).notificationLinks.calls)

test('the retired public notification links neither navigate nor mark anything read', async ({ page }) => {
  await fixture(page)
  for (const link of [
    'alexandria://notifications',
    'alexandria://notification/n-1',
    'https://alexandria.ifftu.dev/notifications',
    'https://alexandria.ifftu.dev/notification/n-1',
  ]) {
    await open(page, link)
    expect(await path(page), link).toBe('/fixture-start')
  }
  await expect(page.getByText('Fixture start')).toBeVisible()
  expect(await calls(page)).not.toContain('notifications_mark_read')
})

test('the inbox does not open a row named in its query', async ({ page }) => {
  await fixture(page)
  // The generic route link can still reach the inbox, but a query no longer
  // opens, marks read or follows a row.
  await open(page, 'alexandria://open?route=/notifications?open=n-1')
  await expect(page.getByRole('heading', { name: 'Notifications' })).toBeVisible()
  await expect(page.getByText('New offer')).toBeVisible()
  await page.waitForTimeout(250)
  expect(await path(page)).toMatch(/^\/notifications/)
  expect(await calls(page)).not.toContain('notifications_mark_read')
})
