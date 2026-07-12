<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
import { useI18n } from 'vue-i18n'
import { useRouter, useRoute } from 'vue-router'
import { useLocalApi } from '@/composables/useLocalApi'
import { useAuth } from '@/composables/useAuth'
import { useProfiles } from '@/composables/useProfiles'
import { useP2P } from '@/composables/useP2P'
import { useTheme } from '@/composables/useTheme'
import {
  useKeyboardShortcuts,
  formatCombo,
  comboFromEvent,
} from '@/composables/useKeyboardShortcuts'
import {
  biometricCredentialExists,
  biometricSupported,
  clearBiometricVaultPassword,
  getBiometricStatus,
  storeVaultPasswordForBiometric,
} from '@/composables/useBiometricVault'
import type { SettingsSectionId } from '@/composables/useSettingsModal'
import { AppButton, AppInput, AppTextarea, AppModal, AppAlert } from '@/components/ui'
import AdvancedSettingsPanel from '@/components/settings/AdvancedSettingsPanel.vue'
import RelayManager from '@/components/settings/RelayManager.vue'
import PluginsPanel from '@/components/settings/PluginsPanel.vue'
import GuardianPanel from '@/components/settings/GuardianPanel.vue'
import LanguageSelector from '@/components/settings/LanguageSelector.vue'
import type { Identity } from '@/types'

const { t } = useI18n()
const { invoke } = useLocalApi()
const { identity, lockVault: authLock, exportMnemonic: authExport, refreshProfile } = useAuth()
const { activeProfileId } = useProfiles()
const { status: p2pStatus, refreshStatus: refreshP2pStatus } = useP2P()
const { theme, setTheme } = useTheme()
const { shortcuts, updateShortcut, resetShortcut, resetAll: resetAllShortcuts } =
  useKeyboardShortcuts()
const router = useRouter()
const route = useRoute()

// ---- Section nav metadata + search index ----
interface SectionMeta {
  id: SettingsSectionId
  label: string
  desc: string
  /** Free-text terms the search box matches against, beyond label/desc. */
  keywords: string[]
}
const SECTION_IDS: SettingsSectionId[] = [
  'account', 'security', 'personalization', 'system', 'plugins', 'guardian', 'advanced',
]
const SECTIONS = computed<SectionMeta[]>(() => [
  { id: 'account', label: t('settings.nav.sections.account.label'), desc: t('settings.nav.sections.account.desc'),
    keywords: ['display name', 'bio', 'stake address', 'payment address', 'peer id', 'profile hash', 'publish', 'did'] },
  { id: 'security', label: t('settings.nav.sections.security.label'), desc: t('settings.nav.sections.security.desc'),
    keywords: ['recovery phrase', 'mnemonic', 'export', 'seed', 'lock wallet', 'vault', 'biometric', 'touch id', 'face id', 'password'] },
  { id: 'personalization', label: t('settings.nav.sections.personalization.label'), desc: t('settings.nav.sections.personalization.desc'),
    keywords: ['language', 'locale', 'translation', 'idioma', 'langue', 'भाषा', 'theme', 'light', 'dark', 'system', 'appearance', 'keyboard shortcuts', 'keybindings', 'hotkeys'] },
  { id: 'system', label: t('settings.nav.sections.system.label'), desc: t('settings.nav.sections.system.desc'),
    keywords: ['storage', 'disk', 'quota', 'cache', 'free space', 'evict', 'network', 'p2p', 'node', 'peers'] },
  { id: 'plugins', label: t('settings.nav.sections.plugins.label'), desc: t('settings.nav.sections.plugins.desc'),
    keywords: ['plugin', 'install', 'uninstall', 'enable', 'disable', 'capability', 'donate', 'instructor', 'review', 'irl', 'music'] },
  { id: 'guardian', label: t('settings.nav.sections.guardian.label'), desc: t('settings.nav.sections.guardian.desc'),
    keywords: ['guardian', 'parent', 'oversight', 'minor', 'ward', 'family', 'link', 'unlink'] },
  { id: 'advanced', label: t('settings.nav.sections.advanced.label'), desc: t('settings.nav.sections.advanced.desc'),
    keywords: ['advanced', 'all settings', 'sync', 'sentinel', 'notifications', 'flags'] },
])

const THEME_LABELS: Record<'light' | 'dark' | 'system', string> = {
  light: 'settings.personalization.themeLight',
  dark: 'settings.personalization.themeDark',
  system: 'settings.personalization.themeSystem',
}
function themeLabel(opt: 'light' | 'dark' | 'system'): string {
  return t(THEME_LABELS[opt])
}

// ---- Search ----
const searchQuery = ref('')
const filteredSections = computed<SectionMeta[]>(() => {
  const q = searchQuery.value.trim().toLowerCase()
  if (!q) return SECTIONS.value
  return SECTIONS.value.filter((s) => {
    const hay = [s.label, s.desc, ...s.keywords].join(' ').toLowerCase()
    return q.split(/\s+/).every((term) => hay.includes(term))
  })
})

/** Per-section match terms shown as chips under a section in search mode,
 *  so the user sees WHY a section matched (e.g. searching "touch id" under
 *  Security). */
function matchedKeywords(s: SectionMeta): string[] {
  const q = searchQuery.value.trim().toLowerCase()
  if (!q) return []
  const terms = q.split(/\s+/)
  return s.keywords.filter((k) => terms.some((t) => k.toLowerCase().includes(t))).slice(0, 4)
}

function onSearchEnter() {
  const first = filteredSections.value[0]
  if (first) setSection(first.id)
}

