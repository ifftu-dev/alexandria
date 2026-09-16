import { readonly, ref } from 'vue'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'

import { useLocalApi } from './useLocalApi'
import { useSentinel } from './useSentinel'

interface DiagnosticsStatus {
  enabled: boolean
  open_assessment_count: number
}

type DiagnosticsAction = 'reload' | 'devtools' | 'sentinel' | 'install_cli'
type EntryPreparation = () => void | Promise<void>

const enabled = ref(false)
const promptOpen = ref(false)
const busy = ref(false)
const error = ref('')
const openAssessmentCount = ref(0)
const preparations = new Set<EntryPreparation>()
let eventUnlisten: UnlistenFn | null = null

export function useDiagnostics() {
  const { invoke } = useLocalApi()
  const sentinel = useSentinel()

  async function initialize(): Promise<void> {
    const status = await invoke<DiagnosticsStatus>('diagnostics_status')
    enabled.value = status.enabled
    openAssessmentCount.value = status.open_assessment_count
    if (!eventUnlisten) {
      eventUnlisten = await listen<boolean>('diagnostics://changed', event => {
        enabled.value = event.payload
        if (!event.payload) promptOpen.value = false
      })
    }
  }

  async function requestEntry(): Promise<void> {
    error.value = ''
    const status = await invoke<DiagnosticsStatus>('diagnostics_status')
    enabled.value = status.enabled
    openAssessmentCount.value = status.open_assessment_count
    if (!status.enabled) promptOpen.value = true
  }

  async function confirmEntry(): Promise<void> {
    if (busy.value) return
    busy.value = true
    error.value = ''
    try {
      for (const prepare of preparations) await prepare()
      if (sentinel.isActive.value) await sentinel.stop()
      const status = await invoke<DiagnosticsStatus>('diagnostics_enter')
      enabled.value = status.enabled
      openAssessmentCount.value = status.open_assessment_count
      promptOpen.value = false
    } catch (cause) {
      error.value = cause instanceof Error ? cause.message : String(cause)
      throw cause
    } finally {
      busy.value = false
    }
  }

  async function exit(): Promise<void> {
    if (busy.value) return
    busy.value = true
    error.value = ''
    try {
      await invoke('diagnostics_exit')
      enabled.value = false
      promptOpen.value = false
    } catch (cause) {
      error.value = cause instanceof Error ? cause.message : String(cause)
      throw cause
    } finally {
      busy.value = false
    }
  }

  async function runAction(action: DiagnosticsAction): Promise<void> {
    error.value = ''
    try {
      await invoke('diagnostics_run_action', { action })
    } catch (cause) {
      error.value = cause instanceof Error ? cause.message : String(cause)
      throw cause
    }
  }

  function cancelEntry(): void {
    if (!busy.value) {
      promptOpen.value = false
      error.value = ''
    }
  }

  function registerEntryPreparation(prepare: EntryPreparation): () => void {
    preparations.add(prepare)
    return () => preparations.delete(prepare)
  }

  function resetForProfileLock(): void {
    enabled.value = false
    promptOpen.value = false
    error.value = ''
    openAssessmentCount.value = 0
  }

  return {
    enabled: readonly(enabled),
    promptOpen: readonly(promptOpen),
    busy: readonly(busy),
    error: readonly(error),
    openAssessmentCount: readonly(openAssessmentCount),
    initialize,
    requestEntry,
    confirmEntry,
    cancelEntry,
    exit,
    runAction,
    registerEntryPreparation,
    resetForProfileLock,
  }
}
