import { computed, readonly, ref } from 'vue'

export type MotionPreference = 'system' | 'reduced' | 'off'
export type MotionMode = 'full' | 'reduced' | 'off'
const CACHE_KEY = 'alexandria-motion'
const preference = ref<MotionPreference>('system')
const osReduced = ref(false)
const mode = computed<MotionMode>(() => preference.value === 'off' ? 'off'
  : preference.value === 'reduced' || osReduced.value ? 'reduced' : 'full')
let initialized = false
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
}

export function useMotion() {
  initMotion()
  function setPreference(value: MotionPreference): void {
    preference.value = value
    // Device-wide, including before unlock. No account or sync state is stored.
    try { localStorage.setItem(CACHE_KEY, value) } catch { /* Keep the in-memory preference usable. */ }
    apply()
  }
  return { preference: readonly(preference), mode, reduced: computed(() => mode.value !== 'full'), setPreference }
}

/** A finite effect over an already-usable final DOM state. New input wins. */
export function animateMotion(element: Element, frames: Keyframe[], duration = 200): () => void {
  initMotion()
  owners.get(element)?.cancel()
  if (mode.value !== 'full' || document.hidden || !element.animate) return () => {}
  const animation = element.animate(frames, { duration: Math.min(duration, 1200), easing: 'cubic-bezier(0.22, 1, 0.36, 1)' })
  owners.set(element, animation)
  animations.add(animation)
  const cleanup = () => { animations.delete(animation); if (owners.get(element) === animation) owners.delete(element) }
  animation.addEventListener('finish', cleanup, { once: true })
  animation.addEventListener('cancel', cleanup, { once: true })
  return () => { animation.cancel(); cleanup() }
}

if (import.meta.hot) import.meta.hot.dispose(() => {
  cancelMotion()
  query?.removeEventListener('change', updateSystem)
  window.removeEventListener('storage', storageChanged)
  window.removeEventListener('focus', updateSystem)
  document.removeEventListener('visibilitychange', visibilityChanged)
  document.removeEventListener('pointerdown', cancelMotion, true)
  document.removeEventListener('keydown', cancelMotion, true)
})
