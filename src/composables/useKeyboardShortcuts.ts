/**
 * Centralized keyboard shortcuts with user-customizable bindings.
 *
 * Defaults:
 *   Cmd/Ctrl + F → search (OmniSearch)
 *   Cmd/Ctrl + B → toggle sidebar
 *   Cmd/Ctrl + , → settings
 *   Cmd/Ctrl + [ → navigate back
 *   Cmd/Ctrl + ] → navigate forward
 *   /            → focus search (outside editable fields)
 *
 * Users can rebind any shortcut via the Settings → Keyboard Shortcuts
 * section. Bindings are persisted in localStorage.
 *
 * Architecture:
 *   The composable owns one global keydown listener. Components register
 *   handlers via `registerAction(id, handler)` and the listener
 *   dispatches by matching the pressed keys against the active bindings.
 *   Handlers are de-registered when the component unmounts.
 */

import { reactive, onMounted, onUnmounted, readonly } from 'vue'
import { isMac } from '@/composables/usePlatform'

const STORAGE_KEY = 'alexandria-keyboard-shortcuts'

/** A single key combination: modifier flags + a key name. */
export interface KeyCombo {
  /** Whether the platform modifier is required (Cmd on macOS, Ctrl elsewhere). */
  mod: boolean
  shift: boolean
  alt: boolean
  /** The `KeyboardEvent.key` value (case-insensitive match). */
  key: string
}

export interface ShortcutDefinition {
  id: string
  label: string
  /** The currently active binding (may differ from `defaultKeys` if the user customized it). */
  keys: KeyCombo
  /** The factory default — used for "reset to default". */
  defaultKeys: KeyCombo
}

// ---- Default bindings -----------------------------------------------

function combo(key: string, mod = true, shift = false, alt = false): KeyCombo {
  return { mod, shift, alt, key }
}

const DEFAULT_SHORTCUTS: Record<string, { label: string; keys: KeyCombo }> = {
  search: { label: 'Search', keys: combo('f') },
  'toggle-sidebar': { label: 'Toggle sidebar', keys: combo('b') },
  settings: { label: 'Open settings', keys: combo(',') },
  'nav-back': { label: 'Navigate back', keys: combo('[') },
  'nav-forward': { label: 'Navigate forward', keys: combo(']') },
  'focus-search': { label: 'Focus search (no modifier)', keys: combo('/', false) },
}

// ---- Global state (singleton) ----------------------------------------

/** Reactive map of id → ShortcutDefinition. Shared across all consumers. */
const shortcuts: Record<string, ShortcutDefinition> = reactive({})
/** Registered action handlers keyed by shortcut id. */
const handlers = new Map<string, Set<() => void>>()
let listenerInstalled = false
let initialized = false

function init() {
  if (initialized) return
  initialized = true

  // Populate from defaults.
  for (const [id, def] of Object.entries(DEFAULT_SHORTCUTS)) {
    shortcuts[id] = {
      id,
      label: def.label,
      keys: { ...def.keys },
      defaultKeys: { ...def.keys },
    }
  }

  // Apply user overrides from localStorage.
  try {
    const stored = localStorage.getItem(STORAGE_KEY)
    if (stored) {
      const overrides: Record<string, KeyCombo> = JSON.parse(stored)
      for (const [id, keys] of Object.entries(overrides)) {
        if (shortcuts[id]) {
          shortcuts[id].keys = { ...keys }
        }
      }
    }
  } catch {
    // Corrupt storage — ignore, defaults will apply.
  }
}

function persist() {
  const overrides: Record<string, KeyCombo> = {}
  for (const [id, def] of Object.entries(shortcuts)) {
    if (!comboEqual(def.keys, def.defaultKeys)) {
      overrides[id] = def.keys
    }
  }
  if (Object.keys(overrides).length > 0) {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(overrides))
  } else {
    localStorage.removeItem(STORAGE_KEY)
  }
}

function comboEqual(a: KeyCombo, b: KeyCombo): boolean {
  return (
    a.key.toLowerCase() === b.key.toLowerCase() &&
    a.mod === b.mod &&
    a.shift === b.shift &&
    a.alt === b.alt
  )
}

