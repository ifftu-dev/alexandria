import { readFile } from 'node:fs/promises'
import { expect, test, type FrameLocator, type Page } from '@playwright/test'

// cmi5 at the player seam: the real course player and wrapper page, the
// cmi5 fixture package (a block of two units and a unit that asks for its
// own window, `package-player/cmi5/package/`), a fake host bridge and a fake
// session endpoint on the loopback address. The package origin is served
// here with the bootstrap linked into every page and the content policy the
// scheme handler sends for a launch with an open cmi5 session, so the browser
// enforces that the unit reaches its endpoint and nothing else. The session
// rules themselves are the host's and are tested at the host seam.

const PACKAGE_ORIGIN = 'http://cmi5-package.test'
const ENDPOINT_PORT = '47125'
const serving = new URL('../../src-tauri/src/package_serving/', import.meta.url)
const packageFiles = new URL('./package-player/cmi5/package/', import.meta.url)
const reserved: Record<string, URL> = {
  'bootstrap.js': new URL('bootstrap.js', serving),
  'clear.html': new URL('clear.html', serving),
  'wrapper.html': new URL('wrapper.html', serving),
  'wrapper.js': new URL('wrapper.js', serving),
  'returned.html': new URL('returned.html', serving),
}

async function handlerConstant(name: string) {
  const source = await readFile(new URL('handler.rs', serving), 'utf8')
  const found = new RegExp(`const ${name}: &str = "((?:[^"\\\\]|\\\\[\\s\\S])*)";`).exec(source)
  if (!found?.[1]) throw new Error(`${name} not found in handler.rs`)
  return found[1].replace(/\\\n\s*/g, '')
}

const contentType = (path: string) =>
  path.endsWith('.html') ? 'text/html' : path.endsWith('.js') ? 'text/javascript' : 'application/octet-stream'

const cors = {
  'Access-Control-Allow-Origin': PACKAGE_ORIGIN,
  'Access-Control-Allow-Methods': 'GET, PUT, POST, DELETE',
  'Access-Control-Allow-Headers': 'Authorization, Content-Type, X-Experience-API-Version, If-Match, If-None-Match',
  'Vary': 'Origin',
}

interface Cmi5Fixture {
  endpoint: (request: { method: string, url: string, authorization: string | null, version: string | null, body: string | null }) =>
    { status: number, body: string }
  statements: { unit: string, verb: string, status: number }[]
  called: (command: string) => number
  navigate: (path: string) => Promise<void>
}

const fixture = <T>(page: Page, read: (fixture: Cmi5Fixture) => T) =>
  page.evaluate(`(${read.toString()})(window.cmi5Fixture)`) as Promise<T>

async function open(page: Page) {
  // Same rule as `admit_endpoint` in handler.rs: the session's loopback
  // endpoint joins connect-src, nothing else does.
  const policy = (await handlerConstant('CONTENT_POLICY'))
    .replace("connect-src 'self';", `connect-src 'self' http://127.0.0.1:${ENDPOINT_PORT};`)
  const permissions = await handlerConstant('PERMISSIONS_POLICY')
  const refused: string[] = []
  await page.route('**/*', async (route) => {
    const request = route.request()
    const url = new URL(request.url())
    if (url.hostname === '127.0.0.1' && url.port === ENDPOINT_PORT) {
      if (request.method() === 'OPTIONS') return route.fulfill({ status: 204, headers: cors })
      const answer = await page.evaluate((call) => (window as unknown as { cmi5Fixture: Cmi5Fixture }).cmi5Fixture.endpoint(call), {
        method: request.method(),
        url: url.href,
        authorization: request.headers()['authorization'] ?? null,
        version: request.headers()['x-experience-api-version'] ?? null,
        body: request.postData(),
      })
      return route.fulfill({ status: answer.status, body: answer.body, contentType: 'application/json', headers: cors })
    }
    if (url.hostname === '127.0.0.1') return route.continue()
    const match = url.origin === PACKAGE_ORIGIN ? /^\/(launch-\d+)\/(.+)$/.exec(url.pathname) : null
    if (!match) {
      refused.push(url.href)
      return route.abort()
    }
    const [, prefix, path] = match as unknown as [string, string, string]
    const file = path.startsWith('_alexandria/') ? reserved[path.slice('_alexandria/'.length)] : new URL(path, packageFiles)
    let body: Buffer
    try {
      if (!file) throw new Error('not reserved')
      body = await readFile(file)
    } catch {
      refused.push(url.href)
      return route.fulfill({ status: 404, body: '' })
    }
    if (path.endsWith('.html')) {
      const tag = `<script src="/${prefix}/_alexandria/bootstrap.js"></script>`
      const html = body.toString('utf8')
      const at = html.indexOf('>', html.toLowerCase().indexOf('<head')) + 1
      body = Buffer.from(html.slice(0, at) + tag + html.slice(at))
    }
    await route.fulfill({
      body,
      contentType: contentType(path),
      headers: { 'Content-Security-Policy': policy, 'Permissions-Policy': permissions },
    })
  })
  await page.goto('/tests/desktop-ui/?screen=cmi5-player')
  await expect(page.locator('[data-package-player]')).toBeVisible()
  return { refused }
}

