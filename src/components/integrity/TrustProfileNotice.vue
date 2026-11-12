<script setup lang="ts">
// The learner's pre-attempt notice: which trust profile the backend chose for
// this session and why, whether the sponsor's minimum is met, and what would
// raise the profile next time (a paired phone, the camera, a faster device).
import { computed } from 'vue'
import { isMobilePlatform } from '@/composables/usePlatform'
import { profileNotice } from '@/utils/sentinel/profileNotice'
import type { DeepReadonly } from 'vue'
import type { SelectedProfile } from '@/types'

const props = defineProps<{ selected: DeepReadonly<SelectedProfile> }>()

const notice = computed(() => profileNotice(props.selected, { mobile: isMobilePlatform }))
</script>

<template>
  <div
    class="space-y-2 rounded-lg border p-3 text-sm"
    :class="notice.floor.satisfied ? 'border-border bg-card' : 'border-warning/40 bg-warning/5'"
    data-testid="trust-profile-notice"
  >
    <p class="font-medium text-foreground">
      {{ $t('sentinel.trust.notice.title', { profile: $t(`sentinel.trust.profile.${notice.profile}`) }) }}
    </p>
    <p class="text-muted-foreground">{{ $t(`sentinel.trust.profileHint.${notice.profile}`) }}</p>
    <ul v-if="notice.notRun.length" class="space-y-0.5 text-muted-foreground">
      <li v-for="item in notice.notRun" :key="item.signal">
        {{ $t('sentinel.trust.notice.notRun', {
          signal: $t(`sentinel.trust.signal.${item.signal}`),
          reason: item.reasons.map(r => $t(`sentinel.trust.reason.${r}`)).join('; '),
        }) }}
      </li>
    </ul>
    <p v-if="notice.floor.required" :class="notice.floor.satisfied ? 'text-success' : 'font-medium text-warning'" data-testid="trust-profile-floor">
      {{ $t(notice.floor.satisfied ? 'sentinel.trust.notice.floorMet' : 'sentinel.trust.notice.floorUnmet', {
        required: $t(`sentinel.trust.profile.${notice.floor.required}`),
      }) }}
    </p>
    <div v-if="notice.raise.length">
      <p class="text-xs font-medium text-foreground">{{ $t('sentinel.trust.notice.raiseTitle') }}</p>
      <ul class="mt-0.5 list-disc space-y-0.5 ps-5 text-xs text-muted-foreground">
        <li v-for="step in notice.raise" :key="step">{{ $t(`sentinel.trust.raise.${step}`) }}</li>
      </ul>
    </div>
    <p class="text-xs text-muted-foreground">{{ $t('sentinel.trust.notice.monitoringLimit') }}</p>
  </div>
</template>
