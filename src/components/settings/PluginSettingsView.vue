<script setup lang="ts">
/**
 * Settings → Plugins → one plugin (`/settings/plugins/:cid`).
 *
 * Header (icon, name, version, author, description, enable toggle, guide,
 * donate, uninstall), the form generated from the manifest's `settings`
 * array, the plugin's permissions with revoke, and its dependency edges.
 * Rendered inside the Settings shell by `pages/Settings.vue`.
 *
 * Setting labels and descriptions come from the plugin author and render as
 * plain text. `?setting=<key>` scrolls to that setting and highlights it
 * (the global Settings search links here that way).
 */
import { ref, computed, onMounted, watch, nextTick } from 'vue'
import { useI18n } from 'vue-i18n'
import { useRoute, useRouter } from 'vue-router'
import { useLocalApi } from '@/composables/useLocalApi'
import { useDisplayNames } from '@/composables/useDisplayNames'
import { animateMotion, useMotion } from '@/composables/useMotion'
import { pluginMonogram, pluginThumbGradient } from '@/utils/pluginThumb'
import { AppButton, AppBadge, AppSpinner, AppAlert, AppModal, AppSwitch } from '@/components/ui'
import SpeechPacksSetting from '@/components/settings/SpeechPacksSetting.vue'
import type {
  InstalledPlugin,
  PluginCapability,
  PluginManifest,
  PluginPermissionRecord,
  PluginSettingSpec,
  PluginSettingType,
  PluginSettingValues,
} from '@/types'

const props = defineProps<{ cid: string }>()
const emit = defineEmits<{
  /** Something the Settings search index shows changed (enabled, removed). */
  (e: 'changed'): void
}>()

const { t } = useI18n()
const route = useRoute()
const router = useRouter()
const { invoke } = useLocalApi()
const { displayName, ensureNames } = useDisplayNames()
const { reduced: motionReduced } = useMotion()

const loading = ref(true)
const loadError = ref<string | null>(null)
const actionError = ref<string | null>(null)
const plugin = ref<InstalledPlugin | null>(null)
const manifest = ref<PluginManifest | null>(null)
const allPlugins = ref<InstalledPlugin[]>([])
const allManifests = ref<Record<string, PluginManifest>>({})
const permissions = ref<PluginPermissionRecord[]>([])
const values = ref<PluginSettingValues>({})
const thumbnail = ref('')
const status = ref('')
const confirmingUninstall = ref(false)

const RENDERED_TYPES: readonly PluginSettingType[] = ['toggle', 'select', 'number', 'text', 'speech_packs']

/** Settings this host can render; unknown types from newer hosts are skipped. */
const renderedSettings = computed<PluginSettingSpec[]>(() =>
  (manifest.value?.settings ?? []).filter((s) => (RENDERED_TYPES as readonly string[]).includes(s.type)),
)
const hasValueSettings = computed(() => renderedSettings.value.some((s) => s.type !== 'speech_packs'))

const name = computed(() => plugin.value?.name ?? manifest.value?.name ?? t('plugins.docs.fallbackName'))
const isBuiltin = computed(() => plugin.value?.source === 'builtin')

async function load() {
  loading.value = true
  loadError.value = null
  try {
    const list = await invoke<InstalledPlugin[]>('plugin_list')
    allPlugins.value = list
    plugin.value = list.find((p) => p.plugin_cid === props.cid) ?? null
    if (!plugin.value) {
      loadError.value = t('plugins.settingsPage.notInstalled')
      return
    }
    const [m, perms, settings, manifests] = await Promise.all([
      invoke<PluginManifest>('plugin_get_manifest', { pluginCid: props.cid }),
      invoke<PluginPermissionRecord[]>('plugin_list_permissions', { pluginCid: props.cid }).catch(
        () => [] as PluginPermissionRecord[],
      ),
      invoke<PluginSettingValues>('plugin_get_settings', { pluginCid: props.cid }),
      Promise.all(
        list.map(async (p) => [
          p.plugin_cid,
          await invoke<PluginManifest>('plugin_get_manifest', { pluginCid: p.plugin_cid }).catch(() => null),
        ] as const),
      ),
    ])
    manifest.value = m
    permissions.value = perms
    values.value = settings
    const byCid: Record<string, PluginManifest> = {}
    for (const [cid, pm] of manifests) if (pm) byCid[cid] = pm
    allManifests.value = byCid
    void ensureNames([m.author_did])
    if (m.icon_path) {
      thumbnail.value = await invoke<string>('plugin_read_asset_data_url', {
        pluginCid: props.cid,
        path: m.icon_path,
      }).catch(() => '')
    }
  } catch (e) {
    loadError.value = t('plugins.settingsPage.loadFailed', { error: String(e) })
  } finally {
    loading.value = false
  }
  await nextTick()
  revealTargetSetting()
}

