import { readFile } from 'node:fs/promises'
import { expect, type Frame, type FrameLocator, type Page } from '@playwright/test'
import type { PackageCommit, PackageSequencingState } from '@/types'
import type { PackageFixtureName } from './packageBackend'

// The player seam for the isolation and conformance checks: the real course
// player, wrapper and vendored runtime, a fixture package from disk and a fake
// host bridge (`packageBackend.ts`). The package origin is served here with
// `page.route` because the harness cannot run the Rust scheme handler. What
// the handler adds is mirrored from its source so a change there fails here:
//
// - every response carries `CONTENT_POLICY` and `PERMISSIONS_POLICY`, and
//   pages also `DOCUMENT_POLICY_SUFFIX`, read out of `handler.rs`;
// - every page gets the bootstrap linked after `<head>`;
// - only launches the host has open are served (anything else is a 404);
// - `Range` requests get 206 slices no longer than `range_chunk`, and a file
//   above `whole_file` is served only by range (`ServeLimits::DEFAULT`).
//
// The app page gets the app's own `frame-src` from `tauri.conf.json`, with the
// package scheme entries standing for this harness's package origin, so the
// browser holds the package frame to the package origin as the app would.
//
// Every frame at the package origin also gets, before any of its own script,
// a stand-in for the native command bridge (the Tauri globals and the message
// channels wry installs) and a peer-connection constructor that reports its
// use. That is the worst case: on macOS the native message channel is in every
// frame. The bootstrap must remove both from every window content can reach.

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

/** A string constant from the scheme handler, so the headers here stay the app's. */
export async function handlerConstant(name: string): Promise<string> {
  const source = await readFile(new URL('handler.rs', serving), 'utf8')
  const found = new RegExp(`const ${name}: &str = "((?:[^"\\\\]|\\\\[\\s\\S])*)";`).exec(source)
  if (found?.[1] === undefined) throw new Error(`${name} not found in handler.rs`)
  return found[1].replace(/\\\n\s*/g, '')
}

/** `ServeLimits::DEFAULT` from the scheme handler, in bytes. */
export async function handlerLimits(): Promise<{ wholeFile: number, rangeChunk: number }> {
  const source = await readFile(new URL('handler.rs', serving), 'utf8')
  const value = (field: string) => {
    const found = new RegExp(`${field}: ([\\d\\s*]+),`).exec(source.slice(source.indexOf('pub const DEFAULT')))
    if (!found?.[1]) throw new Error(`ServeLimits::DEFAULT.${field} not found in handler.rs`)
    return found[1].split('*').reduce((product, factor) => product * Number(factor.trim()), 1)
  }
  return { wholeFile: value('whole_file'), rangeChunk: value('range_chunk') }
}

/** The app window's `frame-src`, with the package scheme standing for the harness origin. */
export async function appFrameSources(): Promise<string> {
  const config = JSON.parse(await readFile(new URL('../../src-tauri/tauri.conf.json', import.meta.url), 'utf8')) as {
    app: { security: { csp: string } }
  }
  const directive = config.app.security.csp.split(';').map(part => part.trim()).find(part => part.startsWith('frame-src '))
  if (!directive) throw new Error('the app policy has no frame-src')
  const sources = directive.split(/\s+/).slice(1)
  const packageSources = ['package:', 'http://package.localhost']
  for (const source of packageSources) {
    if (!sources.includes(source)) throw new Error(`the app frame-src no longer lists ${source}`)
  }
  return ['frame-src', ...sources.filter(source => !packageSources.includes(source)), PACKAGE_ORIGIN].join(' ')
}

/** A page the handler treats as HTML (its `Kind::Html`): the bootstrap is linked in and the document policy added. */
const isPage = (path: string) => /\.html?$/i.test(path)

const contentType = (path: string) =>
  isPage(path) ? 'text/html'
    : path.endsWith('.js') ? 'text/javascript'
      : path.endsWith('.wav') ? 'audio/wav'
        : path.endsWith('.css') ? 'text/css'
          : 'application/octet-stream'

