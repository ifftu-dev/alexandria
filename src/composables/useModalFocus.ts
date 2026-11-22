import { nextTick, onMounted, onUnmounted, ref, watch } from 'vue'
import type { Ref } from 'vue'

interface ModalEntry {
  element: Ref<HTMLElement | null>
  layer: Ref<number>
  previousFocus: HTMLElement | null
}

const modalStack: ModalEntry[] = []
let previousOverflow = ''
const focusableSelector = 'a[href], button, input:not([type="hidden"]), select, textarea, [tabindex]'

export function useModalFocus(
  isOpen: () => boolean,
  element: Ref<HTMLElement | null>,
  close: () => void,
) {
  const entry: ModalEntry = { element, layer: ref(0), previousFocus: null }

  function focusableElements(): HTMLElement[] {
    return Array.from(element.value?.querySelectorAll<HTMLElement>(focusableSelector) ?? [])
      .filter((target) => target.tabIndex >= 0 && !target.matches(':disabled') && target.getClientRects().length > 0)
  }

  function release() {
    const index = modalStack.indexOf(entry)
    if (index < 0) return
    const wasTop = index === modalStack.length - 1
    modalStack.splice(index, 1)
    if (modalStack.length === 0) document.body.style.overflow = previousOverflow
    if (wasTop && entry.previousFocus?.isConnected) entry.previousFocus.focus()
    entry.previousFocus = null
  }

  watch(isOpen, async (open) => {
    if (!open) {
      release()
      return
    }
    entry.previousFocus = document.activeElement instanceof HTMLElement ? document.activeElement : null
    if (modalStack.length === 0) {
      previousOverflow = document.body.style.overflow
      document.body.style.overflow = 'hidden'
    }
    entry.layer.value = (modalStack[modalStack.length - 1]?.layer.value ?? 0) + 1
    modalStack.push(entry)
    await nextTick()
    if (!isOpen() || modalStack[modalStack.length - 1] !== entry) return
    const targets = focusableElements()
    const initial = targets.find((target) => target.matches('[autofocus], [data-modal-initial-focus]'))
      ?? targets.find((target) => target.matches('input, select, textarea'))
      ?? targets[0]
      ?? element.value
    initial?.focus({ preventScroll: true })
  }, { immediate: true, flush: 'post' })

  function onKeydown(event: KeyboardEvent) {
    if (!isOpen() || modalStack[modalStack.length - 1] !== entry) return
    if (event.key === 'Escape') {
      event.preventDefault()
      // Every mounted dialog listens on document. Consume this Escape before focus returns to the parent.
      event.stopImmediatePropagation()
      close()
    } else if (event.key === 'Tab') {
      const targets = focusableElements()
      const first = targets[0]
      const last = targets[targets.length - 1]
      if (!first || !last) {
        event.preventDefault()
        element.value?.focus()
      } else if (event.shiftKey && (document.activeElement === first || !element.value?.contains(document.activeElement))) {
        event.preventDefault()
        last.focus()
      } else if (!event.shiftKey && (document.activeElement === last || !element.value?.contains(document.activeElement))) {
        event.preventDefault()
        first.focus()
      }
    }
  }

  onMounted(() => document.addEventListener('keydown', onKeydown))
  onUnmounted(() => {
    document.removeEventListener('keydown', onKeydown)
    release()
  })

  return entry.layer
}
