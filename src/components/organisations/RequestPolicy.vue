<script setup lang="ts">
/**
 * The organisation's issuance policy on an assessment request, in plain words,
 * before the candidate starts: what an answer must meet for the organisation
 * to issue its own credential, and whether this device can run the monitoring
 * profile it asks for. A required `device_attested` is taken on a phone that
 * attests itself; on a device that cannot reach it (a computer, paired phone
 * or not) the card says where to take it instead. The policy comes from the
 * request itself, which the organisation's proof covers.
 */
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import { floorReach, policyRequirements, type DeviceAbility, type Requirement } from '@/utils/requestPolicy'
import { needsVerifiedDevice, verifiedPhones } from '@/utils/verifiedDevice'
import type { IssuancePolicy, TrustProfile } from '@/types'

const props = defineProps<{ policy: IssuancePolicy; org: string; device: DeviceAbility }>()
const { t } = useI18n()

const items = computed(() => policyRequirements(props.policy))
const floor = computed<TrustProfile | null>(() => props.policy.min_trust_profile ?? null)
const reach = computed(() => floorReach(floor.value, props.device))
const profileName = computed(() => (floor.value ? t(`sentinel.trust.profile.${floor.value}`) : ''))
/** The policy requires a verified device and this one is known not to be. */
const verifiedDeviceBlocked = computed(
  () => needsVerifiedDevice(props.policy.required_assurance_level) && props.device.support?.verified_device === false,
)
const phones = computed(() => verifiedPhones(props.device.support))

function label(r: Requirement): string {
  switch (r.kind) {
    case 'clean': return t('organisations.requests.assessment.policy.clean')
    case 'minIntegrity': return t('organisations.requests.assessment.policy.minIntegrity', { percent: r.percent })
    case 'maxCritical':
      return r.count === 0
        ? t('organisations.requests.assessment.policy.noCritical')
        : t('organisations.requests.assessment.policy.maxCritical', { count: r.count })
    case 'maxWarning':
      return r.count === 0
        ? t('organisations.requests.assessment.policy.noWarning')
        : t('organisations.requests.assessment.policy.maxWarning', { count: r.count })
    case 'assurance':
      if (r.level === 'local') return t('organisations.requests.assessment.policy.assuranceLocal')
      if (r.level === 'device_attested') return t('organisations.requests.assessment.policy.assuranceAttested')
      if (r.level === 'companion_attested') return t('organisations.requests.assessment.policy.assuranceCompanion')
      return t('organisations.requests.assessment.policy.assuranceOther', { level: r.level })
    case 'floor':
      return t('organisations.requests.assessment.policy.floor', { profile: t(`sentinel.trust.profile.${r.profile}`) })
    case 'rung':
      return t('organisations.requests.assessment.policy.rung', { rung: t(`sentinel.trust.rung.${r.rung}`) })
    case 'probes':
      return t('organisations.requests.assessment.policy.probes', { policy: t(`sentinel.trust.probePolicy.${r.policy}`) })
  }
}
</script>

<template>
  <div class="space-y-1 rounded-lg border border-border bg-muted/30 p-3 text-xs" data-testid="request-policy">
    <p v-if="!items.length" class="text-muted-foreground">{{ t('organisations.requests.assessment.policy.empty', { org }) }}</p>
    <template v-else>
      <p class="font-medium text-foreground">{{ t('organisations.requests.assessment.policy.heading', { org }) }}</p>
      <ul class="list-disc space-y-0.5 ps-5 text-muted-foreground">
        <li v-for="r in items" :key="r.kind" :data-testid="`requirement-${r.kind}`">{{ label(r) }}</li>
      </ul>
    </template>
    <p v-if="verifiedDeviceBlocked" class="font-medium text-warning" data-testid="verified-device-blocker">
      {{ t(`organisations.requests.assessment.policy.verifiedDevice.${phones}`) }}
    </p>
    <p v-if="reach.blockers.includes('no_camera')" class="font-medium text-warning" data-testid="floor-blocker-camera">
      {{ t('organisations.requests.assessment.policy.noCamera', { profile: profileName }) }}
    </p>
    <p
      v-if="reach.blockers.includes('no_attestation') || reach.pairPhone"
      class="font-medium text-warning"
      data-testid="floor-blocker-attestation"
    >
      {{ t('organisations.requests.assessment.policy.noAttestation', { profile: profileName }) }}
      <template v-if="reach.pairPhone">{{ t('organisations.requests.assessment.policy.pairPhone') }}</template>
    </p>
  </div>
</template>
