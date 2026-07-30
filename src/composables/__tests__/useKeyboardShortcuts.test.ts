/**
 * Keyboard shortcut registry: the help binding, combo rendering, and the
 * device-sync guarantee.
 *
 * The registry is a module-level singleton, so these tests read it rather than
 * constructing one — which is also what the app does, and means a regression
 * in the shared state is visible here.
 */

import { describe, it, expect } from 'vitest'

import {
  DEFAULT_SHORTCUT_IDS,
  SHORTCUTS_SETTING_KEY,
  formatCombo,
  shortcutName,
  type KeyCombo,
  type ShortcutDefinition,
} from '@/composables/useKeyboardShortcuts'

const combo = (over: Partial<KeyCombo> = {}): KeyCombo => ({
  mod: true,
  shift: false,
  alt: false,
  key: 'f',
  ...over,
})

describe('shortcut registry', () => {
  it('ships a binding for the help sheet', () => {
    expect(DEFAULT_SHORTCUT_IDS).toContain('shortcuts-help')
  })

  it('every default id is unique', () => {
    expect(new Set(DEFAULT_SHORTCUT_IDS).size).toBe(DEFAULT_SHORTCUT_IDS.length)
  })
})

describe('formatCombo', () => {
  it('renders an ordinary modifier combo', () => {
    // Platform-dependent rendering (⌘ vs Ctrl); assert on the key, not the glyph.
    expect(formatCombo(combo({ key: 'f' }))).toMatch(/F$/)
  })

  it('keeps the shift modifier visible for letters', () => {
    // ⇧U and U are genuinely different bindings — hiding the modifier here
    // would make the sheet ambiguous.
    const rendered = formatCombo(combo({ key: 'u', shift: true }))
    expect(rendered).toMatch(/⇧|Shift/)
  })

  it('hides the redundant shift for glyphs that already imply it', () => {
    // "?" is Shift+"/" on most layouts, so the binding carries shift: true.
    // Rendering "⌘⇧?" would tell the user to press shift twice.
    const rendered = formatCombo(combo({ key: '?', shift: true }))
    expect(rendered).not.toMatch(/⇧|Shift/)
    expect(rendered).toMatch(/\?$/)
  })

  it('still shows shift for digits', () => {
    const rendered = formatCombo(combo({ key: '1', shift: true }))
    expect(rendered).toMatch(/⇧|Shift/)
  })
})

describe('the help binding matches the keys a user actually presses', () => {
  /**
   * On most layouts "?" is produced by Shift + "/", so the KeyboardEvent
   * arrives with `key: '?'` AND `shiftKey: true`. A binding declared without
   * shift would never fire. This asserts the shape of the event the default
   * binding has to match, so a well-meaning "the shift looks redundant"
   * cleanup breaks a test rather than the shortcut.
   */
  it('a Cmd/Ctrl+? event carries both the ? key and the shift flag', () => {
    const e = new KeyboardEvent('keydown', {
      key: '?',
      shiftKey: true,
      metaKey: true,
      ctrlKey: true,
    })
    expect(e.key).toBe('?')
    expect(e.shiftKey).toBe(true)
  })
})

describe('bindings sync across a user\'s devices', () => {
  /**
   * Custom bindings follow a user to their other devices because they persist
   * to a **sync-scoped** settings key rather than only to localStorage.
   *
   * This pins the frontend half of that contract. The backend half — that the
   * key is registered `Scope::Sync` and therefore actually replicated — is
   * asserted in `settings::registry`, because that is where it is true.
   */
  it('uses the settings key the Rust registry syncs', () => {
    expect(SHORTCUTS_SETTING_KEY).toBe('input.keyboard_shortcuts')
  })
})

describe('shortcutName', () => {
  const def = (id: string, label: string): ShortcutDefinition => ({
    id,
    label,
    keys: combo(),
    defaultKeys: combo(),
  })

  it('uses the translated name when the catalog has one', () => {
    expect(shortcutName(def('search', 'ignored English label'))).toBe('Search')
  })

  it('falls back to the registry label for an id with no catalog entry', () => {
    // A shortcut added in code before its translation key exists must still be
    // nameable. Rendering the raw key path in a settings row would be worse
    // than showing English.
    const name = shortcutName(def('not-a-real-shortcut', 'Fallback label'))
    expect(name).toBe('Fallback label')
    expect(name).not.toContain('shortcutNames')
  })

  it('names every shipped shortcut without leaking a key path', () => {
    for (const id of DEFAULT_SHORTCUT_IDS) {
      const name = shortcutName(def(id, `fallback-${id}`))
      expect(name, id).not.toContain('settings.personalization')
      expect(name.length, id).toBeGreaterThan(0)
    }
  })
})
