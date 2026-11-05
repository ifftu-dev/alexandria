import { expect, type Locator, type Page } from '@playwright/test'

// Shared by the motion surface specs: choose a motion setting, then read the
// motion a change starts through the real cascade and the Web Animations API.

export type Mode = 'System' | 'Reduced' | 'Off'
export const MODES = ['System', 'Reduced', 'Off'] as const

export async function open(page: Page, path = '/home') {
  await page.goto(`/tests/desktop-ui/?screen=motion-surfaces&path=${encodeURIComponent(path)}`)
  await expect(page.locator('output')).toHaveText(path)
}

/** Choose a motion setting, then let motion started under the previous one finish. */
export async function setMode(page: Page, mode: Mode) {
  await page.getByRole('button', { name: mode, exact: true }).click()
  await expect(page.locator('html')).toHaveAttribute('data-motion', mode === 'System' ? 'full' : mode.toLowerCase())
  await page.mouse.move(0, 0)
  await expect.poll(() => page.evaluate(() => document.getAnimations().filter(a => a.playState === 'running'
    && Number(a.effect?.getComputedTiming().duration) > 1
    && !(a.effect as KeyframeEffect | null)?.target?.closest('[data-progress]')).length)).toBe(0)
}

/** Hold the pointer on a control and read its settled pressed style, then release. */
export async function pressedStyle(page: Page, target: Locator) {
  await target.scrollIntoViewIfNeeded()
  const read = () => target.evaluate(el => {
    const computed = getComputedStyle(el)
    return JSON.stringify({ scale: computed.scale, opacity: computed.opacity, pointerEvents: computed.pointerEvents })
  })
  // Press where the control is now: late data can still shift the layout, so
  // wait until its centre holds still across two reads and hits the control.
  const centre = async () => {
    const box = await target.boundingBox()
    return box ? { x: box.x + box.width / 2, y: box.y + box.height / 2 } : null
  }
  let point = await centre()
  await expect.poll(async () => {
    const next = await centre()
    const steady = !!next && next.x === point?.x && next.y === point?.y
      && await target.evaluate((el, { x, y }) => el.contains(document.elementFromPoint(x, y)), next)
    point = next
    return steady
  }, { message: 'the control holds still under the pointer' }).toBe(true)
  await page.mouse.move(point!.x, point!.y)
  const resting = await read()
  // A loaded browser can lose a press between the protocol and the page, so
  // that nothing is :active; release and press again, at most three times. A
  // control that never takes :active (WebKit and a clipped child) still fails.
  const active = () => target.evaluate(el => new Promise<boolean>(resolve => requestAnimationFrame(() => resolve(el.matches(':active')))))
  for (let attempt = 1; ; attempt++) {
    await page.mouse.down()
    const pressed = await expect.poll(active, { timeout: 1000 }).toBe(true).then(() => true, () => false)
    if (pressed) break
    await page.mouse.up()
    expect(attempt, 'a press on the control makes it :active').toBeLessThan(3)
  }
  // Settled: changed from rest, and the same on two reads in a row.
  let style = resting
  await expect.poll(async () => {
    const next = await read()
    const settled = next !== resting && next === style
    style = next
    return settled
  }).toBe(true)
  await page.mouse.up()
  return JSON.parse(style) as { scale: string, opacity: string, pointerEvents: string }
}

/** Full motion presses inward; Reduced and Off dim in place instead. */
export async function expectPressFeedback(page: Page, target: Locator, mode: Mode) {
  const style = await pressedStyle(page, target)
  expect(style.pointerEvents).not.toBe('none')
  if (mode === 'System') {
    expect(Number(style.scale)).toBeLessThan(1)
  } else {
    expect(style.scale).toBe('none')
    expect(Number(style.opacity)).toBeLessThan(1)
  }
}

/**
 * Click `action` through the DOM and report the most motion the
 * change starts inside `surface` (the whole document by default) over the next twenty frames: every
 * running animation or transition longer than the 0.01 ms Reduced/Off floor.
 * Progress indicators (`data-progress`) are exempt: Reduced keeps them turning.
 */