onMounted(load)

// ---- Search target: ?setting=<key> --------------------------------------
const targetSetting = computed(() => (typeof route.query.setting === 'string' ? route.query.setting : null))

function revealTargetSetting() {
  const key = targetSetting.value
  if (!key) return
  const row = document.getElementById(`setting-${key}`)
  if (!row) return
  row.scrollIntoView({ block: 'center', behavior: motionReduced.value ? 'auto' : 'smooth' })
  row.querySelector<HTMLElement>('input, select, button')?.focus({ preventScroll: true })
  animateMotion(
    row,
    [
      { backgroundColor: 'color-mix(in srgb, var(--app-primary) 18%, transparent)' },
      { backgroundColor: 'transparent' },
    ],
    'reveal',
  )
}

watch(targetSetting, async () => {
  await nextTick()
  revealTargetSetting()
})

// ---- Settings form ------------------------------------------------------
function announce(message: string) {
  status.value = ''
  void nextTick(() => {
    status.value = message
  })
}

async function save(spec: PluginSettingSpec, value: unknown) {
  const previous = values.value[spec.key]
  values.value = { ...values.value, [spec.key]: value }
  actionError.value = null
  try {
    await invoke('plugin_set_setting', { pluginCid: props.cid, key: spec.key, value })
    announce(t('plugins.settingsPage.saved'))
  } catch (e) {
    values.value = { ...values.value, [spec.key]: previous }
    actionError.value = t('plugins.settingsPage.saveFailed', { error: String(e) })
  }
}

async function resetDefaults() {
  actionError.value = null
  try {
    values.value = await invoke<PluginSettingValues>('plugin_reset_settings', { pluginCid: props.cid })
    announce(t('plugins.settingsPage.saved'))
  } catch (e) {
    actionError.value = t('plugins.settingsPage.resetFailed', { error: String(e) })
  }
}

function boolValue(key: string): boolean {
  return values.value[key] === true
}
function stringValue(key: string): string {
  const v = values.value[key]
  return typeof v === 'string' ? v : ''
}
function numberValue(key: string): number {
  const draft = rangeDraft.value[key]
  if (draft !== undefined) return draft
  const v = values.value[key]
  return typeof v === 'number' ? v : 0
}

// A slider shows its value while dragging and saves once on release.
const rangeDraft = ref<Record<string, number>>({})
function onRangeInput(key: string, raw: string) {
  const n = Number(raw)
  if (Number.isFinite(n)) rangeDraft.value = { ...rangeDraft.value, [key]: n }
}

function onNumberInput(spec: PluginSettingSpec, raw: string) {
  const rest = { ...rangeDraft.value }
  delete rest[spec.key]
  rangeDraft.value = rest
  const n = Number(raw)
  if (raw.trim() === '' || !Number.isFinite(n)) return
  if (spec.min !== null && n < spec.min) return
  if (spec.max !== null && n > spec.max) return
  void save(spec, n)
}

function onTextChange(spec: PluginSettingSpec, raw: string) {
  if (raw === stringValue(spec.key)) return
  void save(spec, raw)
}

function textMaxLength(spec: PluginSettingSpec): number {
  return spec.max_length ?? 200
}

/** Show a number as the author's step implies (0.9, not 0.9000000001). */
function formatNumber(spec: PluginSettingSpec, n: number): string {
  const step = spec.step ?? 1
  const decimals = Math.min(6, (String(step).split('.')[1] ?? '').length)
  return n.toFixed(decimals)
}

