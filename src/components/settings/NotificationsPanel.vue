<script setup lang="ts">
// Settings → System → Notifications. Two switches from the settings registry
// (`notifications.enabled`, `notifications.push_enabled`) and, on a phone,
// the state of this device's push registration with Alexandria Cloud.
import { computed, onMounted, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { AppButton, AppSwitch } from '@/components/ui'
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
  unregisterPush,
  requestPermission,
} = useNotifications()

const busy = ref(false)
const permission = ref<string | null>(null)

onMounted(() => {
  void refreshPushStatus()
})

// Flipping the push switch registers or unregisters right away: a switch that
// only takes effect at the next unlock is a switch that looks broken.
watch(() => pushEnabled.ref.value, async (next, previous) => {
  if (previous === null || next === null || next === previous) return
  if (!isMobilePlatform) return
  await (next ? doRegister() : doUnregister())
})

const statusLine = computed(() => {
  const s = pushStatus.value
  if (!s) return ''
  if (!s.supported) return t('notifications.settings.pushUnsupported')
  if (!s.cloudOrigin) return t('notifications.settings.pushNoCloud')
  return s.registered
    ? t('notifications.settings.pushRegistered', { origin: s.cloudOrigin })
    : t('notifications.settings.pushNotRegistered')
})

async function doRegister() {
  busy.value = true
  try {
    await registerPush()
  } catch {
    await refreshPushStatus()
  } finally {
    busy.value = false
  }
}

async function doUnregister() {
  busy.value = true
  try {
    await unregisterPush()
  } catch {
    await refreshPushStatus()
  } finally {
    busy.value = false
  }
}

async function askPermission() {
  permission.value = await requestPermission()
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
          :model-value="pushEnabled.ref.value ?? true"
          :label="$t('notifications.settings.push')"
          :disabled="!isMobilePlatform || busy"
          @update:model-value="(v: boolean) => pushEnabled.set(v)"
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
            @click="doUnregister"
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
