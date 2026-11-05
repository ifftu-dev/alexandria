import { computed, readonly, ref, watch } from 'vue'
import { useSetting } from './useSettings'

export type MotionPreference = 'system' | 'reduced' | 'off'
export type MotionMode = 'full' | 'reduced' | 'off'
// The profile's device-scoped setting is the source of truth once a profile is
// unlocked. The cache is device-wide, so the last preference applies before
// first paint and on the picker; it mirrors the setting, as the theme does.
const SETTING_KEY = 'ui.motion'
const CACHE_KEY = 'alexandria-motion'
const setting = useSetting<string>(SETTING_KEY)
const preference = ref<MotionPreference>('system')
const osReduced = ref(false)
const mode = computed<MotionMode>(() => preference.value === 'off' ? 'off'
  : preference.value === 'reduced' || osReduced.value ? 'reduced' : 'full')
let initialized = false
let stopSetting: (() => void) | undefined
let query: MediaQueryList | undefined
const animations = new Set<Animation>()
const owners = new WeakMap<Element, Animation>()

function validPreference(value: unknown): MotionPreference {
  return value === 'reduced' || value === 'off' ? value : 'system'
}

/** Only decorative work belongs here; domain actions never await animation. */
export function cancelMotion(): void {
  for (const animation of animations) animation.cancel()
  animations.clear()
}
function apply(): void {
  cancelMotion()
  document.documentElement.dataset.motion = mode.value
  document.documentElement.dataset.motionHidden = String(document.hidden)
}
function updateSystem(): void {
  osReduced.value = query?.matches ?? false
  apply()
}
function visibilityChanged(): void {
  if (document.hidden) apply()
  else updateSystem()
}
function writeCache(value: MotionPreference): void {
  try { localStorage.setItem(CACHE_KEY, value) } catch { /* Keep the in-memory preference usable. */ }
}
function settingChanged(value: string | null): void {
  // Null while no profile is unlocked: keep the cached preference.
  if (value === null) return
  preference.value = validPreference(value)
  writeCache(preference.value)
  apply()
}
function storageChanged(event: StorageEvent): void {
  if (event.key !== CACHE_KEY && event.key !== null) return
  preference.value = validPreference(event.newValue)
  apply()
}

export function initMotion(): void {
  if (initialized) return
  initialized = true
  try { preference.value = validPreference(localStorage.getItem(CACHE_KEY)) } catch { /* Storage can be unavailable in a restricted webview. */ }
  query = window.matchMedia?.('(prefers-reduced-motion: reduce)')
  updateSystem()
  query?.addEventListener('change', updateSystem)
  window.addEventListener('storage', storageChanged)
  window.addEventListener('focus', updateSystem)
  document.addEventListener('visibilitychange', visibilityChanged)
  document.addEventListener('pointerdown', cancelMotion, { passive: true, capture: true })
  document.addEventListener('keydown', cancelMotion, true)
  stopSetting = watch(setting.ref, settingChanged, { immediate: true })
}

export function useMotion() {
  initMotion()
  function setPreference(value: MotionPreference): void {
    preference.value = value
    writeCache(value)
    apply()
    // Unlocked: store it in the profile's device setting (never synced).
    if (setting.ref.value !== null) void setting.set(value).catch(() => { /* The cache keeps it on this device. */ })
  }
  return { preference: readonly(preference), mode, reduced: computed(() => mode.value !== 'full'), setPreference }
}

/** Duration tokens in main.css (`--duration-<name>`). */
export type MotionDuration = 'fast' | 'base' | 'slow' | 'emphasis' | 'reveal' | 'stagger'
// Used only where the stylesheet is absent (unit tests, a failed CSS load).
const FALLBACK_DURATION_MS = 200

function cssToken(name: string): string {
  try { return getComputedStyle(document.documentElement).getPropertyValue(name).trim() } catch { return '' }
}

/** A duration token in ms, so a timer can end with the animation it waits for. */
export function motionDuration(name: MotionDuration): number {
  const match = /^(\d*\.?\d+)(ms|s)$/.exec(cssToken(`--duration-${name}`))
  if (!match) return FALLBACK_DURATION_MS
  return Number(match[1]) * (match[2] === 's' ? 1000 : 1)
}

/** A finite effect over an already-usable final DOM state. New input wins. */
export function animateMotion(element: Element, frames: Keyframe[], duration: MotionDuration = 'base'): () => void {
  initMotion()
  owners.get(element)?.cancel()
  if (mode.value !== 'full' || document.hidden || !element.animate) return () => {}
  const easing = cssToken('--ease-settle') || 'ease-out'
  const animation = element.animate(frames, { duration: Math.min(motionDuration(duration), 1200), easing })
  owners.set(element, animation)
  animations.add(animation)
  const cleanup = () => { animations.delete(animation); if (owners.get(element) === animation) owners.delete(element) }
  animation.addEventListener('finish', cleanup, { once: true })
  animation.addEventListener('cancel', cleanup, { once: true })
  return () => { animation.cancel(); cleanup() }
}

/**
 * `@before-leave` for a `motion-list` TransitionGroup on a position: relative
 * list: holds a leaving item where it stood, out of flow, so the rest move into
 * place while it fades. Reduced and Off remove it at once, untouched.
 */
export function holdLeavingPlace(element: Element): void {
  if (mode.value !== 'full' || !(element instanceof HTMLElement)) return
  const { offsetTop, offsetLeft, offsetWidth, offsetHeight } = element
  Object.assign(element.style, {
    position: 'absolute', margin: '0',
    top: `${offsetTop}px`, left: `${offsetLeft}px`, width: `${offsetWidth}px`, height: `${offsetHeight}px`,
  })
}

if (import.meta.hot) import.meta.hot.dispose(() => {
  cancelMotion()
  stopSetting?.()
  query?.removeEventListener('change', updateSystem)
  window.removeEventListener('storage', storageChanged)
  window.removeEventListener('focus', updateSystem)
  document.removeEventListener('visibilitychange', visibilityChanged)
  document.removeEventListener('pointerdown', cancelMotion, true)
  document.removeEventListener('keydown', cancelMotion, true)
})