// Active section is driven by the route param so settings is deep-linkable
// (e.g. /settings/security) and the browser back button works.
const activeSection = computed<SettingsSectionId>(() => {
  const s = route.params.section as string | undefined
  return (s && (SECTION_IDS as string[]).includes(s))
    ? (s as SettingsSectionId)
    : 'account'
})

function setSection(id: SettingsSectionId) {
  if (id === activeSection.value) return
  void router.push(`/settings/${id}`)
}

// ---- Profile ----
const displayName = ref('')
const bio = ref('')
const profileVisibility = ref<'public' | 'private'>('public')

// Username rename (conflict recovery or by choice).
const editingUsername = ref(false)
const newUsername = ref('')
const usernameMessage = ref('')
const usernameSaving = ref(false)
async function saveUsername() {
  usernameSaving.value = true
  usernameMessage.value = ''
  try {
    await invoke('set_username', { username: newUsername.value.trim() })
    await refreshProfile()
    editingUsername.value = false
    usernameMessage.value = t('settings.profile.usernameUpdated')
  } catch (e) {
    usernameMessage.value = `${e}`
  } finally {
    usernameSaving.value = false
  }
}
const saving = ref(false)
const message = ref('')

const publishing = ref(false)
const publishMessage = ref('')

// ---- Security ----
const showExportModal = ref(false)
const exportConfirmed = ref(false)
const exportedMnemonic = ref('')
const exportPassword = ref('')
const exportError = ref('')
const exporting = ref(false)
const locking = ref(false)
const biometricAvailable = ref(false)
const biometricEnabled = ref(false)
const biometricBusy = ref(false)
const biometricPassword = ref('')
const biometricMessage = ref('')
const biometricDiagnostics = ref('')

// ---- Storage ----
interface StorageStats {
  total_pinned_bytes: number
  quota_bytes: number
  evictable_bytes: number
  pin_count: number
  usage_percent: number | null
}

const QUOTA_OPTIONS = [
  { label: '1 GB', bytes: 1_073_741_824 },
  { label: '2 GB', bytes: 2_147_483_648 },
  { label: '5 GB', bytes: 5_368_709_120 },
  { label: '10 GB', bytes: 10_737_418_240 },
  { label: '25 GB', bytes: 26_843_545_600 },
  { label: 'Unlimited', bytes: 0 },
]

const storageStats = ref<StorageStats | null>(null)
const quotaBytes = ref(0)
const evicting = ref(false)
const storageMessage = ref('')

function formatBytes(bytes: number): string {
  if (bytes === 0) return '0 B'
  const units = ['B', 'KB', 'MB', 'GB', 'TB']
  const i = Math.floor(Math.log(bytes) / Math.log(1024))
  return `${(bytes / Math.pow(1024, i)).toFixed(i > 0 ? 1 : 0)} ${units[i]}`
}

function usageColor(percent: number | null): string {
  if (percent === null) return 'bg-primary'
  if (percent < 70) return 'bg-emerald-500'
  if (percent < 90) return 'bg-amber-500'
  return 'bg-red-500'
}

async function loadStorageStats() {
  try {
    storageStats.value = await invoke<StorageStats>('storage_stats')
    quotaBytes.value = await invoke<number>('storage_get_quota')
  } catch (e) {
    console.error('Failed to load storage stats:', e)
  }
}

async function setQuota(bytes: number) {
  try {
    await invoke('storage_set_quota', { bytes })
    quotaBytes.value = bytes
    storageMessage.value = ''
    await loadStorageStats()
  } catch (e) {
    storageMessage.value = t('settings.storage.error', { msg: String(e) })
  }
}

async function freeSpace() {
  evicting.value = true
  storageMessage.value = ''
  try {
    const result = await invoke<{ blobs_evicted: number; bytes_freed: number }>('storage_evict_now')
    if (result.blobs_evicted > 0) {
      storageMessage.value = t('settings.storage.freed', {
        size: formatBytes(result.bytes_freed),
        items: t('settings.storage.freedItems', { count: result.blobs_evicted }, result.blobs_evicted),
      })
    } else {
      storageMessage.value = t('settings.storage.nothingToFree')
    }
    await loadStorageStats()
  } catch (e) {
    storageMessage.value = t('settings.storage.error', { msg: String(e) })
  } finally {
    evicting.value = false
  }
}

// ---- Keyboard shortcut recording ----
const recordingShortcutId = ref<string | null>(null)

function startRecording(id: string) {
  recordingShortcutId.value = id
}

function cancelRecording() {
  recordingShortcutId.value = null
}

function onShortcutKeydown(e: KeyboardEvent) {
  e.preventDefault()
  e.stopPropagation()
  const combo = comboFromEvent(e)
  if (!combo || !recordingShortcutId.value) return
  updateShortcut(recordingShortcutId.value, combo)
  recordingShortcutId.value = null
}

// ---- Lifecycle: refresh when opened ----
async function hydrate() {
  if (identity.value) {
    displayName.value = identity.value.display_name ?? ''
    bio.value = identity.value.bio ?? ''
    profileVisibility.value = identity.value.visibility === 'private' ? 'private' : 'public'
  }
  void refreshBiometricState()
  void loadStorageStats()
  void refreshP2pStatus()
}

onMounted(() => {
  void hydrate()
})

