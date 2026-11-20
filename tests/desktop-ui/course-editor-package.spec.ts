import { expect, test, type Page } from '@playwright/test'
import { servePackageOrigin } from './packageOrigin'

// Adding a standards package in the course editor, at the interface seam:
// the real composer with a fake host. The file picker answers with a path
// the fake host knows; a preview launches the player fixture package from a
// package origin served here.

const GOOD_PACKAGE = '/packages/workshop-safety.zip'

interface CourseEditorFixture {
  control: { nextPath: string | null, refuseUpdate: boolean }
  called: (command: string) => { command: string, args: unknown }[]
}

const pickNext = (page: Page, path: string) =>
  page.evaluate((value) => { (window as unknown as { courseEditorFixture: CourseEditorFixture }).courseEditorFixture.control.nextPath = value }, path)
const refuseUpdates = (page: Page) =>
  page.evaluate(() => { (window as unknown as { courseEditorFixture: CourseEditorFixture }).courseEditorFixture.control.refuseUpdate = true })
const called = (page: Page, command: string) =>
  page.evaluate((name) => (window as unknown as { courseEditorFixture: CourseEditorFixture }).courseEditorFixture.called(name), command)

async function open(page: Page) {
  const served = await servePackageOrigin(page)
  await page.goto('/tests/desktop-ui/?screen=course-editor')
  await expect(page.locator('[data-outline-element]')).toHaveCount(2)
  return served
}

/** Opens the outline's add-element form with the package type chosen. */
async function startAddingPackage(page: Page) {
  await page.getByRole('button', { name: '+ Add element' }).click()
  await page.locator('select').first().selectOption('package')
  const picker = page.locator('[data-package-picker]')
  await expect(picker).toBeVisible()
  return picker
}

test('the editor shows a package\'s title, standard, edition and lessons before it is added', async ({ page }) => {
  await open(page)
  const picker = await startAddingPackage(page)

  await pickNext(page, GOOD_PACKAGE)
  await picker.locator('[data-package-choose]').click()

  const summary = picker.locator('[data-package-summary]')
  await expect(summary.locator('[data-summary-title]')).toHaveText('Workshop safety')
  await expect(summary.locator('[data-summary-standard]')).toHaveText('SCORM 1.2')
  await expect(summary.locator('[data-summary-edition]')).toHaveText('—')
  await expect(summary.locator('[data-summary-activity]')).toHaveText([
    /Getting started/, /Welcome to the workshop.*tracked/, /Safety check.*tracked/, /Summary sheet.*shown only/,
  ])
  await expect(summary.locator('[data-summary-activity][data-launchable="false"]')).toHaveText(['Getting started'])
  expect(await called(page, 'create_element')).toHaveLength(0)

  await picker.locator('[data-package-redistribution-confirm]').check()
  await picker.locator('[data-package-confirm]').click()

  await expect(page.locator('[data-outline-element]')).toHaveCount(3)
  const [created] = await called(page, 'create_element')
  expect(created?.args).toMatchObject({
    req: { title: 'Workshop safety', element_type: 'package', content_hash: 'ab'.repeat(32) },
  })
  await expect(page.locator('[data-package-editor] [data-package-attached]')).toContainText('Workshop safety')
})

const REFUSALS: [string, RegExp][] = [
  ['not_an_archive', /not a readable \.zip archive/],
  ['no_manifest', /no imsmanifest\.xml at the top/],
  ['manifest_unparseable', /manifest \(imsmanifest\.xml\) is malformed or incomplete/],
  ['unsupported_standard', /standard or edition Alexandria cannot play yet/],
  ['launch_resource_missing', /points at a file that is not in the archive/],
  ['launch_resource_remote', /loads from a web address/],
  ['unsafe_entry_path', /file names that are not allowed/],
  ['limit_exceeded', /This package is too large/],
]

test('each refused package says what is wrong in words an instructor can act on', async ({ page }) => {
  await open(page)
  const picker = await startAddingPackage(page)

  for (const [code, words] of REFUSALS) {
    await pickNext(page, `/packages/${code}.zip`)
    await picker.locator('[data-package-choose]').click()
    const refusal = picker.locator('[data-package-refusal]')
    await expect(refusal).toHaveAttribute('data-code', code)
    await expect(refusal).toContainText(words)
    await expect(refusal).toContainText(`what the importer found in /packages/${code}.zip`)
    await expect(picker.locator('[data-package-summary]')).toHaveCount(0)
    await expect(picker.locator('[data-package-confirm]')).toHaveCount(0)
  }
  expect(await called(page, 'create_element')).toHaveLength(0)
})