/** A silent 8 kHz mono WAV of `bytes` data bytes, made here so no media file is committed. */
export function silentWav(bytes: number): Buffer {
  const header = Buffer.alloc(44)
  header.write('RIFF', 0)
  header.writeUInt32LE(36 + bytes, 4)
  header.write('WAVEfmt ', 8)
  header.writeUInt32LE(16, 16)
  header.writeUInt16LE(1, 20)
  header.writeUInt16LE(1, 22)
  header.writeUInt32LE(8000, 24)
  header.writeUInt32LE(8000, 28)
  header.writeUInt16LE(1, 32)
  header.writeUInt16LE(8, 34)
  header.write('data', 36)
  header.writeUInt32LE(bytes, 40)
  return Buffer.concat([header, Buffer.alloc(bytes, 0x80)])
}

export interface MediaRequest { path: string, range: string | null, status: number, start: number | null }

export interface Served {
  /** Requests for anything but the package origin and the dev server: each one an escape. */
  refused: string[]
  /** Web sockets opened from anywhere in the page. */
  sockets: string[]
  /** Uses of the stand-in command bridge, as `what @ address`. */
  bridge: string[]
  /** Peer connections constructed, as `address`. */
  peers: string[]
  /** Package requests answered 404 (closed launch, missing file). */
  missing: string[]
  /** Requests for non-page package files, with the range asked for and the answer. */
  files: MediaRequest[]
  /** Windows the page opened. */
  popups: string[]
}

export interface OpenOptions {
  /** A fixture package under `tests/desktop-ui/<fixture>/package/`. */
  fixture?: PackageFixtureName
  /**
   * Instead, one of the SCORM 2004 test suite's LMS test packages
   * (`scorm2004-conformance/adl/`): every page it launches is the 2004 probe.
   */
  adl?: string
  /** Link the bootstrap into pages, as the handler does (default true). Off only for controls. */
  bootstrap?: boolean
  /** Files served at the package path without being in the package. */
  extraFiles?: Record<string, Buffer>
  /** Stand-ins for the player's reserved files (`_alexandria/<name>`), e.g. a broken `clear.html`. */
  reservedFiles?: Record<string, string>
  /**
   * How often the wrapper may hand over a running lesson's state, instead of
   * its two-second default, so a check that nothing more is written need not
   * wait out the real interval. Served as a changed default in `wrapper.js`.
   */
  commitIntervalMs?: number
  /** More screen parameters, e.g. `flow=off` (a SCORM 2004 package with flow control off). */
  query?: string
}

const RECORD = '__alexandriaHarnessRecord'

/** `wrapper.js` with its default commit interval set to `ms`. */
async function wrapperWithInterval(ms: number): Promise<string> {
  const source = await readFile(reserved['wrapper.js']!, 'utf8')
  const changed = source.replace(/var DEFAULT_COMMIT_INTERVAL_MS = \d+;/, `var DEFAULT_COMMIT_INTERVAL_MS = ${ms};`)
  if (changed === source) throw new Error('DEFAULT_COMMIT_INTERVAL_MS not found in wrapper.js')
  return changed
}

