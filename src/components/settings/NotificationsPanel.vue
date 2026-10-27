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
const permission = ref<string | null>(null)

onMounted(() => {
  void refreshPushStatus()
})

const statusLine = computed(() => {
  const s = pushStatus.value
  if (!s) return ''
  if (s.pendingCleanup) return t('notifications.settings.pushCleanupPending')
  if (s.pendingRegistration) return t('notifications.settings.pushRegistrationPending')
  if (!s.supported) return t('notifications.settings.pushUnsupported')
  if (!s.cloudOrigin) return t('notifications.settings.pushNoCloud')
  return s.registered
    ? t('notifications.settings.pushRegistered', { origin: s.cloudOrigin })
    : t('notifications.settings.pushNotRegistered')
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
        <p v-if="pushStatus.lastError" class="text-xs text-destructive">
          {{ $t('notifications.settings.pushError', { error: pushStatus.lastError }) }}
        </p>
        <div v-if="pushStatus.supported && pushStatus.cloudOrigin" class="flex flex-wrap items-center gap-2">
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
        <span v-if="permission === 'granted'" class="text-xs text-muted-foreground">
          {{ $t('notifications.settings.permissionGranted') }}
        </span>
        <span v-else-if="permission === 'denied'" class="text-xs text-destructive">
          {{ $t('notifications.settings.permissionDenied') }}
        </span>
      </div>
    </div>
  </div>
</template>