// ---- Header actions -----------------------------------------------------
async function setEnabled(next: boolean) {
  const p = plugin.value
  if (!p) return
  actionError.value = null
  try {
    await invoke('plugin_set_enabled', { pluginCid: p.plugin_cid, enabled: next })
    plugin.value = { ...p, enabled: next }
    emit('changed')
  } catch (e) {
    actionError.value = t('settings.plugins.toggleFailed', { msg: String(e) })
  }
}

function openDonate() {
  const url = manifest.value?.donate_url
  if (url) window.open(url, '_blank', 'noopener,noreferrer')
}

async function doUninstall() {
  confirmingUninstall.value = false
  actionError.value = null
  try {
    await invoke('plugin_uninstall', { pluginCid: props.cid })
    emit('changed')
    void router.push('/settings/plugins')
  } catch (e) {
    actionError.value = t('settings.plugins.uninstallFailed', { msg: String(e) })
  }
}

// ---- Permissions --------------------------------------------------------
const CAPABILITY_LABELS: Record<PluginCapability, string> = {
  microphone: 'plugins.settingsPage.capability.microphone',
  camera: 'plugins.settingsPage.capability.camera',
  midi: 'plugins.settingsPage.capability.midi',
  fullscreen: 'plugins.settingsPage.capability.fullscreen',
  clipboard: 'plugins.settingsPage.capability.clipboard',
  storage: 'plugins.settingsPage.capability.storage',
  ml_inference: 'plugins.settingsPage.capability.mlInference',
  instructor_review: 'plugins.settingsPage.capability.instructorReview',
}
const SCOPE_LABELS: Record<string, string> = {
  always: 'plugins.settingsPage.scope.always',
  session: 'plugins.settingsPage.scope.session',
  once: 'plugins.settingsPage.scope.once',
}

function capabilityLabel(cap: PluginCapability): string {
  const key = CAPABILITY_LABELS[cap]
  return key ? t(key) : cap
}

/** Declared capabilities plus any granted one the manifest no longer lists. */
const capabilityRows = computed(() => {
  const declared = manifest.value?.capabilities ?? []
  const caps: PluginCapability[] = [...declared]
  for (const p of permissions.value) if (!caps.includes(p.capability)) caps.push(p.capability)
  return caps.map((cap) => {
    const grant = permissions.value.find((p) => p.capability === cap) ?? null
    return { cap, grant }
  })
})

function scopeLabel(scope: string): string {
  const key = SCOPE_LABELS[scope]
  return key ? t(key) : scope
}

async function revoke(cap: PluginCapability) {
  actionError.value = null
  try {
    await invoke('plugin_revoke_capability', { pluginCid: props.cid, capability: cap })
    permissions.value = await invoke<PluginPermissionRecord[]>('plugin_list_permissions', {
      pluginCid: props.cid,
    })
  } catch (e) {
    actionError.value = t('settings.plugins.revokeFailed', { msg: String(e) })
  }
}

// ---- Dependencies -------------------------------------------------------
interface PluginRef {
  name: string
  cid: string | null
}

function refForPluginId(id: string): PluginRef {
  for (const p of allPlugins.value) {
    if (allManifests.value[p.plugin_cid]?.id === id) return { name: p.name, cid: p.plugin_cid }
  }
  const hash = id.indexOf('#')
  return { name: hash >= 0 ? id.slice(hash + 1) : id, cid: null }
}

const requires = computed<PluginRef[]>(() => (manifest.value?.dependencies ?? []).map(refForPluginId))
const requiredBy = computed<PluginRef[]>(() => {
  const myId = manifest.value?.id
  if (!myId) return []
  return allPlugins.value
    .filter((p) => (allManifests.value[p.plugin_cid]?.dependencies ?? []).includes(myId))
    .map((p) => ({ name: p.name, cid: p.plugin_cid }))
})
</script>

