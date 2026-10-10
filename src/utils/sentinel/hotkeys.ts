// Phantom-hotkey correlation (pure).
//
// The native monitor (`sentinel::global_hotkeys`) records every modifier
// combo the OS saw. The webview records every modifier combo it received.
// A combo the OS saw but the webview never did, while our window had
// focus, went to something else on this machine: an overlay tool's global
// hotkey is the case we care about. OS-owned shortcuts (Cmd+Tab, Win+…)
// are subtracted first.
//
// Both sides use the same normalised vocabulary: modifiers in the order
// cmd, ctrl, alt, shift (Win and Super are "cmd"), then a lower-cased key
// name. Keep `normaliseWebviewCombo` in step with the Rust
// `normalise_combo` key names.

export interface WebviewCombo {
  /** performance.now() when the webview received the keydown. */
  at: number
  combo: string
}

export interface NativeCombo {
  /** Milliseconds since the native monitor started. */
  at_ms: number
  combo: string
}

export interface BlurInterval {
  start: number
  end: number
}

/**
 * Physical-key (`KeyboardEvent.code`) → shared name. The native monitors
 * report the unshifted physical key (CGEvent keycode / VK code / X11
 * keysym), so `cmd+shift+[` must stay `cmd+shift+bracketleft` here even
 * though `e.key` says `{`. Letters, digits and F-keys are handled by the
 * prefix rules in `codeName`.
 */
const CODE_NAMES: Record<string, string> = {
  Space: 'space',
  Enter: 'enter',
  NumpadEnter: 'enter',
  Tab: 'tab',
  Escape: 'escape',
  Backspace: 'backspace',
  Delete: 'delete',
  Insert: 'insert',
  Home: 'home',
  End: 'end',
  PageUp: 'pageup',
  PageDown: 'pagedown',
  ArrowUp: 'up',
  ArrowDown: 'down',
  ArrowLeft: 'left',
  ArrowRight: 'right',
  Backslash: 'backslash',
  IntlBackslash: 'backslash',
  Slash: 'slash',
  Comma: 'comma',
  Period: 'period',
  Semicolon: 'semicolon',
  Quote: 'quote',
  BracketLeft: 'bracketleft',
  BracketRight: 'bracketright',
  Minus: 'minus',
  Equal: 'equal',
  Backquote: 'grave',
}

function codeName(code: string): string | null {
  if (CODE_NAMES[code]) return CODE_NAMES[code]
  if (/^Key[A-Z]$/.test(code)) return code.slice(3).toLowerCase()
  if (/^Digit[0-9]$/.test(code)) return code.slice(5)
  if (/^Numpad[0-9]$/.test(code)) return code.slice(6)
  if (/^F([1-9]|1[0-9]|2[0-4])$/.test(code)) return code.toLowerCase()
  return null
}

/** `e.key` fallback when the event has no physical code (synthetic events). */
const KEY_NAMES: Record<string, string> = {
  ' ': 'space',
  Enter: 'enter',
  Tab: 'tab',
  Escape: 'escape',
  Backspace: 'backspace',
  Delete: 'delete',
  ArrowUp: 'up',
  ArrowDown: 'down',
  ArrowLeft: 'left',
  ArrowRight: 'right',
  '\\': 'backslash',
  '/': 'slash',
  ',': 'comma',
  '.': 'period',
  ';': 'semicolon',
  "'": 'quote',
  '[': 'bracketleft',
  ']': 'bracketright',
  '-': 'minus',
  '=': 'equal',
  '`': 'grave',
}

/**
 * Text-entry chords the native monitors refuse to record: Option+key on
 * macOS types accented letters and symbols; Ctrl+Alt on Windows / Linux is
 * AltGr. Recording either would store typed characters. `cmdIsSystem`
 * (from the hotkey status) tells the two families apart.
 */
export function isTextEntryChord(mods: { metaKey: boolean; ctrlKey: boolean; altKey: boolean }, cmdIsSystem: boolean): boolean {
  if (cmdIsSystem) return mods.ctrlKey && mods.altKey && !mods.metaKey
  return mods.altKey && !mods.metaKey && !mods.ctrlKey
}

/**
 * Normalise a webview keydown into the shared combo vocabulary, or null
 * when no recordable modifier (cmd/ctrl/alt) is held — Shift-only, plain
 * keys and text-entry chords are never recorded, mirroring the native
 * privacy gate. Prefers the physical key (`code`) so shifted symbols and
 * layout-dependent characters line up with what the native side saw.
 */
export function normaliseWebviewCombo(
  e: Pick<KeyboardEvent, 'key' | 'metaKey' | 'ctrlKey' | 'altKey' | 'shiftKey'> & { code?: string },
  cmdIsSystem = false,
): string | null {
  if (!e.metaKey && !e.ctrlKey && !e.altKey) return null
  if (isTextEntryChord(e, cmdIsSystem)) return null
  const key = e.key
  if (['Meta', 'Control', 'Alt', 'Shift', 'OS', 'Dead', 'Unidentified'].includes(key)) return null
  const fromCode = e.code ? codeName(e.code) : null
  const name = fromCode ?? KEY_NAMES[key] ?? key.toLowerCase()
  const parts: string[] = []
  if (e.metaKey) parts.push('cmd')
  if (e.ctrlKey) parts.push('ctrl')
  if (e.altKey) parts.push('alt')
  if (e.shiftKey) parts.push('shift')
  parts.push(name)
  return parts.join('+')
}

export function isOsCombo(combo: string, osCombos: readonly string[], cmdIsSystem: boolean): boolean {
  return osCombos.includes(combo) || (cmdIsSystem && combo.startsWith('cmd+'))
}

/** Native events the webview never saw while our window was focused. */
export function phantomCombos(opts: {
  native: readonly NativeCombo[]
  /** performance.now() corresponding to the native monitor's t=0. */
  epoch: number
  webview: readonly WebviewCombo[]
  blurs: readonly BlurInterval[]
  osCombos: readonly string[]
  cmdIsSystem: boolean
  /** Match tolerance in ms (IPC latency + clock alignment). */
  toleranceMs?: number
}): NativeCombo[] {
  const tol = opts.toleranceMs ?? 250
  return opts.native.filter(ev => {
    if (isOsCombo(ev.combo, opts.osCombos, opts.cmdIsSystem)) return false
    const at = opts.epoch + ev.at_ms
    if (opts.blurs.some(b => at >= b.start - tol && at <= b.end + tol)) return false
    return !opts.webview.some(w => w.combo === ev.combo && Math.abs(w.at - at) <= tol)
  })
}
