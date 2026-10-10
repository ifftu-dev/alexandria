import { describe, expect, it } from 'vitest'
import { isOsCombo, isTextEntryChord, normaliseWebviewCombo, phantomCombos } from '../hotkeys'

const OS = ['cmd+tab', 'cmd+space', 'cmd+shift+3']

describe('normaliseWebviewCombo', () => {
  it('orders modifiers cmd,ctrl,alt,shift and lower-cases the key', () => {
    expect(normaliseWebviewCombo({ key: 'B', metaKey: true, ctrlKey: false, altKey: false, shiftKey: true })).toBe('cmd+shift+b')
    expect(normaliseWebviewCombo({ key: 'p', metaKey: false, ctrlKey: true, altKey: true, shiftKey: false })).toBe('ctrl+alt+p')
  })
  it('maps named and punctuation keys to the shared vocabulary', () => {
    expect(normaliseWebviewCombo({ key: ' ', metaKey: true, ctrlKey: false, altKey: false, shiftKey: false })).toBe('cmd+space')
    expect(normaliseWebviewCombo({ key: '\\', metaKey: true, ctrlKey: false, altKey: false, shiftKey: false })).toBe('cmd+backslash')
    expect(normaliseWebviewCombo({ key: 'ArrowLeft', metaKey: false, ctrlKey: true, altKey: false, shiftKey: false })).toBe('ctrl+left')
    expect(normaliseWebviewCombo({ key: 'F15', metaKey: false, ctrlKey: true, altKey: true, shiftKey: true })).toBe('ctrl+alt+shift+f15')
  })
  it('uses the physical key so shifted symbols and layout characters match the native names', () => {
    // cmd+shift+[ produces "{" in e.key; the native tap saw keycode 0x21 = bracketleft.
    expect(normaliseWebviewCombo({ key: '{', code: 'BracketLeft', metaKey: true, ctrlKey: false, altKey: false, shiftKey: true })).toBe('cmd+shift+bracketleft')
    expect(normaliseWebviewCombo({ key: '!', code: 'Digit1', metaKey: true, ctrlKey: false, altKey: false, shiftKey: true })).toBe('cmd+shift+1')
    expect(normaliseWebviewCombo({ key: 'ß', code: 'KeyS', metaKey: true, ctrlKey: false, altKey: false, shiftKey: false })).toBe('cmd+s')
    expect(normaliseWebviewCombo({ key: 'Enter', code: 'NumpadEnter', metaKey: false, ctrlKey: true, altKey: false, shiftKey: false })).toBe('ctrl+enter')
  })

  it('drops text-entry chords: Option+key on macOS, AltGr (ctrl+alt) on Windows / Linux', () => {
    // macOS: Option+e is a dead key for accents, never a hotkey worth recording.
    expect(normaliseWebviewCombo({ key: '´', code: 'KeyE', metaKey: false, ctrlKey: false, altKey: true, shiftKey: false }, false)).toBeNull()
    expect(normaliseWebviewCombo({ key: 'e', code: 'KeyE', metaKey: true, ctrlKey: false, altKey: true, shiftKey: false }, false)).toBe('cmd+alt+e')
    // Windows / Linux: AltGr reports as ctrl+alt; `@` on a German layout.
    expect(normaliseWebviewCombo({ key: '@', code: 'KeyQ', metaKey: false, ctrlKey: true, altKey: true, shiftKey: false }, true)).toBeNull()
    expect(normaliseWebviewCombo({ key: 'q', code: 'KeyQ', metaKey: false, ctrlKey: false, altKey: true, shiftKey: false }, true)).toBe('alt+q')
    expect(isTextEntryChord({ metaKey: false, ctrlKey: true, altKey: true }, false)).toBe(false)
  })

  it('never records plain or shift-only keys, nor bare modifiers', () => {
    expect(normaliseWebviewCombo({ key: 'a', metaKey: false, ctrlKey: false, altKey: false, shiftKey: false })).toBeNull()
    expect(normaliseWebviewCombo({ key: 'A', metaKey: false, ctrlKey: false, altKey: false, shiftKey: true })).toBeNull()
    expect(normaliseWebviewCombo({ key: 'Meta', metaKey: true, ctrlKey: false, altKey: false, shiftKey: false })).toBeNull()
  })
})

describe('isOsCombo', () => {
  it('matches the list exactly, and the whole cmd family only where cmd is system-owned', () => {
    expect(isOsCombo('cmd+tab', OS, false)).toBe(true)
    expect(isOsCombo('cmd+b', OS, false)).toBe(false)
    expect(isOsCombo('cmd+b', OS, true)).toBe(true)
    expect(isOsCombo('ctrl+b', OS, true)).toBe(false)
  })
})

describe('phantomCombos', () => {
  const native = [
    { at_ms: 1000, combo: 'cmd+b' },        // seen by webview → not phantom
    { at_ms: 2000, combo: 'cmd+backslash' },// never seen → phantom
    { at_ms: 3000, combo: 'cmd+tab' },      // OS → not phantom
    { at_ms: 4000, combo: 'cmd+enter' },    // during blur → not phantom
    { at_ms: 5000, combo: 'cmd+enter' },    // same combo but webview saw it 400 ms late → phantom
  ]
  const epoch = 10_000
  const webview = [
    { at: 11_120, combo: 'cmd+b' },
    { at: 15_400, combo: 'cmd+enter' },
  ]
  const blurs = [{ start: 13_900, end: 14_200 }]

  it('keeps only combos the webview never received while focused', () => {
    const out = phantomCombos({ native, epoch, webview, blurs, osCombos: OS, cmdIsSystem: false })
    expect(out.map(e => e.combo)).toEqual(['cmd+backslash', 'cmd+enter'])
  })

  it('treats every cmd combo as OS-owned on Windows / Linux', () => {
    const out = phantomCombos({ native, epoch, webview, blurs, osCombos: OS, cmdIsSystem: true })
    expect(out).toEqual([])
  })

  it('tolerance is symmetric and configurable', () => {
    const out = phantomCombos({ native, epoch, webview, blurs, osCombos: OS, cmdIsSystem: false, toleranceMs: 500 })
    expect(out.map(e => e.combo)).toEqual(['cmd+backslash'])
  })
})