/** Opens the player on `fixture`, serving the package origin from disk. */
export async function openPlayer(page: Page, options: OpenOptions): Promise<Served> {
  const policy = await handlerConstant('CONTENT_POLICY')
  const documentPolicy = policy + await handlerConstant('DOCUMENT_POLICY_SUFFIX')
  const permissions = await handlerConstant('PERMISSIONS_POLICY')
  const limits = await handlerLimits()
  const frameSources = await appFrameSources()
  if (options.commitIntervalMs !== undefined) {
    options = { ...options, reservedFiles: { 'wrapper.js': await wrapperWithInterval(options.commitIntervalMs), ...options.reservedFiles } }
  }
  const packageFiles = new URL(`./${options.fixture ?? 'package-player'}/package/`, import.meta.url)
  const adlLesson = new URL('./scorm2004-conformance/package/probe.html', import.meta.url)
  const served: Served = { refused: [], sockets: [], bridge: [], peers: [], missing: [], files: [], popups: [] }

  await page.exposeBinding(RECORD, (_source, kind: string, detail: string) => {
    if (kind === 'peer') served.peers.push(detail)
    else served.bridge.push(`${kind} @ ${detail}`)
  })
  await page.addInitScript(({ origin, record }) => {
    if (self.origin !== origin) return
    const report = (window as unknown as Record<string, (kind: string, detail: string) => void>)[record]
    if (typeof report !== 'function') return
    const call = (what: string) => () => { report(what, String(location.href)); return Promise.resolve(null) }
    const win = window as unknown as Record<string, unknown>
    win.__TAURI_INTERNALS__ = { invoke: call('__TAURI_INTERNALS__.invoke'), postMessage: call('__TAURI_INTERNALS__.postMessage') }
    win.__TAURI__ = { core: { invoke: call('__TAURI__.core.invoke') } }
    win.__TAURI_IPC__ = call('__TAURI_IPC__')
    win.ipc = { postMessage: call('ipc.postMessage') }
    const chrome = (win.chrome ?? (win.chrome = {})) as Record<string, unknown>
    chrome.webview = { postMessage: call('chrome.webview.postMessage') }
    const webkit = (win.webkit ?? (win.webkit = {})) as Record<string, unknown>
    try {
      Object.defineProperty(webkit, 'messageHandlers', {
        configurable: true,
        value: { ipc: { postMessage: call('webkit.messageHandlers.ipc.postMessage') } },
      })
    } catch { /* a native one we cannot replace */ }
    const Real = window.RTCPeerConnection
    if (Real) {
      const Reporting = function (this: unknown, ...args: ConstructorParameters<typeof RTCPeerConnection>) {
        report('peer', String(location.href))
        return new Real(...args)
      } as unknown as typeof RTCPeerConnection
      Reporting.prototype = Real.prototype
      win.RTCPeerConnection = Reporting
      win.webkitRTCPeerConnection = Reporting
    }
  }, { origin: PACKAGE_ORIGIN, record: RECORD })

  page.on('websocket', (socket) => {
    // The dev server's own reload socket is the app page's, not content's.
    if (new URL(socket.url()).hostname !== '127.0.0.1') served.sockets.push(socket.url())
  })
  page.on('popup', popup => served.popups.push(popup.url()))
  page.context().on('page', opened => { if (opened !== page) served.popups.push(opened.url()) })

  const openLaunches = () => page.evaluate(() =>
    (window as unknown as { packageFixture?: { openLaunches: () => string[] } }).packageFixture?.openLaunches() ?? [])

  await page.route('**/*', async (route) => {
    const request = route.request()
    const url = new URL(request.url())
    if (url.hostname === '127.0.0.1') {
      if (request.isNavigationRequest() && request.frame() === page.mainFrame()) {
        const response = await route.fetch()
        return route.fulfill({ response, headers: { ...response.headers(), 'content-security-policy': frameSources } })
      }
      return route.continue()
    }
    const match = url.origin === PACKAGE_ORIGIN ? /^\/(launch-\d+)\/(.+)$/.exec(url.pathname) : null
    if (!match) {
      served.refused.push(url.href)
      return route.abort()
    }
    const [, prefix, path] = match as unknown as [string, string, string]
    const notFound = () => {
      served.missing.push(url.href)
      return route.fulfill({ status: 404, body: '', headers: { 'Content-Security-Policy': policy, 'Permissions-Policy': permissions } })
    }
    if (!(await openLaunches()).includes(prefix)) return notFound()
    let body: Buffer
    try {
      if (path.startsWith('_alexandria/')) {
        const name = path.slice('_alexandria/'.length)
        const standIn = options.reservedFiles?.[name]
        const file = reserved[name]
        if (standIn !== undefined) body = Buffer.from(standIn)
        else if (!file) return notFound()
        else body = await readFile(file)
      } else if (options.extraFiles?.[path]) {
        body = options.extraFiles[path]!
      } else if (options.adl !== undefined) {
        if (!isPage(path)) return notFound()
        body = await readFile(adlLesson)
      } else {
        body = await readFile(new URL(path, packageFiles))
      }
    } catch {
      return notFound()
    }
    const headers: Record<string, string> = { 'Content-Security-Policy': policy, 'Permissions-Policy': permissions }
    if (isPage(path)) {
      // The handler's `without_fetch_preloads`, mirrored (its own tests pin it).
      body = Buffer.from(body.toString('utf8').replace(
        /(<link\b[^>]*?[\s/])as(\s*=\s*(?:"\s*fetch\s*"|'\s*fetch\s*'|fetch(?=[\s/>])))/gi, '$1data-alexandria-as$2'))
      if (options.bootstrap !== false) {
        const tag = `<script src="/${prefix}/_alexandria/bootstrap.js"></script>`
        const html = body.toString('utf8')
        const at = html.indexOf('>', html.toLowerCase().indexOf('<head')) + 1
        body = Buffer.from(html.slice(0, at) + tag + html.slice(at))
      }
      // The Trusted Types requirement goes with the bootstrap that meets it.
      headers['Content-Security-Policy'] = options.bootstrap === false ? policy : documentPolicy
      return route.fulfill({ body, contentType: contentType(path), headers })
    }
    // The handler's range rules, for the player side of ranged media.
    const range = request.headers().range ?? null
    const record = (status: number, start: number | null) => served.files.push({ path, range, status, start })
    headers['Accept-Ranges'] = 'bytes'
    if (!range) {
      if (body.length > limits.wholeFile) {
        record(416, null)
        return route.fulfill({ status: 416, body: '', headers: { ...headers, 'Content-Range': `bytes */${body.length}` } })
      }
      record(200, null)
      return route.fulfill({ body, contentType: contentType(path), headers })
    }
    const spec = /^bytes=(\d*)-(\d*)/.exec(range)
    let start = spec?.[1] ? Number(spec[1]) : body.length - Number(spec?.[2] ?? 0)
    let end = spec?.[1] ? (spec[2] ? Math.min(Number(spec[2]), body.length - 1) : body.length - 1) : body.length - 1
    if (!spec || start < 0 || start > end || start >= body.length) {
      record(416, null)
      return route.fulfill({ status: 416, body: '', headers: { ...headers, 'Content-Range': `bytes */${body.length}` } })
    }
    end = Math.min(end, start + limits.rangeChunk - 1)
    start = Math.max(0, start)
    record(206, start)
    return route.fulfill({
      status: 206,
      body: body.subarray(start, end + 1),
      contentType: contentType(path),
      headers: { ...headers, 'Content-Range': `bytes ${start}-${end}/${body.length}` },
    })
  })

  const which = options.adl !== undefined ? `adl=${encodeURIComponent(options.adl)}` : `package=${options.fixture ?? 'package-player'}`
  await page.goto(`/tests/desktop-ui/?screen=package-player&${which}${options.query ? `&${options.query}` : ''}`)
  await expect(page.locator('[data-package-player]')).toBeVisible()
  return served
}