function matches(e: KeyboardEvent, c: KeyCombo): boolean {
  const modPressed = isMac ? e.metaKey : e.ctrlKey
  if (c.mod && !modPressed) return false
  if (!c.mod && modPressed) return false
  if (c.shift !== e.shiftKey) return false
  if (c.alt !== e.altKey) return false
  return e.key.toLowerCase() === c.key.toLowerCase()
}

function isEditableTarget(target: EventTarget | null): boolean {
  if (!target || !(target instanceof HTMLElement)) return false
  const tag = target.tagName
  if (tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT') return true
  return target.isContentEditable
}

function onKeydown(e: KeyboardEvent) {
  for (const [id, def] of Object.entries(shortcuts)) {
    if (!matches(e, def.keys)) continue
    // For non-modifier shortcuts (like "/"), skip when typing in a field.
    if (!def.keys.mod && isEditableTarget(e.target)) continue
    const actionHandlers = handlers.get(id)
    if (actionHandlers && actionHandlers.size > 0) {
      e.preventDefault()
      for (const fn of actionHandlers) fn()
      return
    }
  }
}

function installListener() {
  if (listenerInstalled) return
  listenerInstalled = true
  document.addEventListener('keydown', onKeydown, { capture: true })
}

// removeListener is reserved for future teardown (e.g., when the last
// consumer unmounts). Not called in practice because the listener is
// app-lifetime.
// function removeListener() { ... }

// ---- Public API ------------------------------------------------------

/**
 * Register a handler for a named shortcut. Multiple components can
 * register for the same id; all handlers fire. Automatically unregistered
 * when the calling component unmounts.
 */
function registerAction(id: string, handler: () => void) {
  if (!handlers.has(id)) handlers.set(id, new Set())
  handlers.get(id)!.add(handler)

  onUnmounted(() => {
    handlers.get(id)?.delete(handler)
    if (handlers.get(id)?.size === 0) handlers.delete(id)
  })
}

/** Update the binding for a shortcut. Persists immediately. */
function updateShortcut(id: string, keys: KeyCombo) {
  if (!shortcuts[id]) return
  shortcuts[id].keys = { ...keys }
  persist()
}

/** Reset a single shortcut to its factory default. */
function resetShortcut(id: string) {
  if (!shortcuts[id]) return
  shortcuts[id].keys = { ...shortcuts[id].defaultKeys }
  persist()
}

/** Reset all shortcuts to factory defaults. */
function resetAll() {
  for (const def of Object.values(shortcuts)) {
    def.keys = { ...def.defaultKeys }
  }
  persist()
}

/** Human-readable label for a KeyCombo, e.g. "⌘F" or "Ctrl+B". */
export function formatCombo(c: KeyCombo): string {
  const parts: string[] = []
  if (c.mod) parts.push(isMac ? '⌘' : 'Ctrl')
  if (c.shift) parts.push(isMac ? '⇧' : 'Shift')
  if (c.alt) parts.push(isMac ? '⌥' : 'Alt')
  const keyDisplay = c.key.length === 1 ? c.key.toUpperCase() : c.key
  parts.push(keyDisplay)
  return isMac ? parts.join('') : parts.join('+')
}

/** Parse a KeyboardEvent into a KeyCombo (for the settings recorder). */
export function comboFromEvent(e: KeyboardEvent): KeyCombo | null {
  // Ignore bare modifier presses.
  if (['Meta', 'Control', 'Shift', 'Alt'].includes(e.key)) return null
  return {
    mod: isMac ? e.metaKey : e.ctrlKey,
    shift: e.shiftKey,
    alt: e.altKey,
    key: e.key,
  }
}

export function useKeyboardShortcuts() {
  init()

  onMounted(() => {
    installListener()
  })

  return {
    shortcuts: readonly(shortcuts) as Readonly<Record<string, ShortcutDefinition>>,
    registerAction,
    updateShortcut,
    resetShortcut,
    resetAll,
    formatCombo,
    comboFromEvent,
  }
}