const unit = (page: Page): FrameLocator => page.frameLocator('[data-package-frame]').frameLocator('#lesson')

/** What the unit saw, once its `initialized` went through. */
async function seen(page: Page) {
  const values = unit(page).locator('#seen')
  await expect.poll(async () => JSON.parse(await values.getAttribute('data-values') ?? '{}').sent?.length ?? 0).toBeGreaterThan(0)
  return JSON.parse(await values.getAttribute('data-values') ?? '{}') as Record<string, unknown>
}

const sent = async (page: Page) =>
  (JSON.parse(await unit(page).locator('#seen').getAttribute('data-values') ?? '{}').sent ?? []) as { verb: string, status: number }[]

test.beforeEach(async ({ context, browserName }) => {
  // Chromium treats a page served by an interceptor as public and refuses
  // its requests to the loopback address unless Local Network Access is
  // granted (ticket 23). The app's webviews grant or skip it.
  if (browserName === 'chromium') {
    await context.grantPermissions(['local-network-access'], { origin: PACKAGE_ORIGIN }).catch(() => {})
  }
})

test('a cmi5 unit launches in the frame, runs a session and the package completes', async ({ page }) => {
  const { refused } = await open(page)

  await expect(page.locator('[data-package-toc] [data-activity]')).toHaveText([/Walkaround/, /Safety check/, /Handbook/])
  const walkaround = await seen(page)
  expect(walkaround).toMatchObject({
    token: true,
    launchMode: 'Normal',
    moveOn: 'Completed',
    paramA: '1',
    languagePreference: 'en-US',
    registration: '11111111-2222-4333-8444-555555555555',
  })
  expect(walkaround.endpoint).toBe(`http://127.0.0.1:${ENDPOINT_PORT}/xapi/`)
  expect(String(walkaround.returnURL)).toMatch(/\/_alexandria\/returned\.html$/)

  await unit(page).locator('#complete').click()
  await expect.poll(() => sent(page)).toContainEqual({ verb: 'completed', status: 200 })
  // Exit: the unit terminates and returns to its return page; the player ends the session.
  await unit(page).locator('#exit').click()
  await expect(page.locator('[data-package-unit-exited]')).toBeVisible()
  await expect.poll(() => fixture(page, f => f.called('package_cmi5_end'))).toBe(1)
  await expect(page.locator('[data-package-toc] [data-activity="https://example.org/cmi5/forklift/walkaround"]'))
    .toHaveAttribute('data-complete', 'true')

  await page.locator('[data-package-toc] [data-activity="https://example.org/cmi5/forklift/check"]').click()
  const check = await seen(page)
  expect(check).toMatchObject({ moveOn: 'Passed', masteryScore: 0.8, launchParameters: 'route=b', paramA: null })
  await unit(page).locator('#pass').click()
  await expect.poll(() => sent(page)).toContainEqual({ verb: 'passed', status: 200 })
  await unit(page).locator('#exit').click()
  await expect(page.locator('[data-package-result]')).toContainText('Completed')

  const verbs = await fixture(page, f => f.statements.map(s => `${s.verb}:${s.status}`))
  expect(verbs).toEqual([
    'initialized:200', 'completed:200', 'terminated:200',
    'initialized:200', 'passed:200', 'terminated:200',
  ])
  expect(refused).toEqual([])
})

test('a statement the session refuses reaches the unit as a refusal', async ({ page }) => {
  await open(page)
  await seen(page)
  await unit(page).locator('#again').click()
  await expect.poll(() => sent(page)).toContainEqual({ verb: 'initialized', status: 403 })
})

test('leaving the player mid-session ends the session, which the host records as abandoned', async ({ page }) => {
  await open(page)
  await seen(page)
  await fixture(page, f => f.navigate('/elsewhere'))
  await expect(page.getByText('Elsewhere')).toBeVisible()
  await expect.poll(() => fixture(page, f => f.statements.map(s => s.verb))).toContain('abandoned')
  await expect.poll(() => fixture(page, f => f.called('package_attempt_terminate'))).toBe(1)
})

test('a unit that asks for its own window leaves the frame a placeholder that can bring it forward or end it', async ({ page }) => {
  await open(page)
  await seen(page)
  await page.locator('[data-package-toc] [data-activity="https://example.org/cmi5/forklift/handbook"]').click()
  const placeholder = page.locator('[data-package-unit-window]')
  await expect(placeholder).toBeVisible()
  await placeholder.locator('[data-package-unit-focus]').click()
  await expect.poll(() => fixture(page, f => f.called('package_cmi5_focus'))).toBe(1)
  await placeholder.locator('[data-package-unit-end]').click()
  await expect(placeholder).toBeHidden()
  // The walkaround's session was abandoned by the handbook's launch; the
  // handbook's own ended when the learner ended it.
  await expect.poll(() => fixture(page, f => f.called('package_cmi5_end'))).toBe(2)
})