interface PackageFixtureWindow {
  packageFixture: {
    commits: PackageCommit[]
    commitTimes: number[]
    /** SCORM 2004: the engine state the wrapper handed the host, in order. */
    sequencing: PackageSequencingState[]
    openLaunches: () => string[]
    control: { failCommits: boolean }
    called: (command: string) => number
    navigate: (path: string) => Promise<void>
  }
}

export const commits = (page: Page) =>
  page.evaluate(() => (window as unknown as PackageFixtureWindow).packageFixture.commits)
export const savedStates = (page: Page) =>
  page.evaluate(() => (window as unknown as PackageFixtureWindow).packageFixture.sequencing)
export const commitTimes = (page: Page) =>
  page.evaluate(() => (window as unknown as PackageFixtureWindow).packageFixture.commitTimes)
export const openLaunches = (page: Page) =>
  page.evaluate(() => (window as unknown as PackageFixtureWindow).packageFixture.openLaunches())
export const called = (page: Page, command: string) =>
  page.evaluate(name => (window as unknown as PackageFixtureWindow).packageFixture.called(name), command)
export const navigate = (page: Page, path: string) =>
  page.evaluate(to => (window as unknown as PackageFixtureWindow).packageFixture.navigate(to), path)

/** The running lesson's frame. */
export const lesson = (page: Page): FrameLocator => page.frameLocator('[data-package-frame]').frameLocator('#lesson')

