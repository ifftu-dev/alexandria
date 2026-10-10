import { computed, readonly, ref } from 'vue'
import { useLocalApi } from '@/composables/useLocalApi'
import type { PersonhoodLabAction, PersonhoodLabStatus } from '@/types'

const status = ref<PersonhoodLabStatus | null>(null)
const pending = ref(false)
const error = ref<string | null>(null)
let generation = 0
let actionQueue: Promise<void> = Promise.resolve()

export function usePersonhoodLab() {
  const { invoke } = useLocalApi()
  const busy = computed(() => status.value !== null &&
    ['downloading', 'checking', 'witness', 'proving', 'verifying', 'cancelling'].includes(status.value.phase))

  async function refresh() {
    if (pending.value) return
    const current = generation
    try {
      const next = await invoke<PersonhoodLabStatus>('personhood_lab_status')
      if (current === generation) status.value = next
    } catch (cause) {
      if (current === generation) error.value = String(cause)
    }
  }

  async function act(action: PersonhoodLabAction) {
    if (pending.value && action !== 'cancel') return
    const current = ++generation
    pending.value = true
    error.value = null
    const operation = actionQueue.then(() =>
      invoke<PersonhoodLabStatus>('personhood_lab_action', { action }))
    actionQueue = operation.then(() => undefined, () => undefined)
    try {
      const next = await operation
      if (current === generation) status.value = next
    } catch (cause) {
      if (current === generation) error.value = String(cause)
    } finally {
      if (current === generation) pending.value = false
    }
  }

  async function cancel() {
    if (status.value?.enabled && (busy.value || pending.value)) await act('cancel')
  }

  return { status: readonly(status), pending: readonly(pending), error: readonly(error), busy, refresh, act, cancel }
}
