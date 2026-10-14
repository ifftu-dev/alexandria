// The camera, microphone and speaker a call uses on this computer: chosen
// before joining, remembered for the next call on this device.
//
// Kept in browser storage because it is a per-device convenience: device ids
// mean nothing on another machine, and losing the choice only means picking
// again. A remembered device that has gone away falls back to automatic.
import { reactive, ref, watch } from 'vue'
import type { DeviceList } from '@/types'
import { useTutoringRoom } from '@/composables/useTutoringRoom'

const STORAGE_KEY = 'alexandria.call-devices'

export interface CallDeviceChoice {
  /** Camera index as `tutoring_list_devices` reports it; null = automatic. */
  camera: string | null
  mic: string | null
  speaker: string | null
}

function load(): CallDeviceChoice {
  try {
    const saved = JSON.parse(localStorage.getItem(STORAGE_KEY) ?? 'null') as Partial<CallDeviceChoice> | null
    return { camera: saved?.camera ?? null, mic: saved?.mic ?? null, speaker: saved?.speaker ?? null }
  } catch {
    return { camera: null, mic: null, speaker: null }
  }
}

function save(choice: CallDeviceChoice) {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(choice))
  } catch {
    // Storage can be unavailable; the choice still holds for this call.
  }
}

export function useCallDevices() {
  const tutoring = useTutoringRoom()
  const devices = ref<DeviceList | null>(null)
  const choice = reactive<CallDeviceChoice>(load())

  /** Read the devices again, and forget remembered ones that are gone. */
  async function refresh() {
    const list = await tutoring.listDevices()
    devices.value = list
    if (choice.camera && !list.cameras.some(c => c.index === choice.camera)) choice.camera = null
    if (choice.mic && !list.audio_inputs.some(d => d.id === choice.mic)) choice.mic = null
    if (choice.speaker && !list.audio_outputs.some(d => d.id === choice.speaker)) choice.speaker = null
  }

  watch(choice, () => save({ ...choice }), { deep: true })

  return { devices, choice, refresh }
}
