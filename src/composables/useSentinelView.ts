import { readonly, ref } from 'vue'

const visible = ref(false)
export function useSentinelView() {
  return {
    visible: readonly(visible),
    toggle() { visible.value = !visible.value },
    close() { visible.value = false },
  }
}