// ---- Section actions ----
async function refreshBiometricState() {
  try {
    const [status, enabled] = await Promise.all([
      getBiometricStatus(),
      activeProfileId.value ? biometricCredentialExists(activeProfileId.value) : Promise.resolve(false),
    ])
    biometricAvailable.value = status.isAvailable
    biometricEnabled.value = enabled
    biometricDiagnostics.value = status.error
      ? (status.errorCode
          ? t('settings.security.statusErrorCode', { code: status.errorCode, msg: status.error })
          : t('settings.security.statusError', { msg: status.error }))
      : ''
  } catch {
    biometricAvailable.value = false
    biometricEnabled.value = false
    biometricDiagnostics.value = ''
  }
}

async function saveProfile() {
  if (!displayName.value.trim()) {
    message.value = t('settings.profile.displayNameRequired')
    return
  }
  saving.value = true
  message.value = ''
  try {
    await invoke<Identity>('update_profile', {
      update: {
        display_name: displayName.value.trim(),
        bio: bio.value || null,
        visibility: profileVisibility.value,
      },
    })
    await refreshProfile()
    message.value = t('settings.profile.updated')
  } catch (e) {
    message.value = t('settings.profile.error', { msg: String(e) })
  } finally {
    saving.value = false
  }
}

async function publishProfile() {
  publishing.value = true
  publishMessage.value = ''
  try {
    await invoke('publish_profile')
    publishMessage.value = t('settings.profile.published')
    await refreshProfile()
  } catch (e) {
    publishMessage.value = t('settings.profile.error', { msg: String(e) })
  } finally {
    publishing.value = false
  }
}

function openExportModal() {
  showExportModal.value = true
  exportConfirmed.value = false
  exportedMnemonic.value = ''
  exportPassword.value = ''
  exportError.value = ''
}

function closeExportModal() {
  showExportModal.value = false
  exportedMnemonic.value = ''
  exportPassword.value = ''
  exportConfirmed.value = false
}

function confirmExport() {
  exportConfirmed.value = true
  exportError.value = ''
}

async function doExport() {
  exporting.value = true
  exportError.value = ''
  try {
    exportedMnemonic.value = await authExport(exportPassword.value)
  } catch (e) {
    exportError.value = String(e)
  } finally {
    exporting.value = false
    exportPassword.value = ''
  }
}

async function lockWallet() {
  locking.value = true
  try {
    await authLock()
    router.replace('/unlock')
  } catch (e) {
    console.error('Failed to lock:', e)
  } finally {
    locking.value = false
  }
}

async function enableBiometric() {
  if (!biometricPassword.value) {
    biometricMessage.value = t('settings.security.enterPasswordToEnable')
    return
  }
  biometricBusy.value = true
  biometricMessage.value = ''
  try {
    const supported = await biometricSupported()
    if (!supported) {
      biometricMessage.value = t('settings.security.biometricUnavailableRuntime')
      return
    }
    if (!activeProfileId.value) {
      biometricMessage.value = t('settings.security.noActiveProfile')
      return
    }
    const mode = await storeVaultPasswordForBiometric(activeProfileId.value, biometricPassword.value)
    biometricEnabled.value = true
    biometricPassword.value = ''
    biometricMessage.value = mode === 'secure'
      ? t('settings.security.biometricEnabledMsg')
      : t('settings.security.biometricSessionOnly')
  } catch (e) {
    const msg = String(e)
    if (msg.includes('-34018')) {
      biometricMessage.value = t('settings.security.keychainEntitlement')
    } else {
      biometricMessage.value = t('settings.security.biometricEnableFailed', { msg })
    }
  } finally {
    biometricBusy.value = false
    await refreshBiometricState()
  }
}

async function disableBiometric() {
  biometricBusy.value = true
  biometricMessage.value = ''
  try {
    if (activeProfileId.value) {
      await clearBiometricVaultPassword(activeProfileId.value)
    }
    biometricEnabled.value = false
    biometricMessage.value = t('settings.security.biometricCleared')
  } catch (e) {
    biometricMessage.value = t('settings.security.biometricClearFailed', { msg: String(e) })
  } finally {
    biometricBusy.value = false
    await refreshBiometricState()
  }
}

function gotoNetwork() {
  router.push('/network')
}

async function copyText(value: string | undefined | null) {
  if (!value) return
  try {
    await navigator.clipboard.writeText(value)
  } catch {
    // clipboard may be unavailable — silently ignore
  }
}

function onSectionClick(id: SettingsSectionId) {
  setSection(id)
}
</script>

