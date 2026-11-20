import { afterEach, describe, expect, it, vi } from 'vitest'
import type { PackageActivity, PackageAttemptProgress } from '@/types'
import { createPackageSession, type PackageInvoke } from './packageSession'

// The host half of the player at its own seam: a real frame element (jsdom
// loads nothing into it), messages dispatched to the app window as the
// wrapper would post them, and a fake host bridge. What the wrapper page and
// the browser do is covered by the desktop-ui player specs.

const lesson: PackageActivity = {
  id: 'welcome',
  title: 'Welcome',
  visible: true,
  launch: { path: 'welcome/index.html', kind: 'sco' },
} as PackageActivity

const progress = { attempt: { id: 'attempt-1' }, element_progress: {} } as unknown as PackageAttemptProgress

function setup(baseUrl: string) {
  const frame = document.createElement('iframe')
  document.body.appendChild(frame)
  // A browser keeps a frame's window across navigations; jsdom makes a new one.
  const wrapper = frame.contentWindow!
  Object.defineProperty(frame, 'contentWindow', { value: wrapper })
  const posted: { message: unknown, target: string }[] = []
  vi.spyOn(wrapper, 'postMessage').mockImplementation(((message: unknown, target: string) => {
    posted.push({ message, target })
  }) as typeof wrapper.postMessage)
  const commands: { command: string, args?: Record<string, unknown> }[] = []
  const invoke: PackageInvoke = async <T>(command: string, args?: Record<string, unknown>) => {
    commands.push({ command, args })
    if (command === 'package_attempt_initialise') return { runtime: { cmi: {} } } as T
    return progress as T
  }
  const session = createPackageSession({
    invoke,
    frame,
    launch: { prefix: 'launch-1', package_hash: 'hash', base_url: baseUrl },
    attemptId: 'attempt-1',
    standard: 'scorm12',
    events: { progress: () => {}, saveError: () => {}, blocked: () => {}, focusRequest: () => {}, refused: () => {} },
  })
  const send = (data: unknown, source: Window | null, origin: string) =>
    window.dispatchEvent(new MessageEvent('message', { data, source, origin }))
  const commits = () => commands.filter(call => call.command === 'package_attempt_commit')
  return { frame, wrapper, posted, session, send, commits, origin: new URL(baseUrl).origin }
}

/** A started session with a lesson open: the clearing page and the wrapper answered from the frame. */
async function started(baseUrl: string, wrapperOrigin: string) {
  const parts = setup(baseUrl)
  const starting = parts.session.start()
  await vi.waitFor(() => expect(parts.frame.src).toContain('clear.html#'))
  const nonce = new URL(parts.frame.src).hash.slice(1)
  parts.send({ source: 'alexandria-package', type: 'storage-cleared', nonce, ok: true }, parts.wrapper, wrapperOrigin)
  await vi.waitFor(() => expect(parts.frame.src).toContain('wrapper.html'))
  parts.send({ source: 'alexandria-wrapper', type: 'ready' }, parts.wrapper, wrapperOrigin)
  await starting
  await parts.session.openLesson(lesson)
  return parts
}

afterEach(() => {
  document.body.replaceChildren()
  vi.restoreAllMocks()
})

describe('package session messages', () => {
  it('posts to the wrapper only at the launch origin', async () => {
    const { posted, origin } = await started('http://package.localhost/launch-1/', 'http://package.localhost')
    expect(posted.length).toBeGreaterThan(0)
    expect(posted.map(post => post.target)).toEqual(posted.map(() => origin))
  })

  it('takes a sourceless commit after the frame left the page only from the launch origin', async () => {
    const { frame, send, commits } = await started('http://package.localhost/launch-1/', 'http://package.localhost')
    frame.remove()
    const commit = { source: 'alexandria-wrapper', type: 'commit', activityId: 'welcome', sessionEnded: true, runtime: { cmi: {} } }
    send(commit, null, 'http://elsewhere.test')
    send(commit, null, 'null')
    await Promise.resolve()
    expect(commits()).toHaveLength(0)
    send(commit, null, 'http://package.localhost')
    await vi.waitFor(() => expect(commits()).toHaveLength(1))
  })

  it('stores only commits whose run-time state has the shape the host reads', async () => {
    const { wrapper, send, commits } = await started('http://package.localhost/launch-1/', 'http://package.localhost')
    const commit = (runtime: unknown) => ({ source: 'alexandria-wrapper', type: 'commit', activityId: 'welcome', sessionEnded: false, runtime })
    for (const runtime of [null, 'cmi', [], {}, { cmi: 'state' }, { cmi: ['a'] }, { cmi: null }]) {
      send(commit(runtime), wrapper, 'http://package.localhost')
    }
    await new Promise(resolve => setTimeout(resolve, 0))
    expect(commits()).toHaveLength(0)
    send(commit({ cmi: { core: { lesson_location: 'page-2' } } }), wrapper, 'http://package.localhost')
    await vi.waitFor(() => expect(commits()).toHaveLength(1))
    expect(commits()[0]?.args).toMatchObject({ commit: { runtime: { cmi: { core: { lesson_location: 'page-2' } } } } })
  })

  it('on an app scheme with an opaque launch origin, still refuses sourceless messages from other origins', async () => {
    const { frame, send, commits } = await started('package://localhost/launch-1/', 'package://localhost')
    frame.remove()
    const commit = { source: 'alexandria-wrapper', type: 'commit', activityId: 'welcome', sessionEnded: true, runtime: { cmi: {} } }
    send(commit, null, 'http://elsewhere.test')
    send(commit, null, 'http://127.0.0.1:1420')
    await Promise.resolve()
    expect(commits()).toHaveLength(0)
    send(commit, null, 'package://localhost')
    await vi.waitFor(() => expect(commits()).toHaveLength(1))
  })
})
