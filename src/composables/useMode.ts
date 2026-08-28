// Learner ⇄ Instructor mode.
//
// Modes apply to accounts that carry the instructor role; everybody is a
// learner, so learner mode is always available and is the default.
// The active mode is a per-profile device setting (`ui.active_mode`)
// so it survives restarts without following the user across devices.

import { computed } from 'vue'

import { useAccountStatus } from './useAccountStatus'
import { useSetting } from './useSettings'

export type AppMode = 'learner' | 'instructor'

const modeSetting = useSetting<string>('ui.active_mode')

export function useMode() {
  const { isInstructor } = useAccountStatus()

  const canSwitchModes = computed(() => isInstructor.value)

  const mode = computed<AppMode>(() => {
    // Only instructor accounts ever surface instructor mode; a role
    // downgrade (or another profile's leftover setting) falls back
    // to learner without touching the stored value.
    if (!isInstructor.value) return 'learner'
    return modeSetting.ref.value === 'instructor' ? 'instructor' : 'learner'
  })

  const isInstructorMode = computed(() => mode.value === 'instructor')

  async function setMode(next: AppMode): Promise<void> {
    if (next === 'instructor' && !canSwitchModes.value) return
    await modeSetting.set(next)
  }

  async function toggleMode(): Promise<void> {
    await setMode(mode.value === 'instructor' ? 'learner' : 'instructor')
  }

  return { mode, isInstructorMode, canSwitchModes, setMode, toggleMode }
}