/** The wrapper page's frame. */
export function wrapperFrame(page: Page): Frame {
  const frame = page.frames().find(item => item.url().endsWith('/_alexandria/wrapper.html'))
  if (!frame) throw new Error('the wrapper is not loaded')
  return frame
}

/** A lesson frame's page under the launch prefix, query included (`probe.html?plain`). */
function lessonPath(frame: Frame): string | null {
  if (!frame.parentFrame()?.url().endsWith('/_alexandria/wrapper.html') || !URL.canParse(frame.url())) return null
  const url = new URL(frame.url())
  return url.pathname.split('/').slice(2).join('/') + url.search
}

const findLesson = (page: Page, path: string) => page.frames().find(frame => lessonPath(frame) === path)

/** The lesson frame showing exactly `path`, once it is there. */
export async function lessonFrame(page: Page, path: string): Promise<Frame> {
  await expect.poll(() => Boolean(findLesson(page, path))).toBe(true)
  return findLesson(page, path)!
}

/**
 * Opens the lesson `activityId` from the table of contents and waits until
 * its page (`path`, by default `<activityId>.html`) is the only lesson frame
 * and reports ready, so nothing is read from the lesson it replaced. With
 * `prefix`, the frame must also be under that launch (after a fresh attempt
 * or a return, the earlier launch's frame shows the same page).
 */
export async function openLesson(page: Page, activityId: string, path = `${activityId}.html`, prefix?: string): Promise<Frame> {
  const entry = page.locator(`[data-activity="${activityId}"]`)
  // Let the player pick the lesson it resumes at first: clicking that one
  // before it is current would open it twice, and the first session would
  // end under a test that is already driving it.
  await expect.poll(() => page.locator('[data-activity][aria-current="true"], [data-package-sequencing]').count()).toBeGreaterThan(0)
  if (await entry.getAttribute('aria-current') !== 'true') await entry.click()
  await expect(entry).toHaveAttribute('aria-current', 'true')
  return runningLesson(page, path, prefix)
}

/**
 * The lesson frame once `path` is the only lesson running and reports ready
 * (under launch `prefix`, if given), however it was opened: the player
 * resuming, a navigation control, the package's own request.
 */
export async function runningLesson(page: Page, path: string, prefix?: string): Promise<Frame> {
  const wanted = (frame: Frame) => lessonPath(frame) === path && (prefix === undefined || frame.url().includes(`/${prefix}/`))
  await expect.poll(async () => {
    const lessons = page.frames().filter(frame => lessonPath(frame) !== null)
    if (lessons.length !== 1 || !wanted(lessons[0]!)) return false
    return (await lessons[0]!.locator('#seen[data-ready]').count().catch(() => 0)) === 1
  }).toBe(true)
  return page.frames().find(wanted)!
}

/** What the running lesson reported in `#seen`. */
export async function seen(page: Page): Promise<Record<string, unknown>> {
  const values = lesson(page).locator('#seen[data-ready]')
  await expect(values).toBeAttached()
  return JSON.parse(await values.getAttribute('data-values') ?? '{}') as Record<string, unknown>
}

/** What the lesson in `frame` reported in `#seen`, read from that frame only. */
export async function seenIn(frame: Frame): Promise<Record<string, unknown>> {
  const values = frame.locator('#seen[data-ready]')
  await expect(values).toBeAttached()
  return JSON.parse(await values.getAttribute('data-values') ?? '{}') as Record<string, unknown>
}

/** Makes the fake host fail (or stop failing) every commit. */
export const failCommits = (page: Page, fail: boolean) =>
  page.evaluate((value) => { (window as unknown as PackageFixtureWindow).packageFixture.control.failCommits = value }, fail)
