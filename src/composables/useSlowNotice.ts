import { onBeforeUnmount, readonly, ref, watch, type WatchSource } from 'vue'

/** About two seconds: ordinary starts and unlocks finish before it. */
export const SLOW_NOTICE_MS = 2000

/**
 * True once `active` has stayed true for `delay` ms. Callers show it as an
 * extra line under the current step; it never replaces or invents a step.
 */
export function useSlowNotice(active: WatchSource<boolean>, delay = SLOW_NOTICE_MS) {
  const slow = ref(false)
  let timer: ReturnType<typeof setTimeout> | undefined
  function reset(): void {
    if (timer !== undefined) clearTimeout(timer)
    timer = undefined
    slow.value = false
  }
  watch(active, value => {
    reset()
    if (value) timer = setTimeout(() => { timer = undefined; slow.value = true }, delay)
  }, { immediate: true })
  onBeforeUnmount(reset)
  return readonly(slow)
}
