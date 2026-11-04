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
  const box = (await target.boundingBox())!
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2)
  const resting = await read()
  await page.mouse.down()
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
 * Click `action` through the DOM and, two frames later, report the motion the
 * change started inside `surface` (the whole document by default): every
 * running animation or transition longer than the 0.01 ms Reduced/Off floor.
 * Progress indicators (`data-progress`) are exempt: Reduced keeps them turning.
 */
export async function motionAfter(action: Locator, surface?: Locator): Promise<number> {
  const scope = surface ? await surface.elementHandle() : null
  return action.evaluate((el, scoped) => new Promise<number>(resolve => {
    (el as HTMLElement).click()
    requestAnimationFrame(() => requestAnimationFrame(() => {
      const animations = scoped ? scoped.getAnimations({ subtree: true }) : document.getAnimations()
      resolve(animations.filter(a => a.playState === 'running' && Number(a.effect?.getComputedTiming().duration) > 1
        && !(a.effect as KeyframeEffect | null)?.target?.closest('[data-progress]')).length)
    }))
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

