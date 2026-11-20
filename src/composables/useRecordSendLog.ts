import { readonly, ref } from 'vue'
import { useLocalApi } from '@/composables/useLocalApi'
import { onProfileLocked } from '@/composables/useProfiles'
import type { RecordAccessEntry } from '@/types'

/**
 * What this device sent of the learner's records, and to whom: the local
 * half of the access log (`record_access_log`).
 *
 * Module-level state, so every view of the log (the record destinations
 * panel, an organisation's access page) shows the same rows, and whatever
 * sends or voids records refreshes them all at once. Cleared when the
 * profile locks.
 */

const entries = ref<RecordAccessEntry[]>([])
const error = ref('')

onProfileLocked(() => {
  entries.value = []
  error.value = ''
})

export function useRecordSendLog() {
  const { invoke } = useLocalApi()

  /** Reads the log again; a failure is kept in `error`. */
  async function refresh() {
    try {
      entries.value = (await invoke<RecordAccessEntry[] | null>('record_access_log')) ?? []
      error.value = ''
    } catch (e) {
      error.value = String(e)
    }
  }

  return { entries: readonly(entries), error: readonly(error), refresh }
}