<template>
  <div class="settings-page flex w-full flex-1 min-h-0 flex-col sm:flex-row gap-0 overflow-hidden bg-background">
            <!-- Sidebar nav -->
            <aside class="settings-sidebar shrink-0 sm:w-64 border-b sm:border-b-0 sm:border-e border-border bg-muted/20 flex flex-col">
              <div class="px-4 pt-5 pb-3">
                <h2 class="text-sm font-semibold tracking-wide uppercase text-muted-foreground mb-3">
                  {{ $t('settings.nav.heading') }}
                </h2>
                <!-- Search -->
                <div class="relative">
                  <svg class="pointer-events-none absolute start-2.5 top-1/2 h-4 w-4 -translate-y-1/2 text-muted-foreground" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                    <path stroke-linecap="round" stroke-linejoin="round" d="M21 21l-4.35-4.35M17 11a6 6 0 11-12 0 6 6 0 0112 0z" />
                  </svg>
                  <input
                    v-model="searchQuery"
                    type="text"
                    :placeholder="$t('settings.nav.searchPlaceholder')"
                    class="w-full rounded-lg border border-border bg-background py-1.5 ps-8 pe-7 text-sm text-foreground outline-none focus:border-primary"
                    @keyup.enter="onSearchEnter"
                  >
                  <button
                    v-if="searchQuery"
                    class="absolute end-2 top-1/2 -translate-y-1/2 text-muted-foreground hover:text-foreground"
                    :aria-label="$t('settings.nav.clearSearch')"
                    @click="searchQuery = ''"
                  >
                    <svg class="h-3.5 w-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                      <path stroke-linecap="round" stroke-linejoin="round" d="M6 18L18 6M6 6l12 12" />
                    </svg>
                  </button>
                </div>
              </div>

              <nav class="flex-1 overflow-y-auto px-2 pb-4 flex sm:block overflow-x-auto sm:overflow-visible gap-1 sm:gap-0">
                <p v-if="filteredSections.length === 0" class="px-3 py-4 text-xs text-muted-foreground">
                  {{ $t('settings.nav.noMatch', { query: searchQuery }) }}
                </p>
                <button
                  v-for="s in filteredSections"
                  :key="s.id"
                  class="settings-nav-item"
                  :class="{ 'settings-nav-item--active': activeSection === s.id }"
                  @click="onSectionClick(s.id)"
                >
                  <span class="settings-nav-icon" aria-hidden="true">
                    <!-- Account -->
                    <svg v-if="s.id === 'account'" class="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.75">
                      <path stroke-linecap="round" stroke-linejoin="round" d="M16 7a4 4 0 11-8 0 4 4 0 018 0zM12 14c-4.418 0-8 2.239-8 5v1h16v-1c0-2.761-3.582-5-8-5z" />
                    </svg>
                    <!-- Security -->
                    <svg v-else-if="s.id === 'security'" class="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.75">
                      <path stroke-linecap="round" stroke-linejoin="round" d="M12 11c-1.105 0-2 .895-2 2 0 .738.4 1.38 1 1.723V17h2v-2.277c.6-.343 1-.985 1-1.723 0-1.105-.895-2-2-2zM6 10V8a6 6 0 1112 0v2M5 10h14v10H5V10z" />
                    </svg>
                    <!-- Personalization -->
                    <svg v-else-if="s.id === 'personalization'" class="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.75">
                      <path stroke-linecap="round" stroke-linejoin="round" d="M7 21a4 4 0 01-4-4 4 4 0 014-4h1m11-7l-7 7m0 0l-3-3m3 3v6a4 4 0 11-4-4" />
                    </svg>
                    <!-- Plugins -->
                    <svg v-else-if="s.id === 'plugins'" class="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.75">
                      <path stroke-linecap="round" stroke-linejoin="round" d="M14.7 6.3a1 1 0 001.4 0l1.6-1.6a1 1 0 011.4 1.4l-1.6 1.6a1 1 0 000 1.4l3 3a1 1 0 010 1.4l-1.6 1.6a1 1 0 01-1.4 0l-3-3a1 1 0 00-1.4 0L9 17a4 4 0 11-5.6-5.6L9 6a1 1 0 011.4 0l4.3 4.3" />
                    </svg>
                    <!-- System -->
                    <svg v-else class="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.75">
                      <path stroke-linecap="round" stroke-linejoin="round" d="M4 7h16M4 12h16M4 17h16" />
                    </svg>
                  </span>
                  <span class="flex flex-col text-start min-w-0">
                    <span class="text-sm font-medium truncate">{{ s.label }}</span>
                    <span class="hidden sm:block text-[11px] text-muted-foreground truncate">{{ s.desc }}</span>
                    <span v-if="matchedKeywords(s).length" class="hidden sm:flex flex-wrap gap-1 mt-1">
                      <span
                        v-for="kw in matchedKeywords(s)"
                        :key="kw"
                        class="rounded bg-primary/10 px-1.5 py-0.5 text-[10px] text-primary"
                      >{{ kw }}</span>
                    </span>
                  </span>
                </button>
              </nav>
            </aside>

            <!-- Content panel -->
            <section class="settings-content flex-1 flex flex-col min-w-0 min-h-0">
              <header class="flex items-center justify-between px-6 py-4 border-b border-border">
                <h3 class="text-base font-semibold text-foreground">
                  {{ SECTIONS.find(s => s.id === activeSection)?.label }}
                </h3>
              </header>

              <div class="flex-1 overflow-y-auto px-6 py-5 space-y-8">
                <!-- ──────────── Account & Identity ──────────── -->
                <template v-if="activeSection === 'account'">
                  <div>
                    <h4 class="settings-group-title">{{ $t('settings.profile.title') }}</h4>
                    <div class="space-y-4">
                      <div>
                        <label class="label text-xs text-muted-foreground">{{ $t('settings.profile.usernameLabel') }}</label>
                        <div v-if="!editingUsername" class="flex items-center gap-2">
                          <span class="rounded-md border border-border bg-muted/40 px-3 py-2 text-sm text-foreground">
                            @{{ identity?.username ?? '—' }}
                          </span>
                          <AppButton variant="ghost" size="xs" @click="editingUsername = true; newUsername = identity?.username ?? ''">
                            {{ $t('settings.profile.change') }}
                          </AppButton>
                          <span class="text-xs text-muted-foreground">{{ $t('settings.profile.findYou') }}</span>
                        </div>
                        <div v-else class="flex items-center gap-2">
                          <AppInput v-model="newUsername" :placeholder="$t('settings.profile.newHandlePlaceholder')" />
                          <AppButton size="sm" :loading="usernameSaving" @click="saveUsername">{{ $t('common.actions.save') }}</AppButton>
                          <AppButton variant="ghost" size="sm" @click="editingUsername = false">{{ $t('common.actions.cancel') }}</AppButton>
                        </div>
                        <p v-if="usernameMessage" class="mt-1 text-xs text-muted-foreground">{{ usernameMessage }}</p>
                        <p v-if="editingUsername" class="mt-1 text-xs text-warning">
                          {{ $t('settings.profile.usernameWarning') }}
                        </p>
                      </div>
                      <AppInput
                        v-model="displayName"
                        :label="$t('settings.profile.displayNameLabel')"
                        :placeholder="$t('settings.profile.displayNamePlaceholder')"
                      />
                      <AppTextarea
                        v-model="bio"
                        :label="$t('settings.profile.bioLabel')"
                        :placeholder="$t('settings.profile.bioPlaceholder')"
                      />
                      <div>
                        <label class="label text-xs text-muted-foreground">{{ $t('settings.profile.visibilityLabel') }}</label>
                        <div class="flex gap-2">
                          <button
                            class="vis-option"
                            :class="{ 'vis-option--active': profileVisibility === 'public' }"
                            @click="profileVisibility = 'public'"
                          >
                            🌐 {{ $t('settings.profile.public') }}
                            <span class="vis-desc">{{ $t('settings.profile.publicDesc') }}</span>
                          </button>
                          <button
                            class="vis-option"
                            :class="{ 'vis-option--active': profileVisibility === 'private' }"
                            @click="profileVisibility = 'private'"
                          >
                            🔒 {{ $t('settings.profile.private') }}
                            <span class="vis-desc">{{ $t('settings.profile.privateDesc') }}</span>
                          </button>
                        </div>
                      </div>
                      <div class="flex flex-wrap items-center gap-3">
                        <AppButton :loading="saving" @click="saveProfile">
                          {{ $t('settings.profile.save') }}
                        </AppButton>
                        <AppButton variant="outline" :loading="publishing" @click="publishProfile">
                          {{ $t('settings.profile.publish') }}
                        </AppButton>
                        <span v-if="message" class="text-xs text-emerald-600 dark:text-emerald-400">{{ message }}</span>
                        <span v-if="publishMessage" class="text-xs text-emerald-600 dark:text-emerald-400">{{ publishMessage }}</span>
                      </div>
                    </div>
                  </div>

                  <div v-if="identity">
                    <h4 class="settings-group-title">{{ $t('settings.account.title') }}</h4>
                    <details>
                      <summary class="cursor-pointer text-xs text-muted-foreground">{{ $t('common.advanced.toggle') }}</summary>
                      <div class="space-y-3 mt-3">
                        <div class="settings-row-stack">
                          <div class="flex items-center justify-between">
                            <p class="text-xs text-muted-foreground">{{ $t('settings.account.stakeAddress') }}</p>
                            <button class="settings-copy-btn" @click="copyText(identity.stake_address)">{{ $t('common.actions.copy') }}</button>
                          </div>
                          <code class="settings-code">{{ identity.stake_address }}</code>
                        </div>

                        <div class="settings-row-stack">
                          <div class="flex items-center justify-between">
                            <p class="text-xs text-muted-foreground">{{ $t('settings.account.paymentAddress') }}</p>
                            <button class="settings-copy-btn" @click="copyText(identity.payment_address)">{{ $t('common.actions.copy') }}</button>
                          </div>
                          <code class="settings-code">{{ identity.payment_address }}</code>
                        </div>

                        <div class="settings-row-stack">
                          <div class="flex items-center justify-between">
                            <p class="text-xs text-muted-foreground">{{ $t('settings.account.deviceId') }}</p>
                            <button
                              v-if="p2pStatus?.peer_id"
                              class="settings-copy-btn"
                              @click="copyText(p2pStatus.peer_id)"
                            >
                              {{ $t('common.actions.copy') }}
                            </button>
                          </div>
                          <code v-if="p2pStatus?.peer_id" class="settings-code">{{ p2pStatus.peer_id }}</code>
                          <p v-else class="text-xs text-muted-foreground italic">{{ $t('settings.account.offline') }}</p>
                        </div>

                        <div v-if="identity.profile_hash" class="settings-row-stack">
                          <div class="flex items-center justify-between">
                            <p class="text-xs text-muted-foreground">{{ $t('settings.account.profileFingerprint') }}</p>
                            <button class="settings-copy-btn" @click="copyText(identity.profile_hash)">{{ $t('common.actions.copy') }}</button>
                          </div>
                          <code class="settings-code">{{ identity.profile_hash }}</code>
                        </div>
                      </div>
                    </details>
                  </div>
                </template>

                <!-- ──────────── Security & Privacy ──────────── -->
                <template v-else-if="activeSection === 'security'">
                  <div>
                    <h4 class="settings-group-title">{{ $t('settings.security.accountTitle') }}</h4>
                    <div class="divide-y divide-border/50 rounded-lg border border-border">
                      <div class="flex items-center justify-between gap-4 p-4">
                        <div>
                          <p class="text-sm font-medium text-foreground">{{ $t('settings.security.recoveryPhrase') }}</p>
                          <p class="text-xs text-muted-foreground">{{ $t('settings.security.recoveryPhraseDesc') }}</p>
                        </div>
                        <AppButton variant="outline" size="sm" @click="openExportModal">
                          {{ $t('settings.security.export') }}
                        </AppButton>
                      </div>
                      <div class="flex items-center justify-between gap-4 p-4">
                        <div>
                          <p class="text-sm font-medium text-foreground">{{ $t('settings.security.lockTitle') }}</p>
                          <p class="text-xs text-muted-foreground">{{ $t('settings.security.lockDesc') }}</p>
                        </div>
                        <AppButton variant="outline" size="sm" :loading="locking" @click="lockWallet">
                          {{ $t('settings.security.lock') }}
                        </AppButton>
                      </div>
                    </div>
                  </div>

                  <div>
                    <h4 class="settings-group-title">{{ $t('settings.security.deviceTitle') }}</h4>
                    <div class="rounded-lg border border-border p-4">
                      <div class="flex items-center justify-between gap-4">
                        <div>
                          <p class="text-sm font-medium text-foreground">{{ $t('settings.security.biometricTitle') }}</p>
                          <p class="text-xs text-muted-foreground">{{ $t('settings.security.biometricDesc') }}</p>
                        </div>
                        <AppButton
                          v-if="biometricEnabled"
                          variant="outline"
                          size="sm"
                          :loading="biometricBusy"
                          @click="disableBiometric"
                        >
                          {{ $t('settings.security.disable') }}
                        </AppButton>
                      </div>

                      <div v-if="!biometricAvailable" class="mt-3 text-xs text-muted-foreground">
                        {{ $t('settings.security.biometricUnavailable') }}
                      </div>

                      <div v-else-if="!biometricEnabled" class="mt-3 flex flex-col sm:flex-row sm:items-end gap-2">
                        <div class="flex-1">
                          <AppInput
                            v-model="biometricPassword"
                            :label="$t('settings.security.passwordLabel')"
                            type="password"
                            :placeholder="$t('settings.security.passwordPlaceholder')"
                          />
                        </div>
                        <AppButton size="sm" :loading="biometricBusy" @click="enableBiometric">
                          {{ $t('settings.security.enable') }}
                        </AppButton>
                      </div>

                      <div v-else class="mt-3 text-xs text-emerald-600 dark:text-emerald-400">
                        {{ $t('settings.security.biometricEnabled') }}
                      </div>

                      <p v-if="biometricMessage" class="mt-2 text-xs text-muted-foreground">
                        {{ biometricMessage }}
                      </p>
                      <p v-if="biometricDiagnostics" class="mt-1 text-xs text-muted-foreground/80">
                        {{ biometricDiagnostics }}
                      </p>
                    </div>
                  </div>
                </template>

                <!-- ──────────── Personalization ──────────── -->
                <template v-else-if="activeSection === 'personalization'">
                  <div>
                    <h4 class="settings-group-title">{{ t('common.language.label') }}</h4>
                    <LanguageSelector />
                  </div>

                  <div>
                    <h4 class="settings-group-title">{{ $t('settings.personalization.themeTitle') }}</h4>
                    <div class="grid grid-cols-3 gap-2">
                      <button
                        v-for="opt in (['light', 'dark', 'system'] as const)"
                        :key="opt"
                        class="theme-card"
                        :class="{ 'theme-card--active': theme === opt }"
                        @click="setTheme(opt)"
                      >
                        <span class="theme-card-swatch" :class="`theme-card-swatch--${opt}`" aria-hidden="true" />
                        <span class="text-sm">{{ themeLabel(opt) }}</span>
                      </button>
                    </div>
                  </div>

                  <div>
                    <div class="flex items-center justify-between mb-3">
                      <h4 class="settings-group-title mb-0">{{ $t('settings.personalization.shortcutsTitle') }}</h4>
                      <AppButton variant="ghost" size="sm" @click="resetAllShortcuts">
                        {{ $t('settings.personalization.resetAll') }}
                      </AppButton>
                    </div>
                    <p class="text-xs text-muted-foreground mb-3">
                      {{ $t('settings.personalization.shortcutsHint') }}
                    </p>
                    <div class="divide-y divide-border/50 rounded-lg border border-border">
                      <div
                        v-for="def in Object.values(shortcuts)"
                        :key="def.id"
                        class="flex items-center justify-between px-4 py-3"
                      >
                        <span class="text-sm text-foreground">{{ def.label }}</span>
                        <div class="flex items-center gap-2">
                          <template v-if="recordingShortcutId === def.id">
                            <kbd
                              class="inline-flex min-w-[60px] items-center justify-center rounded-md border border-primary bg-primary/10 px-2 py-1 font-mono text-xs text-primary animate-pulse"
                              tabindex="0"
                              autofocus
                              @keydown="onShortcutKeydown"
                              @blur="cancelRecording"
                            >
                              {{ $t('settings.personalization.pressKeys') }}
                            </kbd>
                          </template>
                          <template v-else>
                            <button
                              class="inline-flex min-w-[60px] items-center justify-center rounded-md border border-border bg-muted/30 px-2 py-1 font-mono text-xs text-foreground transition-colors hover:bg-muted/60"
                              @click="startRecording(def.id)"
                            >
                              {{ formatCombo(def.keys) }}
                            </button>
                          </template>
                          <button
                            v-if="def.keys.key !== def.defaultKeys.key || def.keys.mod !== def.defaultKeys.mod || def.keys.shift !== def.defaultKeys.shift || def.keys.alt !== def.defaultKeys.alt"
                            class="text-xs text-muted-foreground hover:text-foreground transition-colors"
                            :title="$t('settings.personalization.resetToDefault')"
                            @click="resetShortcut(def.id)"
                          >
                            {{ $t('settings.personalization.reset') }}
                          </button>
                        </div>
                      </div>
                    </div>
                  </div>
                </template>

                <!-- ──────────── System ──────────── -->
                <template v-else-if="activeSection === 'system'">
                  <div>
                    <h4 class="settings-group-title">{{ $t('settings.storage.title') }}</h4>
                    <div v-if="storageStats" class="space-y-5 rounded-lg border border-border p-4">
                      <div>
                        <div class="flex items-center justify-between mb-1.5">
                          <p class="text-sm font-medium text-foreground">{{ $t('settings.storage.cacheLabel') }}</p>
                          <p class="text-xs text-muted-foreground">
                            {{ formatBytes(storageStats.total_pinned_bytes) }}
                            <template v-if="storageStats.quota_bytes > 0">
                              {{ $t('settings.storage.cacheOf', { size: formatBytes(storageStats.quota_bytes) }) }}
                            </template>
                            <template v-else>
                              {{ $t('settings.storage.unlimited') }}
                            </template>
                          </p>
                        </div>
                        <div class="h-2 rounded-full bg-muted/50 overflow-hidden">
                          <div
                            class="h-full rounded-full transition-all duration-300"
                            :class="usageColor(storageStats.usage_percent)"
                            :style="{ width: storageStats.usage_percent !== null ? `${Math.min(storageStats.usage_percent, 100)}%` : '0%' }"
                          />
                        </div>
                        <p class="mt-1 text-xs text-muted-foreground">
                          {{ $t('settings.storage.cached', { count: storageStats.pin_count }, storageStats.pin_count) }}
                          <template v-if="storageStats.evictable_bytes > 0">
                            &middot; {{ $t('settings.storage.canBeFreed', { size: formatBytes(storageStats.evictable_bytes) }) }}
                          </template>
                        </p>
                      </div>

                      <div>
                        <p class="text-sm font-medium text-foreground mb-2">{{ $t('settings.storage.quotaLabel') }}</p>
                        <div class="flex flex-wrap gap-2">
                          <button
                            v-for="option in QUOTA_OPTIONS"
                            :key="option.bytes"
                            class="px-3 py-1.5 text-xs font-medium rounded-lg border transition-colors"
                            :class="quotaBytes === option.bytes
                              ? 'bg-primary text-primary-foreground border-primary'
                              : 'bg-background text-foreground border-border hover:bg-muted/50'"
                            @click="setQuota(option.bytes)"
                          >
                            {{ option.bytes === 0 ? $t('settings.storage.unlimitedOption') : option.label }}
                          </button>
                        </div>
                      </div>

                      <div class="flex items-center gap-3">
                        <AppButton
                          variant="outline"
                          size="sm"
                          :loading="evicting"
                          :disabled="storageStats.evictable_bytes === 0"
                          @click="freeSpace"
                        >
                          {{ $t('settings.storage.freeSpace') }}
                        </AppButton>
                        <span v-if="storageMessage" class="text-xs text-muted-foreground">{{ storageMessage }}</span>
                      </div>
                    </div>
                    <div v-else class="rounded-lg border border-border p-4 text-sm text-muted-foreground">
                      {{ $t('settings.storage.loading') }}
                    </div>
                  </div>

                  <div>
                    <h4 class="settings-group-title">{{ $t('settings.network.title') }}</h4>
                    <div class="rounded-lg border border-border p-4">
                      <div class="flex items-center justify-between gap-4">
                        <div>
                          <p class="text-sm font-medium text-foreground">{{ $t('settings.network.nodeLabel') }}</p>
                          <p class="text-xs text-muted-foreground">
                            <template v-if="p2pStatus?.is_running">
                              {{ $t('common.status.connected') }} · {{ $t('settings.network.deviceIdLabel') }}
                              <code class="font-mono text-[11px]">{{ p2pStatus.peer_id?.slice(0, 12) }}…</code>
                            </template>
                            <template v-else-if="p2pStatus">
                              {{ $t('common.status.offline') }}
                            </template>
                            <template v-else>
                              {{ $t('settings.network.starting') }}
                            </template>
                          </p>
                        </div>
                        <AppButton variant="outline" size="sm" @click="gotoNetwork">
                          {{ $t('settings.network.openNetwork') }}
                        </AppButton>
                      </div>
                    </div>

                    <div class="mt-3">
                      <RelayManager />
                    </div>
                  </div>
                </template>

                <!-- ──────────── Plugins ──────────── -->
                <template v-else-if="activeSection === 'plugins'">
                  <PluginsPanel />
                </template>

                <!-- ──────────── Guardian (oversight transparency) ──────────── -->
                <template v-else-if="activeSection === 'guardian'">
                  <GuardianPanel />
                </template>

                <!-- ──────────── Advanced — every registered setting ──────────── -->
                <template v-else-if="activeSection === 'advanced'">
                  <AdvancedSettingsPanel />
                </template>
              </div>
            </section>

        <!-- Export Recovery Phrase modal -->
        <AppModal
          :open="showExportModal"
          :title="$t('settings.export.title')"
          max-width="28rem"
          @close="closeExportModal"
        >
          <div v-if="!exportedMnemonic && !exportConfirmed">
            <AppAlert variant="error" class="mb-4">
              {{ $t('settings.export.warning') }}
            </AppAlert>
            <div class="flex gap-2">
              <AppButton variant="ghost" class="flex-1" @click="closeExportModal">{{ $t('common.actions.cancel') }}</AppButton>
              <AppButton variant="danger" class="flex-1" @click="confirmExport">
                {{ $t('settings.export.understand') }}
              </AppButton>
            </div>
          </div>

          <div v-else-if="exportConfirmed && !exportedMnemonic && !exporting && !exportError">
            <p class="text-sm text-muted-foreground mb-3">{{ $t('settings.export.enterPassword') }}</p>
            <AppInput
              v-model="exportPassword"
              type="password"
              :placeholder="$t('settings.export.passwordPlaceholder')"
              class="mb-3"
              @keyup.enter="doExport"
            />
            <div class="flex gap-2">
              <AppButton variant="ghost" class="flex-1" @click="closeExportModal">{{ $t('common.actions.cancel') }}</AppButton>
              <AppButton
                variant="danger"
                class="flex-1"
                :disabled="!exportPassword"
                @click="doExport"
              >
                {{ $t('settings.export.showPhrase') }}
              </AppButton>
            </div>
          </div>

          <div v-else-if="exporting" class="text-center py-4">
            <div class="w-6 h-6 border-2 border-primary border-t-transparent rounded-full animate-spin mx-auto mb-2" />
            <p class="text-sm text-muted-foreground">{{ $t('settings.export.decrypting') }}</p>
          </div>

          <div v-else-if="exportError">
            <AppAlert variant="error" class="mb-3">{{ exportError }}</AppAlert>
            <div class="flex gap-2">
              <AppButton variant="outline" class="flex-1" @click="closeExportModal">{{ $t('common.actions.close') }}</AppButton>
              <AppButton variant="ghost" class="flex-1" @click="exportError = ''">{{ $t('settings.export.tryAgain') }}</AppButton>
            </div>
          </div>

          <div v-else-if="exportedMnemonic">
            <div class="grid grid-cols-2 sm:grid-cols-3 gap-2 mb-4">
              <div
                v-for="(word, i) in exportedMnemonic.split(' ')"
                :key="i"
                class="flex items-center gap-2 text-sm py-1.5 px-2.5 rounded bg-muted/30"
              >
                <span class="text-xs text-muted-foreground w-5 text-end">{{ i + 1 }}.</span>
                <span class="font-mono font-medium">{{ word }}</span>
              </div>
            </div>
            <AppButton class="w-full" @click="closeExportModal">{{ $t('common.actions.done') }}</AppButton>
          </div>
        </AppModal>
  </div>