export async function motionAfter(action: Locator, surface?: Locator): Promise<number> {
  const scope = surface ? await surface.elementHandle() : null
  return action.evaluate((el, scoped) => new Promise<number>(resolve => {
    // Record every Web Animation started meanwhile: on a slow machine a short
    // one can start and finish between two frames, out of reach of polling.
    const started: { target: Element; duration: number }[] = []
    const animate = Element.prototype.animate
    Element.prototype.animate = function (this: Element, frames, options) {
      started.push({ target: this, duration: typeof options === 'number' ? options : Number(options?.duration ?? 0) })
      return animate.call(this, frames, options)
    }
    const within = (target: Element | null | undefined) => !!target && (!scoped || scoped.contains(target)) && !target.closest('[data-progress]')
    ;(el as HTMLElement).click()
    // Every frame for a while: a change that follows a route or a backend
    // reply starts later on a slow machine than two frames after the click.
    let most = 0, frames = 0
    const sample = () => {
      const animations = scoped ? scoped.getAnimations({ subtree: true }) : document.getAnimations()
      most = Math.max(most, animations.filter(a => a.playState === 'running' && Number(a.effect?.getComputedTiming().duration) > 1
        && within((a.effect as KeyframeEffect | null)?.target)).length)
      if (++frames < 20) { requestAnimationFrame(sample); return }
      Element.prototype.animate = animate
      resolve(Math.max(most, started.filter(entry => entry.duration > 1 && within(entry.target)).length))
    }
    requestAnimationFrame(() => requestAnimationFrame(sample))
  }), scope)
}

/** Full motion moves; Reduced and Off change state with no motion at all. */
export function expectMotion(count: number, mode: Mode) {
  if (mode === 'System') expect(count).toBeGreaterThan(0)
  else expect(count).toBe(0)
}

/** The selection indicator's opacity on the next frame. */
export const indicator = (target: Locator) => target.evaluate(el => new Promise<string>(resolve => {
  requestAnimationFrame(() => resolve(getComputedStyle(el, '::before').opacity))
}))

/**
 * Start `act`, and once an element matching `selector` with `text` arrives,
 * report (two frames later) the motion running inside `scope`, or on the
 * arriving element itself when no scope is given.
 */
export async function motionOnArrival(page: Page, act: () => Promise<void>, selector: string, text: string, scope = ''): Promise<number> {
  const arrival = page.evaluate(([selector, text, scope]) => new Promise<number>(resolve => {
    const find = () => [...document.querySelectorAll(selector)].find(el => el.textContent?.includes(text))
    const report = (arrived: Element) => requestAnimationFrame(() => requestAnimationFrame(() => {
      const root = scope ? document.querySelector(scope) : arrived
      const animations = root ? root.getAnimations({ subtree: Boolean(scope) }) : []
      resolve(animations.filter(a => a.playState === 'running' && Number(a.effect?.getComputedTiming().duration) > 1
        && !(a.effect as KeyframeEffect | null)?.target?.closest('[data-progress]')).length)
    }))
    const now = find()
    if (now) return report(now)
    const observer = new MutationObserver(() => {
      const arrived = find()
      if (arrived) { observer.disconnect(); report(arrived) }
    })
    observer.observe(document.body, { subtree: true, childList: true, characterData: true })
  }), [selector, text, scope] as const)
  await act()
  return arrival
}

/** Whether a pointer at the centre of `target` reaches it, one frame after `act` runs in the page. */
export async function reachableAfter(actor: Locator, target: Locator, act: (el: Element) => void): Promise<boolean> {
  const handle = await target.elementHandle()
  return actor.evaluate((el, [target, source]) => new Promise<boolean>(resolve => {
    new Function('el', `(${source})(el)`)(el)
    requestAnimationFrame(() => {
      const box = target!.getBoundingClientRect()
      resolve(target!.contains(document.elementFromPoint(box.x + box.width / 2, box.y + box.height / 2)))
    })
  }), [handle, act.toString()] as const)
}

