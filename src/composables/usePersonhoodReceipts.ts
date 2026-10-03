import { readonly, ref } from 'vue'
import { useLocalApi } from '@/composables/useLocalApi'
import type { PersonhoodPrivateReceipt } from '@/types'

const receipts = ref<PersonhoodPrivateReceipt[]>([])
const pending = ref(false)
const error = ref<string | null>(null)
let generation = 0
let active: { id: string | null; cancelled: boolean } | null = null

export function usePersonhoodReceipts() {
  const { invoke } = useLocalApi()

  async function refresh() {
    const current = generation
    try {
      const values = await invoke<PersonhoodPrivateReceipt[]>('personhood_receipt_list')
      if (generation === current) receipts.value = values
    } catch (cause) {
      if (generation === current) error.value = String(cause)
    }
  }

  async function start() {
    if (active) return
    const job = { id: null as string | null, cancelled: false }
    active = job
    const current = ++generation
    pending.value = true
    error.value = null
    try {
      job.id = await invoke<string>('personhood_receipt_prepare')
      if (job.cancelled || current !== generation) {
        await invoke<void>('personhood_receipt_cancel', { challengeId: job.id })
        return
      }
      await invoke<PersonhoodPrivateReceipt>('personhood_receipt_prove', { challengeId: job.id })
      if (current === generation) await refresh()
    } catch (cause) {
      if (current === generation && !job.cancelled) error.value = String(cause)
    } finally {
      if (active === job) active = null
      if (current === generation) pending.value = false
    }
  }

  async function cancel(clear = false) {
    const job = active
    if (job) {
      job.cancelled = true
      if (job.id) {
        try { await invoke<void>('personhood_receipt_cancel', { challengeId: job.id }) }
        catch (cause) { if (!clear) error.value = String(cause) }
      }
    }
    if (clear) {
      generation++
      receipts.value = []
      error.value = null
      pending.value = false
    }
  }

  return { receipts: readonly(receipts), pending: readonly(pending), error: readonly(error), start, cancel, refresh }
}