test('adding a package to a course that will be public needs the instructor to confirm they may redistribute it', async ({ page }) => {
  await open(page)
  const picker = await startAddingPackage(page)
  await pickNext(page, GOOD_PACKAGE)
  await picker.locator('[data-package-choose]').click()

  const warning = picker.locator('[data-package-redistribution]')
  await expect(warning).toContainText('Every learner\'s device will pass this package on')
  await expect(warning).toContainText('serves it to other learners')
  const add = picker.locator('[data-package-confirm]')
  await expect(add).toBeDisabled()

  await picker.locator('[data-package-redistribution-confirm]').check()
  await expect(add).toBeEnabled()
  await picker.locator('[data-package-redistribution-confirm]').uncheck()
  await expect(add).toBeDisabled()

  // Replacing an element's package asks again.
  await picker.getByRole('button', { name: 'Cancel' }).click()
  await page.locator('[data-outline-element]', { hasText: 'Workshop safety' }).click()
  await page.locator('[data-package-replace]').click()
  const replacing = page.locator('[data-package-editor] [data-package-picker]')
  await replacing.locator('[data-package-choose]').click()
  await expect(replacing.locator('[data-package-redistribution]')).toBeVisible()
  await expect(replacing.locator('[data-package-confirm]')).toBeDisabled()
  expect(await called(page, 'create_element')).toHaveLength(0)
  expect(await called(page, 'update_element')).toHaveLength(0)
})

test('a package element offers no grading controls and the host\'s refusal to grade it is explained', async ({ page }) => {
  await open(page)

  // A text lesson can carry skill tags; a package element cannot.
  await page.locator('[data-outline-element]', { hasText: 'House rules' }).click()
  await expect(page.getByRole('button', { name: 'Skill', exact: true })).toBeVisible()
  await page.locator('[data-outline-element]', { hasText: 'Workshop safety' }).click()
  await expect(page.locator('[data-package-editor]')).toBeVisible()
  await expect(page.getByRole('button', { name: 'Skill', exact: true })).toHaveCount(0)
  await expect(page.locator('[data-package-self-reported]')).toContainText('never counts toward a credential')

  // Should the host refuse a change as grading the package, the editor says why.
  await refuseUpdates(page)
  await page.locator('[data-package-replace]').click()
  const picker = page.locator('[data-package-editor] [data-package-picker]')
  await picker.locator('[data-package-choose]').click()
  await picker.locator('[data-package-redistribution-confirm]').check()
  await picker.locator('[data-package-confirm]').click()
  const error = page.locator('[data-package-error]')
  await expect(error).toHaveAttribute('data-code', 'package_result_self_reported')
  await expect(error).toContainText('can\'t be graded or become the assessed basis of a credential')
})

test('preview runs the package as a learner sees it without an enrolment or a recorded attempt', async ({ page }) => {
  const { refused } = await open(page)
  await page.locator('[data-outline-element]', { hasText: 'Workshop safety' }).click()
  await page.locator('[data-package-preview]').click()

  const player = page.locator('[data-package-preview-frame] [data-package-player]')
  await expect(player.locator('[data-package-label]')).toContainText('this course\'s author')
  await expect(player.locator('[data-package-preview-note]')).toContainText('nothing you do here is saved')
  await expect(player.locator('[data-package-toc] [data-activity]')).toHaveText([
    /Welcome to the workshop/, /Safety check/, /Summary sheet/,
  ])

  const lesson = page.frameLocator('[data-package-frame]').frameLocator('#lesson')
  const seen = lesson.locator('#seen[data-ready]')
  await expect(seen).toBeAttached()
  const values = JSON.parse(await seen.getAttribute('data-values') ?? '{}') as Record<string, string>
  expect(values).toMatchObject({ 'initialised': 'true', 'cmi.core.entry': 'ab-initio' })

  await lesson.locator('#leave').click()
  await expect.poll(async () => (await called(page, 'package_preview_commit')).length).toBeGreaterThan(0)

  await page.locator('[data-package-preview]').click()
  await expect(player).toHaveCount(0)
  await expect.poll(async () => (await called(page, 'package_preview_terminate')).length).toBe(1)

  expect((await called(page, 'package_preview_open'))[0]?.args).toMatchObject({ elementId: 'element-package' })
  for (const command of ['enroll', 'prepare_local_course', 'package_attempt_open', 'package_attempt_initialise',
    'package_attempt_commit', 'package_attempt_terminate', 'update_progress']) {
    expect(await called(page, command), command).toHaveLength(0)
  }
  expect(refused).toEqual([])
})
