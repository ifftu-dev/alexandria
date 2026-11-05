<script setup lang="ts">
// Settings → System → Notifications. Two switches from the settings registry
// (`notifications.enabled`, `notifications.push_enabled`) and, on a phone,
// the state of this device's push registration with Alexandria Cloud.
import { computed, onMounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { AppButton, AppSwitch } from '@/components/ui'
import { getProfileSessionToken } from '@/composables/profileSession'
import { useSetting } from '@/composables/useSettings'
import { useNotifications } from '@/composables/useNotifications'
import { isMobilePlatform } from '@/composables/usePlatform'
import type { MessageKey } from '@/i18n/keys'
import type { NotificationPermissionOutcome, PushErrorCode } from '@/types'

const { t } = useI18n()
const enabled = useSetting<boolean>('notifications.enabled')
const pushEnabled = useSetting<boolean>('notifications.push_enabled')
const {
  pushStatus,
  refreshPushStatus,
  registerPush,
  setPushEnabled,
  requestPermission,
} = useNotifications()

const busy = ref(false)
const permission = ref<NotificationPermissionOutcome | null>(null)

onMounted(() => {
  void refreshPushStatus()
})

const statusLine = computed(() => {
  const s = pushStatus.value
  if (!s) return ''
  if (s.pending_cleanup) return t('notifications.settings.pushCleanupPending')
  if (s.pending_registration) return t('notifications.settings.pushRegistrationPending')
  if (!s.supported) return t('notifications.settings.pushUnsupported')
  if (!s.cloud_origin) return t('notifications.settings.pushNoCloud')
  return s.registered
    ? t('notifications.settings.pushRegistered', { origin: s.cloud_origin })
    : t('notifications.settings.pushNotRegistered')
})

// Every code has its own message; a new backend code is a type error here
// until it is worded. The raw detail is diagnostics only and is not shown.
const PUSH_ERROR_MESSAGES: Record<PushErrorCode, MessageKey> = {
  permission_denied: 'notifications.settings.pushErrors.permission_denied',
  token_unavailable: 'notifications.settings.pushErrors.token_unavailable',
  insecure_origin: 'notifications.settings.pushErrors.insecure_origin',
  cloud_unreachable: 'notifications.settings.pushErrors.cloud_unreachable',
  cloud_rejected: 'notifications.settings.pushErrors.cloud_rejected',
  cloud_unacknowledged: 'notifications.settings.pushErrors.cloud_unacknowledged',
  invalid_response: 'notifications.settings.pushErrors.invalid_response',
}

// One message per outcome the backend returns.
const PERMISSION_MESSAGES: Record<NotificationPermissionOutcome, MessageKey> = {
  granted: 'notifications.settings.permissionGranted',
  denied: 'notifications.settings.permissionDenied',
  prompt_unavailable: 'notifications.settings.permissionPromptUnavailable',
}

const pushError = computed(() => {
  const code = pushStatus.value?.last_error_code
  return code ? t(PUSH_ERROR_MESSAGES[code]) : ''
})

async function doRegister() {
  const session = getProfileSessionToken()
  busy.value = true
  try {
    await registerPush()
  } catch {
    if (session === getProfileSessionToken()) await refreshPushStatus()
  } finally {
    if (session === getProfileSessionToken()) busy.value = false
  }
}

async function changePush(enabled: boolean) {
  const session = getProfileSessionToken()
  busy.value = true
  try {
    await setPushEnabled(enabled)
  } catch {
    if (session === getProfileSessionToken()) await refreshPushStatus()
  } finally {
    if (session === getProfileSessionToken()) busy.value = false
  }
}

async function askPermission() {
  const session = getProfileSessionToken()
  const result = await requestPermission()
  if (session === getProfileSessionToken()) permission.value = result
}
</script>

<template>
  <div>
    <h4 class="settings-group-title">{{ $t('notifications.settings.title') }}</h4>
    <div class="rounded-lg border border-border p-4 space-y-4">
      <p class="text-sm text-muted-foreground">{{ $t('notifications.settings.description') }}</p>

      <div class="flex items-start justify-between gap-4">
        <div class="min-w-0">
          <p class="text-sm font-medium text-foreground">{{ $t('notifications.settings.inApp') }}</p>
          <p class="text-xs text-muted-foreground mt-0.5">{{ $t('notifications.settings.inAppHint') }}</p>
        </div>
        <AppSwitch
          :model-value="enabled.ref.value ?? true"
          :label="$t('notifications.settings.inApp')"
          @update:model-value="(v: boolean) => enabled.set(v)"
        />
      </div>

      <div class="flex items-start justify-between gap-4">
        <div class="min-w-0">
          <p class="text-sm font-medium text-foreground">{{ $t('notifications.settings.push') }}</p>
          <p class="text-xs text-muted-foreground mt-0.5">{{ $t('notifications.settings.pushHint') }}</p>
        </div>
        <AppSwitch
          :model-value="pushStatus?.enabled ?? pushEnabled.ref.value ?? true"
          :label="$t('notifications.settings.push')"
          :disabled="!isMobilePlatform || busy"
          @update:model-value="changePush"
        />
      </div>

      <div v-if="pushStatus" class="space-y-2">
        <p class="text-xs text-muted-foreground">{{ statusLine }}</p>
        <p v-if="pushStatus.paused_by_organisations" class="text-xs text-muted-foreground" data-testid="push-paused-by-organisations">
          {{ $t('notifications.settings.pushPausedByOrganisations') }}
          <RouterLink to="/settings/organisations" class="text-primary underline">
            {{ $t('notifications.settings.pushPausedOpenOrganisations') }}
          </RouterLink>
        </p>
        <p v-if="pushError" class="text-xs text-destructive" data-testid="push-error">{{ pushError }}</p>
        <div v-if="pushStatus.supported && pushStatus.cloud_origin && !pushStatus.paused_by_organisations" class="flex flex-wrap items-center gap-2">
          <AppButton
            v-if="!pushStatus.registered"
            variant="outline"
            size="sm"
            :disabled="busy || !(pushEnabled.ref.value ?? true)"
            @click="doRegister"
          >
            {{ busy ? $t('notifications.settings.working') : $t('notifications.settings.register') }}
          </AppButton>
          <AppButton
            v-else
            variant="ghost"
            size="sm"
            :disabled="busy"
            @click="changePush(false)"
          >
            {{ busy ? $t('notifications.settings.working') : $t('notifications.settings.unregister') }}
          </AppButton>
        </div>
      </div>

      <div class="flex flex-wrap items-center gap-3">
        <AppButton variant="outline" size="sm" @click="askPermission">
          {{ $t('notifications.settings.permission') }}
        </AppButton>
        <span
          v-if="permission"
          class="text-xs"
          :class="permission === 'granted' ? 'text-muted-foreground' : 'text-destructive'"
          data-testid="permission-outcome"
        >
          {{ $t(PERMISSION_MESSAGES[permission]) }}
        </span>
      </div>
    </div>
  </div>
</template>
