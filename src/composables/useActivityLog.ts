import { readonly, ref } from 'vue'
import { useLocalApi } from '@/composables/useLocalApi'
import { onProfileLocked } from '@/composables/useProfiles'
import type { LearningRecordEntry, LearningRecordExport, LearningRecordUsage, SendFailure } from '@/types'

/**
 * The activity log: the learning records the app wrote about the learner,
 * read a page at a time, with the log's size against its limit and the
 * records a recipient refused (`record_send_failures`).
 *
 * Module-level state so the page and anything that adds to it see the same
 * rows. Cleared when the profile locks.
 */

/** Rows read per page. */
export const ACTIVITY_LOG_PAGE = 100

const entries = ref<LearningRecordEntry[]>([])
const usage = ref<LearningRecordUsage | null>(null)
const loading = ref(false)
/** Records a recipient refused at their last try, newest first. */
const failures = ref<SendFailure[]>([])

onProfileLocked(() => {
  entries.value = []
  usage.value = null
  failures.value = []
})

export function useActivityLog() {
  const { invoke } = useLocalApi()

  async function readUsage() {
    usage.value = await invoke<LearningRecordUsage>('learning_records_usage')
  }

  /** Reads the size, the send failures and the first page again. */
  async function refresh() {
    loading.value = true
    try {
      const [first, refused] = await Promise.all([
        invoke<LearningRecordEntry[]>('learning_records_list', { offset: 0, limit: ACTIVITY_LOG_PAGE }),
        invoke<SendFailure[]>('record_send_failures'),
        readUsage(),
      ])
      entries.value = first
      failures.value = refused
    } finally {
      loading.value = false
    }
  }

  /** Appends the next page, skipping rows already shown. */
  async function loadMore() {
    const next = await invoke<LearningRecordEntry[]>('learning_records_list', {
      offset: entries.value.length,
      limit: ACTIVITY_LOG_PAGE,
    })
    const shown = new Set(entries.value.map(entry => entry.id))
    entries.value = [...entries.value, ...next.filter(entry => !shown.has(entry.id))]
  }

  /** The statement file and its signature, ready to save unchanged. */
  function exportLog(): Promise<LearningRecordExport> {
    return invoke<LearningRecordExport>('learning_records_export')
  }

  /** Deletes these records from this device. */
  async function remove(ids: string[]) {
    await invoke<number>('learning_records_delete', { ids })
    const removed = new Set(ids)
    entries.value = entries.value.filter(entry => !removed.has(entry.id))
    // A deleted record is never sent, so its failures go with it.
    failures.value = failures.value.filter(failure => !removed.has(failure.record_id))
    await readUsage()
  }

  /** Deletes every record from this device. */
  async function removeAll() {
    await invoke<number>('learning_records_delete_all')
    entries.value = []
    failures.value = []
    await readUsage()
  }

  return {
    entries: readonly(entries),
    usage: readonly(usage),
    failures: readonly(failures),
    loading: readonly(loading),
    refresh,
    loadMore,
    exportLog,
    remove,
    removeAll,
  }
}
