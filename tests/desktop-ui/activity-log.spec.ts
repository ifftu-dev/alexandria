import { readFile } from 'node:fs/promises'
import { expect, test, type Page } from '@playwright/test'
import { FIXTURE_EXPORT } from './activityLogBackend'

interface Call { command: string, args?: Record<string, unknown> }

async function open(page: Page, query = '') {
  await page.goto(`/tests/desktop-ui/?screen=activity-log${query}`)
  await expect(page.getByRole('heading', { name: 'Activity log' })).toBeVisible()
}

const calls = (page: Page) =>
  page.evaluate(() => (window as unknown as { activityLogFixture: { calls: Call[] } }).activityLogFixture.calls)

const rows = (page: Page) => page.getByRole('list', { name: 'Learning records' }).getByRole('listitem')

test('the log lists every record with what it is about and shows its size limit', async ({ page }) => {
  await open(page)

  await expect(rows(page)).toHaveCount(4)
  const first = rows(page).nth(0)
  await expect(first).toContainText('Passed')
  await expect(first).toContainText('Check')
  await expect(first).toContainText('Warehouse safety')
  await expect(first).toContainText('Score 80%')
  await expect(rows(page).nth(1)).toContainText('Response')
  await expect(rows(page).nth(2)).toContainText('50% of the course')
  await expect(rows(page).nth(2)).toContainText('Summary')
  // A course no longer on this device is still named by its identifier.
  await expect(rows(page).nth(3)).toContainText('Registered')

  const limit = page.getByRole('region', { name: 'Log size' })
  await expect(limit).toContainText('4 records')
  await expect(limit).toContainText('1.9 KB of 16 MB')
  await expect(limit.getByRole('progressbar')).toHaveAttribute('aria-valuemax', String(16 * 1024 * 1024))
})

test('export saves the statement file and its signature unchanged', async ({ page }) => {
  await open(page)

  const downloads: { name: string, path: string }[] = []
  page.on('download', async (download) => {
    downloads.push({ name: download.suggestedFilename(), path: await download.path() })
  })
  await page.getByRole('button', { name: 'Export' }).click()

  await expect(page.getByRole('status')).toContainText('Exported 4 records with your signature')
  await expect.poll(() => downloads.length).toBe(2)
  const statements = downloads.find(d => d.name.endsWith('.json'))
  const signature = downloads.find(d => d.name.endsWith('.json.sig'))
  expect(statements?.name).toMatch(/^alexandria-activity-log-\d{4}-\d{2}-\d{2}\.json$/)
  expect(signature?.name).toBe(`${statements?.name}.sig`)
  expect(await readFile(statements?.path ?? '', 'utf8')).toBe(FIXTURE_EXPORT.statements)
  expect(await readFile(signature?.path ?? '', 'utf8')).toBe(FIXTURE_EXPORT.signature)
})

test('deleting one record asks first, says what happens to unsent records, and removes it', async ({ page }) => {
  await open(page)

  await rows(page).nth(1).getByRole('button', { name: 'Delete this record' }).click()
  const dialog = page.getByRole('dialog', { name: 'Delete this record?' })
  await expect(dialog).toContainText('A record waiting to be sent is not sent')
  await dialog.getByRole('button', { name: 'Delete' }).click()

  await expect(rows(page)).toHaveCount(3)
  await expect(page.getByRole('list', { name: 'Learning records' })).not.toContainText('Response')
  await expect(page.getByRole('region', { name: 'Log size' })).toContainText('3 records')
  expect((await calls(page)).find(c => c.command === 'learning_records_delete')?.args).toEqual({ ids: ['record-1'] })
})

test('cancelling a delete keeps the record', async ({ page }) => {
  await open(page)

  await rows(page).nth(0).getByRole('button', { name: 'Delete this record' }).click()
  await page.getByRole('dialog').getByRole('button', { name: 'Cancel' }).click()

  await expect(rows(page)).toHaveCount(4)
  expect((await calls(page)).map(c => c.command)).not.toContain('learning_records_delete')
})

test('a selection of records can be deleted together', async ({ page }) => {
  await open(page)
  const deleteSelected = page.getByRole('button', { name: /Delete selected/ })
  await expect(deleteSelected).toBeDisabled()

  await rows(page).nth(0).getByRole('checkbox').check()
  await rows(page).nth(2).getByRole('checkbox').check()
  await expect(deleteSelected).toHaveText('Delete selected (2)')
  await deleteSelected.click()
  await page.getByRole('dialog', { name: 'Delete 2 records?' }).getByRole('button', { name: 'Delete' }).click()

  await expect(rows(page)).toHaveCount(2)
  await expect(deleteSelected).toBeDisabled()
  expect((await calls(page)).find(c => c.command === 'learning_records_delete')?.args).toEqual({ ids: ['record-0', 'record-2'] })
})

test('everything can be deleted, leaving an empty log', async ({ page }) => {
  await open(page)

  await page.getByRole('button', { name: 'Delete everything' }).click()
  await page.getByRole('dialog', { name: 'Delete every record?' }).getByRole('button', { name: 'Delete everything' }).click()

  await expect(page.getByText('No learning records yet')).toBeVisible()
  await expect(page.getByRole('dialog')).toHaveCount(0)
  await expect(page.getByRole('region', { name: 'Log size' })).toContainText('0 records')
  await expect(page.getByRole('button', { name: 'Export' })).toBeEnabled()
  await expect(page.getByRole('button', { name: 'Delete everything' })).toBeDisabled()
  expect((await calls(page)).map(c => c.command)).toContain('learning_records_delete_all')
})

test('a long log is read a page at a time', async ({ page }) => {
  await open(page, '&records=130')

  await expect(rows(page)).toHaveCount(100)
  await page.getByRole('button', { name: 'Show more' }).click()

  await expect(rows(page)).toHaveCount(130)
  await expect(page.getByRole('button', { name: 'Show more' })).toHaveCount(0)
})
