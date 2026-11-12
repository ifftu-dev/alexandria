import { readonly, ref } from 'vue'
import { useLocalApi } from '@/composables/useLocalApi'
import type {
  CameraFeatures,
  CompanionJoined,
  CompanionOfferResponse,
  CompanionPaired,
} from '@/types'

/**
 * Paired attested phone for a desktop Sentinel session.
 *
 * Desktop: `pairPhone(sessionId)` opens a one-time code (shown by
 * `CompanionPhoneOffer`), waits for the phone, and resolves with what the
 * backend stored, or null when the learner skipped or pairing failed.
 * Phone: `join(code)`, then `sendCamera(window)` when it is the camera.
 * The backend verifies the phone's attestation itself; nothing here is
 * trusted.
 */

const offer = ref<CompanionOfferResponse | null>(null)
const paired = ref<CompanionPaired | null>(null)
const joined = ref<CompanionJoined | null>(null)
const error = ref<string | null>(null)
let pendingSession: string | null = null

function message(e: unknown): string {
  return e instanceof Error ? e.message : String(e)
}

export function useCompanionPhone() {
  const { invoke } = useLocalApi()

  async function pairPhone(sessionId: string, camera = false): Promise<CompanionPaired | null> {
    error.value = null
    paired.value = null
    try {
      offer.value = await invoke<CompanionOfferResponse>('integrity_companion_offer', { sessionId, camera })
      pendingSession = sessionId
      const result = await invoke<CompanionPaired>('integrity_companion_await', { sessionId })
      paired.value = result
      return result
    } catch (e) {
      // A skip withdraws the code, which ends the wait with an error.
      if (pendingSession === sessionId) error.value = message(e)
      return null
    } finally {
      offer.value = null
      pendingSession = null
    }
  }

  /** Desktop: withdraw the open code and continue without the phone. */
  async function skip(): Promise<void> {
    const sessionId = pendingSession
    pendingSession = null
    offer.value = null
    if (sessionId) await invoke('integrity_companion_cancel', { sessionId })
  }

  async function join(code: string): Promise<CompanionJoined | null> {
    error.value = null
    try {
      joined.value = await invoke<CompanionJoined>('integrity_companion_join', { code: code.trim() })
      return joined.value
    } catch (e) {
      error.value = message(e)
      return null
    }
  }

  async function sendCamera(camera: CameraFeatures): Promise<void> {
    await invoke('integrity_companion_send_camera', { camera })
  }

  async function leave(): Promise<void> {
    joined.value = null
    await invoke('integrity_companion_leave')
  }

  function reset(): void {
    paired.value = null
    error.value = null
  }

  return {
    offer: readonly(offer),
    paired: readonly(paired),
    joined: readonly(joined),
    error: readonly(error),
    pairPhone,
    skip,
    join,
    sendCamera,
    leave,
    reset,
  }
}