</template>

<style scoped>
.settings-page {
  height: 100%;
  min-height: 0;
}

.settings-sidebar {
  flex-shrink: 0;
}

.settings-nav-item {
  display: flex;
  align-items: center;
  gap: 0.625rem;
  width: 100%;
  padding: 0.5rem 0.75rem;
  border-radius: 0.5rem;
  color: var(--app-foreground);
  background: transparent;
  border: none;
  cursor: pointer;
  text-align: left;
  transition: background 0.15s, color 0.15s;
  flex-shrink: 0;
}

.settings-nav-item:hover {
  background: color-mix(in srgb, var(--app-muted) 50%, transparent);
}

.settings-nav-item--active {
  background: color-mix(in srgb, var(--app-primary) 10%, transparent);
  color: var(--app-primary);
}

.settings-nav-item--active .settings-nav-icon {
  color: var(--app-primary);
}

.settings-nav-icon {
  display: inline-flex;
  flex-shrink: 0;
  width: 1.5rem;
  height: 1.5rem;
  align-items: center;
  justify-content: center;
  color: var(--app-muted-foreground);
}

.settings-group-title {
  font-size: 0.75rem;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.05em;
  color: var(--app-muted-foreground);
  margin-bottom: 0.75rem;
}

.settings-row-stack {
  padding: 0.75rem;
  border-radius: 0.5rem;
  border: 1px solid var(--app-border);
  background: color-mix(in srgb, var(--app-muted) 15%, transparent);
}

