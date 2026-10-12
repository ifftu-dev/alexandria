<script setup lang="ts">
/**
 * Settings → Organisations.
 *
 * One switch decides whether the app contacts Alexandria Cloud for requests at
 * all. Off, the section says so and shows nothing else: a list of requests
 * under a switch that stops them arriving would look stale rather than off.
 *
 * With no Cloud set for the network the inbox would be empty for a reason
 * that has nothing to do with whether anyone asked, so that is said plainly
 * instead of showing an empty inbox.
 */
import { computed, onMounted, ref } from 'vue'
import { useRouter } from 'vue-router'
import { useI18n } from 'vue-i18n'
import { useLocalApi } from '@/composables/useLocalApi'
import { usePanel } from '@/composables/usePanel'
import { useSettings } from '@/composables/useSettings'
import { AppButton, AppInput, AppSwitch } from '@/components/ui'
import OrgRequests from './OrgRequests.vue'
import OrgAccess from './OrgAccess.vue'

const ENABLED_KEY = 'organisations.enabled'

const props = defineProps<{
  /** Where the Cloud address can be changed: the Developer section when it is shown, else All settings. */
  cloudSettingsSection: 'developer' | 'advanced'
}>()

const { invoke } = useLocalApi()
const { t } = useI18n()
const router = useRouter()
const { byKey, ready, initialize, setSetting } = useSettings()

const cloud = ref<string | null | undefined>(undefined)
const error = ref('')
const switching = ref(false)
const accessPanel = ref<InstanceType<typeof OrgAccess> | null>(null)

// A panelist's join link normally arrives as a deep link. Where the system
// does not hand it to the app (a link pasted into chat, a build that is not
// the registered handler), pasting it here does the same thing.
const panel = usePanel()
const panelLink = ref('')
const panelError = ref('')
const openingPanel = ref(false)

async function openPanelLink() {
  openingPanel.value = true
  panelError.value = ''
  try {
    const seat = await panel.openLink(panelLink.value.trim())
    await router.push(`/panel/${seat.sessionId}`)
  } catch (e) {
    panelError.value = String(e)
  } finally {
    openingPanel.value = false
  }
}

const enabled = computed(() => {
  const entry = byKey.value.get(ENABLED_KEY)
  return entry ? entry.current_value === 'true' || entry.current_value === '1' : true
})

async function loadCloud() {
  error.value = ''
  try {
    cloud.value = await invoke<string | null>('org_cloud')
  } catch (e) {
    cloud.value = null
    error.value = String(e)
  }
}

async function setEnabled(value: boolean) {
  switching.value = true
  error.value = ''
  try {
    await setSetting(ENABLED_KEY, value ? 'true' : 'false')
    await loadCloud()
  } catch (e) {
    error.value = String(e)
  } finally {
    switching.value = false
  }
}

onMounted(async () => {
  try {
    await initialize()
  } catch {
    /* the switch falls back to its default (on) */
  }
  await loadCloud()
})
</script>

<template>
  <div class="space-y-5">
    <div class="flex items-start gap-3 rounded-xl border border-border bg-card p-4">
      <AppSwitch
        :model-value="enabled"
        :disabled="switching || !ready"
        :label="t('organisations.settings.enabled')"
        @update:model-value="setEnabled"
      />
      <span class="min-w-0 flex-1">
        <span class="block text-sm font-medium text-foreground">{{ t('organisations.settings.enabled') }}</span>
        <span class="block text-xs text-muted-foreground">{{ t('organisations.settings.enabledDesc') }}</span>
      </span>
    </div>

    <p v-if="error" role="alert" class="text-sm text-destructive">{{ error }}</p>

    <p v-if="!enabled" class="text-sm text-muted-foreground" data-testid="organisations-off">
      {{ t('organisations.settings.off') }}
    </p>

    <div v-else-if="cloud === null" class="space-y-2 rounded-xl border border-border p-4" data-testid="organisations-no-cloud">
      <p class="text-sm text-foreground">{{ t('organisations.settings.noCloud') }}</p>
      <p class="text-xs text-muted-foreground">{{ t('organisations.settings.noCloudDev') }}</p>
      <AppButton size="sm" variant="outline" @click="router.push(`/settings/${props.cloudSettingsSection}`)">
        {{ t('organisations.settings.openDeveloper') }}
      </AppButton>
    </div>

    <template v-else-if="cloud">
      <OrgRequests @shared="accessPanel?.refresh()" />
      <OrgAccess ref="accessPanel" :cloud-url="cloud" />
    </template>

    <form class="space-y-2 rounded-xl border border-border p-4" data-testid="panel-link" @submit.prevent="openPanelLink">
      <p class="text-sm font-medium text-foreground">{{ t('organisations.settings.panelTitle') }}</p>
      <p class="text-xs text-muted-foreground">{{ t('organisations.settings.panelDesc') }}</p>
      <div class="flex gap-2">
        <AppInput v-model="panelLink" class="flex-1" :placeholder="t('organisations.settings.panelPlaceholder')" :aria-label="t('organisations.settings.panelTitle')" />
        <AppButton type="submit" size="sm" :loading="openingPanel" :disabled="!panelLink.trim()">
          {{ t('organisations.settings.panelOpen') }}
        </AppButton>
      </div>
      <p v-if="panelError" role="alert" class="text-xs text-destructive">{{ panelError }}</p>
    </form>
  </div>
</template>
