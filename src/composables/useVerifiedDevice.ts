// The verified-device gate an attempt page shows before the first item: a
// role or request whose policy requires `device_attested` is taken on a
// phone that attests itself. The page passes whether its role or request
// requires it; this asks the backend what the device can reach, keeps the
// phones a backend refusal named, and says whether starting is known to be
// blocked here. Unknown support never blocks: the backend still refuses.
// Per page, like `useCameraPresence`: one attempt's refusal is not
// another's.
import { computed, readonly, ref, type Ref } from 'vue'
import { useLocalApi } from '@/composables/useLocalApi'
import { verifiedDeviceRefusal, verifiedPhones, type VerifiedPhones } from '@/utils/verifiedDevice'
import type { DeviceSupport } from '@/types'

export function useVerifiedDevice(required: Ref<boolean>) {
  const { invoke } = useLocalApi()
  /** What this device can reach; asked only when the page needs it. */
  const support = ref<DeviceSupport | null>(null)
  /** The phones the backend's `verified_device_required` refusal named. */
  const refused = ref<VerifiedPhones | null>(null)

  const phones = computed<VerifiedPhones>(() => refused.value ?? verifiedPhones(support.value))
  /** Known not to reach `device_attested` here. */
  const blocked = computed(() =>
    required.value && (refused.value !== null || support.value?.verified_device === false),
  )

  async function detect(): Promise<void> {
    try {
      support.value = await invoke<DeviceSupport>('integrity_device_support')
    } catch {
      /* unknown: nothing is claimed; the backend still refuses */
    }
  }

  /** Keep the phones `cause` names when it is the verified-device refusal;
   *  true when it was. */
  function refuse(cause: unknown): boolean {
    const phonesNamed = verifiedDeviceRefusal(cause)
    if (phonesNamed) refused.value = phonesNamed
    return phonesNamed !== null
  }

  return { support: readonly(support), phones, blocked, detect, refuse }
}