<template>
  <div class="space-y-7">
    <div v-if="loading" class="flex justify-center p-10">
      <AppSpinner />
    </div>

    <AppAlert v-else-if="loadError" variant="error">{{ loadError }}</AppAlert>

    <template v-else-if="plugin && manifest">
      <!-- Header -->
      <header class="flex flex-col gap-4 sm:flex-row sm:items-start">
        <div
          class="flex h-16 w-16 shrink-0 items-center justify-center overflow-hidden rounded-2xl text-2xl font-bold text-white shadow-sm"
          :style="thumbnail ? undefined : { background: pluginThumbGradient(cid) }"
        >
          <img v-if="thumbnail" :src="thumbnail" alt="" class="h-full w-full object-cover" />
          <span v-else aria-hidden="true">{{ pluginMonogram(name) }}</span>
        </div>
        <div class="min-w-0 flex-1">
          <h4 class="text-xl font-bold text-foreground">{{ name }}</h4>
          <div class="mt-1.5 flex flex-wrap items-center gap-2">
            <AppBadge variant="secondary">v{{ plugin.version }}</AppBadge>
            <AppBadge variant="secondary">{{ plugin.source }}</AppBadge>
            <AppBadge v-if="!plugin.enabled" variant="warning">{{ $t('settings.plugins.disabled') }}</AppBadge>
            <span class="text-xs text-muted-foreground">
              {{ $t('plugins.docs.by', { name: displayName(manifest.author_did) }) }}
            </span>
          </div>
          <p v-if="manifest.description" class="mt-2 text-sm text-muted-foreground">{{ manifest.description }}</p>

          <div class="mt-4 flex flex-wrap items-center gap-x-4 gap-y-2">
            <span class="flex items-center gap-2 text-sm text-foreground">
              <AppSwitch
                :model-value="plugin.enabled"
                :label="$t('settings.plugins.enabled')"
                @update:model-value="setEnabled"
              />
              <span aria-hidden="true">{{ $t('settings.plugins.enabled') }}</span>
            </span>
            <router-link
              :to="`/settings/plugins/${cid}/docs`"
              class="inline-flex min-h-9 items-center text-sm font-medium text-primary hover:underline"
            >
              {{ $t('plugins.settingsPage.openDocs') }}
            </router-link>
            <AppButton v-if="manifest.donate_url" size="sm" variant="outline" @click="openDonate">
              {{ $t('plugins.docs.donate') }}
            </AppButton>
            <AppButton v-if="!isBuiltin" size="sm" variant="danger" @click="confirmingUninstall = true">
              {{ $t('settings.plugins.uninstall') }}
            </AppButton>
            <span v-else class="text-xs text-muted-foreground">{{ $t('plugins.settingsPage.builtinProtected') }}</span>
          </div>
        </div>
      </header>

      <AppAlert v-if="actionError" variant="error">{{ actionError }}</AppAlert>
      <p class="sr-only" role="status" aria-live="polite">{{ status }}</p>

      <!-- Plugin settings -->
      <section aria-labelledby="plugin-settings-heading">
        <div class="mb-3 flex items-center justify-between gap-2">
          <h4 id="plugin-settings-heading" class="plugin-group-title">{{ $t('plugins.settingsPage.settingsHeading') }}</h4>
          <AppButton v-if="hasValueSettings" variant="ghost" size="sm" @click="resetDefaults">
            {{ $t('plugins.settingsPage.resetDefaults') }}
          </AppButton>
        </div>
        <p v-if="renderedSettings.length === 0" class="text-sm text-muted-foreground">
          {{ $t('plugins.settingsPage.noSettings') }}
        </p>
        <template v-else>
          <p class="mb-3 text-xs text-muted-foreground">{{ $t('plugins.settingsPage.authorNote') }}</p>
          <div class="divide-y divide-border/50 rounded-lg border border-border">
            <div
              v-for="spec in renderedSettings"
              :id="`setting-${spec.key}`"
              :key="spec.key"
              class="plugin-setting px-4 py-3"
              :class="{ 'plugin-setting--target': targetSetting === spec.key }"
            >
              <!-- Toggle -->
              <div v-if="spec.type === 'toggle'" class="flex items-center justify-between gap-4">
                <div class="min-w-0">
                  <p :id="`setting-${spec.key}-label`" class="text-sm font-medium text-foreground">{{ spec.label }}</p>
                  <p v-if="spec.description" class="mt-0.5 text-xs text-muted-foreground">{{ spec.description }}</p>
                </div>
                <AppSwitch
                  :model-value="boolValue(spec.key)"
                  :label="spec.label"
                  @update:model-value="(v: boolean) => save(spec, v)"
                />
              </div>

              <!-- Select -->
              <div v-else-if="spec.type === 'select'" class="flex flex-col gap-2 sm:flex-row sm:items-center sm:justify-between sm:gap-4">
                <div class="min-w-0">
                  <label :for="`setting-${spec.key}-input`" class="text-sm font-medium text-foreground">{{ spec.label }}</label>
                  <p v-if="spec.description" class="mt-0.5 text-xs text-muted-foreground">{{ spec.description }}</p>
                </div>
                <select
                  :id="`setting-${spec.key}-input`"
                  :value="stringValue(spec.key)"
                  class="input sm:w-56"
                  @change="save(spec, ($event.target as HTMLSelectElement).value)"
                >
                  <option v-for="opt in spec.options" :key="opt.value" :value="opt.value">{{ opt.label }}</option>
                </select>
              </div>

              <!-- Number -->
              <div v-else-if="spec.type === 'number'" class="flex flex-col gap-2">
                <div class="flex items-baseline justify-between gap-4">
                  <label :for="`setting-${spec.key}-input`" class="text-sm font-medium text-foreground">{{ spec.label }}</label>
                  <span v-if="spec.min !== null && spec.max !== null" class="font-mono text-sm text-foreground" aria-hidden="true">
                    {{ formatNumber(spec, numberValue(spec.key)) }}
                  </span>
                </div>
                <p v-if="spec.description" class="text-xs text-muted-foreground">{{ spec.description }}</p>
                <input
                  v-if="spec.min !== null && spec.max !== null"
                  :id="`setting-${spec.key}-input`"
                  type="range"
                  :min="spec.min"
                  :max="spec.max"
                  :step="spec.step ?? 'any'"
                  :value="numberValue(spec.key)"
                  :aria-valuetext="formatNumber(spec, numberValue(spec.key))"
                  class="w-full accent-primary"
                  @input="onRangeInput(spec.key, ($event.target as HTMLInputElement).value)"
                  @change="onNumberInput(spec, ($event.target as HTMLInputElement).value)"
                >
                <input
                  v-else
                  :id="`setting-${spec.key}-input`"
                  type="number"
                  :min="spec.min ?? undefined"
                  :max="spec.max ?? undefined"
                  :step="spec.step ?? 'any'"
                  :value="numberValue(spec.key)"
                  class="input sm:w-40"
                  @change="onNumberInput(spec, ($event.target as HTMLInputElement).value)"
                >
              </div>

              <!-- Text -->
              <div v-else-if="spec.type === 'text'" class="flex flex-col gap-2">
                <label :for="`setting-${spec.key}-input`" class="text-sm font-medium text-foreground">{{ spec.label }}</label>
                <p v-if="spec.description" class="text-xs text-muted-foreground">{{ spec.description }}</p>
                <input
                  :id="`setting-${spec.key}-input`"
                  type="text"
                  :value="stringValue(spec.key)"
                  :maxlength="textMaxLength(spec)"
                  class="input"
                  @change="onTextChange(spec, ($event.target as HTMLInputElement).value)"
                >
              </div>

              <!-- Host widget: pronunciation language packs -->
              <div v-else-if="spec.type === 'speech_packs'" class="flex flex-col gap-2">
                <p class="text-sm font-medium text-foreground">{{ spec.label }}</p>
                <p v-if="spec.description" class="text-xs text-muted-foreground">{{ spec.description }}</p>
                <SpeechPacksSetting />
              </div>
            </div>
          </div>
        </template>
      </section>

      <!-- Permissions -->
      <section aria-labelledby="plugin-permissions-heading">
        <h4 id="plugin-permissions-heading" class="plugin-group-title mb-3">{{ $t('plugins.settingsPage.permissionsHeading') }}</h4>
        <p v-if="capabilityRows.length === 0" class="text-sm text-muted-foreground">
          {{ $t('plugins.settingsPage.permissionsEmpty') }}
        </p>
        <template v-else>
          <p class="mb-3 text-xs text-muted-foreground">{{ $t('plugins.settingsPage.permissionsIntro') }}</p>
          <ul class="divide-y divide-border/50 rounded-lg border border-border">
            <li v-for="row in capabilityRows" :key="row.cap" class="flex items-center justify-between gap-4 px-4 py-3">
              <div class="min-w-0">
                <p class="text-sm font-medium text-foreground">{{ capabilityLabel(row.cap) }}</p>
                <p class="mt-0.5 text-xs" :class="row.grant ? 'text-success' : 'text-muted-foreground'">
                  {{ row.grant ? scopeLabel(row.grant.scope) : $t('plugins.settingsPage.notGranted') }}
                </p>
              </div>
              <AppButton v-if="row.grant" size="sm" variant="outline" @click="revoke(row.cap)">
                {{ $t('plugins.installed.detail.revoke') }}
              </AppButton>
            </li>
          </ul>
        </template>
      </section>

      <!-- Dependencies -->
      <section aria-labelledby="plugin-deps-heading">
        <h4 id="plugin-deps-heading" class="plugin-group-title mb-3">{{ $t('plugins.settingsPage.dependenciesHeading') }}</h4>
        <p v-if="requires.length === 0 && requiredBy.length === 0" class="text-sm text-muted-foreground">
          {{ $t('plugins.settingsPage.dependenciesEmpty') }}
        </p>
        <div v-else class="space-y-3 text-sm">
          <div v-if="requires.length" class="flex flex-wrap items-center gap-1.5">
            <span class="text-muted-foreground">{{ $t('settings.plugins.requires') }}</span>
            <template v-for="dep in requires" :key="`req-${dep.name}`">
              <router-link v-if="dep.cid" :to="`/settings/plugins/${dep.cid}`" class="plugin-dep plugin-dep--link">{{ dep.name }}</router-link>
              <span v-else class="plugin-dep">{{ dep.name }}</span>
            </template>
          </div>
          <div v-if="requiredBy.length">
            <div class="flex flex-wrap items-center gap-1.5">
              <span class="text-muted-foreground">{{ $t('settings.plugins.requiredBy') }}</span>
              <router-link
                v-for="dep in requiredBy"
                :key="`reqby-${dep.cid}`"
                :to="`/settings/plugins/${dep.cid}`"
                class="plugin-dep plugin-dep--link"
              >{{ dep.name }}</router-link>
            </div>
            <p v-if="!isBuiltin" class="mt-1 text-xs text-muted-foreground">{{ $t('plugins.settingsPage.requiredByHint') }}</p>
          </div>
        </div>
      </section>
    </template>

    <AppModal
      v-if="confirmingUninstall && plugin"
      :open="confirmingUninstall"
      :title="$t('settings.plugins.uninstallTitle')"
      @close="confirmingUninstall = false"
    >
      <p class="text-sm">
        {{ $t('settings.plugins.uninstallBody', { name: plugin.name, version: plugin.version }) }}
      </p>
      <template #footer>
        <AppButton variant="ghost" @click="confirmingUninstall = false">{{ $t('common.actions.cancel') }}</AppButton>
        <AppButton variant="danger" @click="doUninstall">{{ $t('settings.plugins.uninstall') }}</AppButton>
      </template>
    </AppModal>
  </div>
</template>

<style scoped>
.plugin-group-title {
  font-size: 0.75rem;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.05em;
  color: var(--app-muted-foreground);
}
.plugin-setting { scroll-margin-block: 4rem; }
.plugin-setting--target { box-shadow: inset 3px 0 0 var(--app-primary); }
.plugin-setting--target:dir(rtl) { box-shadow: inset -3px 0 0 var(--app-primary); }
.plugin-dep {
  border-radius: 0.375rem;
  background: color-mix(in srgb, var(--app-muted) 40%, transparent);
  padding: 0.125rem 0.5rem;
  font-size: 0.75rem;
  color: var(--app-foreground);
}
.plugin-dep--link { color: var(--app-primary); }
.plugin-dep--link:hover { text-decoration: underline; }
</style>
