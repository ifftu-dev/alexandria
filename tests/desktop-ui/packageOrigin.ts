import { readFile } from 'node:fs/promises'
import type { Page } from '@playwright/test'
import { handlerConstant } from './packageHarness'

// Serves the package origin the fake hosts hand out (`launch-N/` under
// `http://scorm-package.test`) from a fixture package on disk (the player
// fixture unless told otherwise), as
// the package player spec does: reserved files from the app's serving
// directory, the bootstrap linked into every HTML page, and the scheme
// handler's own content policy headers. Any other request is refused and
// recorded.

export const PACKAGE_ORIGIN = 'http://scorm-package.test'
const serving = new URL('../../src-tauri/src/package_serving/', import.meta.url)
const reserved: Record<string, URL> = {
  'bootstrap.js': new URL('bootstrap.js', serving),
  'clear.html': new URL('clear.html', serving),
  'wrapper.html': new URL('wrapper.html', serving),
  'wrapper.js': new URL('wrapper.js', serving),
  'scorm12.min.js': new URL('../../resources/scorm-again/scorm12.min.js', serving),
  'scorm2004.min.js': new URL('../../resources/scorm-again/scorm2004.min.js', serving),
}

const contentType = (path: string) =>
  path.endsWith('.html') ? 'text/html' : path.endsWith('.js') ? 'text/javascript' : 'application/octet-stream'

export async function servePackageOrigin(page: Page, fixture = 'package-player') {
  const packageFiles = new URL(`./${fixture}/package/`, import.meta.url)
  const policy = await handlerConstant('CONTENT_POLICY')
  const permissions = await handlerConstant('PERMISSIONS_POLICY')
  const refused: string[] = []
  await page.route('**/*', async (route) => {
    const url = new URL(route.request().url())
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
  return { refused }
}
