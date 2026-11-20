import { readonly, ref } from 'vue'
import { useLocalApi } from '@/composables/useLocalApi'
import { onProfileLocked } from '@/composables/useProfiles'
import type { LearningRecordEntry, LearningRecordExport, LearningRecordUsage } from '@/types'

/**
 * The activity log: the learning records the app wrote about the learner,
 * read a page at a time, with the log's size against its limit.
 *
 * Module-level state so the page and anything that adds to it (send status,
 * for example) see the same rows. Cleared when the profile locks.
 */

/** Rows read per page. */
export const ACTIVITY_LOG_PAGE = 100

const entries = ref<LearningRecordEntry[]>([])
const usage = ref<LearningRecordUsage | null>(null)
const loading = ref(false)

onProfileLocked(() => {
  entries.value = []
  usage.value = null
})

export function useActivityLog() {
  const { invoke } = useLocalApi()

  async function readUsage() {
    usage.value = await invoke<LearningRecordUsage>('learning_records_usage')
  }

  /** Reads the size and the first page again. */
  async function refresh() {
    loading.value = true
    try {
      const [first] = await Promise.all([
        invoke<LearningRecordEntry[]>('learning_records_list', { offset: 0, limit: ACTIVITY_LOG_PAGE }),
        readUsage(),
      ])
      entries.value = first
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
    await readUsage()
  }

  /** Deletes every record from this device. */
  async function removeAll() {
    await invoke<number>('learning_records_delete_all')
    entries.value = []
    await readUsage()
  }

  return {
    entries: readonly(entries),
    usage: readonly(usage),
    loading: readonly(loading),
    refresh,
    loadMore,
    exportLog,
    remove,
    removeAll,
  }
}