.settings-code {
  display: block;
  margin-top: 0.375rem;
  font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
  font-size: 0.6875rem;
  word-break: break-all;
  user-select: all;
  color: var(--app-foreground);
}

.settings-copy-btn {
  font-size: 0.6875rem;
  color: var(--app-muted-foreground);
  background: transparent;
  border: none;
  padding: 0.125rem 0.375rem;
  border-radius: 0.25rem;
  cursor: pointer;
  transition: color 0.15s, background 0.15s;
}

.settings-copy-btn:hover {
  color: var(--app-foreground);
  background: color-mix(in srgb, var(--app-muted) 50%, transparent);
}

/* Theme picker cards */
.theme-card {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 0.5rem;
  padding: 0.75rem;
  border-radius: 0.5rem;
  border: 1px solid var(--app-border);
  background: var(--app-background);
  color: var(--app-foreground);
  cursor: pointer;
  transition: border-color 0.15s, background 0.15s;
}

.theme-card:hover {
  background: color-mix(in srgb, var(--app-muted) 30%, transparent);
}

.theme-card--active {
  border-color: var(--app-primary);
  background: color-mix(in srgb, var(--app-primary) 8%, transparent);
}

.theme-card-swatch {
  width: 100%;
  height: 2.5rem;
  border-radius: 0.375rem;
  border: 1px solid var(--app-border);
}

.theme-card-swatch--light {
  background: linear-gradient(135deg, #ffffff 50%, #f3f4f6 50%);
}

.theme-card-swatch--dark {
  background: linear-gradient(135deg, #0b0b0e 50%, #1f1f25 50%);
}

.theme-card-swatch--system {
  background: linear-gradient(135deg, #ffffff 50%, #0b0b0e 50%);
}

.vis-option {
  flex: 1;
  display: flex;
  flex-direction: column;
  gap: 0.25rem;
  align-items: flex-start;
  padding: 0.6rem 0.8rem;
  border-radius: 0.6rem;
  border: 1px solid var(--app-border);
  background: var(--app-card);
  font-size: 0.8rem;
  font-weight: 600;
  color: var(--app-foreground);
  text-align: left;
  transition: border-color 0.15s, background 0.15s;
}
.vis-option--active {
  border-color: var(--app-primary);
  background: color-mix(in srgb, var(--app-primary) 7%, var(--app-card));
}
.vis-desc {
  font-size: 0.68rem;
  font-weight: 400;
  color: var(--app-muted-foreground);
}
</style>
