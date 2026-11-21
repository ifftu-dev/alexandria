/**
 * iOS on-screen keyboard handling.
 *
 * WKWebView does not shrink the page when the keyboard opens. Instead it
 * slides the whole web view up to reveal the focused field, which pushes
 * the top bar under the notch and leaves controls below the field (the
 * Unlock button, for one) hidden behind the keyboard's toolbar.
 *
 * While the keyboard is open this sizes the app to the visual viewport
 * (the space above the keyboard and its toolbar), undoes the slide, and
 * scrolls the focused field into view inside the app's own scroll areas.
 * `html.keyboard-open` lets CSS hide bottom chrome that would otherwise
 * sit on top of the keyboard.
 */

/** Below this many pixels of lost height the viewport change is not a keyboard. */
const KEYBOARD_MIN_PX = 120

export interface KeyboardViewport {
  open: boolean
  /** Visible height above the keyboard, in CSS pixels. */
  height: number
}

/** Whether the keyboard is open, from the layout and visual viewport heights. */
export function keyboardViewport(layoutHeight: number, visualHeight: number): KeyboardViewport {
  const open = layoutHeight - visualHeight >= KEYBOARD_MIN_PX
  return { open, height: Math.round(visualHeight) }
}

function isTextField(el: Element | null): el is HTMLElement {
  if (!(el instanceof HTMLElement)) return false
  if (el.isContentEditable || el instanceof HTMLTextAreaElement || el instanceof HTMLSelectElement) return true
  if (!(el instanceof HTMLInputElement)) return false
  return !['button', 'checkbox', 'radio', 'range', 'submit', 'reset', 'file', 'color'].includes(el.type)
}

/**
 * Reveal the focused field. A short form (sign-in, unlock) comes into view
 * whole, so its submit button sits above the keyboard too.
 */
function revealFocused(visibleHeight: number) {
  const el = document.activeElement
  if (!isTextField(el)) return
  const form = el.closest('form')
  const target = form && form.getBoundingClientRect().height <= visibleHeight ? form : el
  target.scrollIntoView({ block: 'nearest', inline: 'nearest' })
}

export function installKeyboardViewport(): () => void {
  const vv = window.visualViewport
  if (!vv) return () => {}
  const root = document.documentElement
  let open = false

  const undoSlide = () => {
    if (window.scrollX !== 0 || window.scrollY !== 0) window.scrollTo(0, 0)
  }

  const update = () => {
    // The root element's clientHeight is the layout viewport, which the
    // keyboard leaves alone. (On iOS window.innerHeight shrinks with the
    // visual viewport, so it cannot tell the keyboard is open.)
    const next = keyboardViewport(root.clientHeight, vv.height)
    if (next.open) {
      root.style.setProperty('--app-height', `${next.height}px`)
      root.classList.add('keyboard-open')
      undoSlide()
      // Wait for the new height to lay out before measuring the form.
      requestAnimationFrame(() => revealFocused(next.height))
    } else if (open) {
      root.style.removeProperty('--app-height')
      root.classList.remove('keyboard-open')
      undoSlide()
    }
    open = next.open
  }

  const onScroll = () => {
    if (open) undoSlide()
  }

  vv.addEventListener('resize', update)
  vv.addEventListener('scroll', onScroll)
  window.addEventListener('scroll', onScroll)
  return () => {
    vv.removeEventListener('resize', update)
    vv.removeEventListener('scroll', onScroll)
    window.removeEventListener('scroll', onScroll)
  }
}
