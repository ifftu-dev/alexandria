<script setup lang="ts">
/**
 * Plugin management page (Phase 1).
 *
 * Lists installed plugins, lets the user install a new one from a local
 * directory path, inspect manifest details, manage capability grants,
 * and uninstall.
 *
 * Phase 3 will replace the path-input field with a P2P discovery browser
 * driven by the `/alexandria/plugins/1.0` gossip topic. The install flow
 * (manifest verify → bundle copy → DB record) is the same.
 */

import { ref, onMounted, computed } from 'vue'
import { useI18n } from 'vue-i18n'
import { useLocalApi } from '@/composables/useLocalApi'
import { useDisplayNames } from '@/composables/useDisplayNames'
import { AppButton, AppInput, AppSpinner, AppAlert, EmptyState, AppBadge } from '@/components/ui'
import type {
  InstalledPlugin,
  PluginManifest,
  PluginCapability,
  PluginPermissionRecord,
  PluginAttestationStatus,
} from '@/types'

const { t } = useI18n()
const { invoke } = useLocalApi()
const { displayName, ensureNames } = useDisplayNames()

const plugins = ref<InstalledPlugin[]>([])
const loading = ref(true)
const installPath = ref('')
const installing = ref(false)
const installError = ref<string | null>(null)
const installSuccess = ref<string | null>(null)

/** Attestation status keyed by plugin_cid, populated alongside `plugins`. */
const attestation = ref<Record<string, PluginAttestationStatus>>({})

const expandedCid = ref<string | null>(null)
const expandedManifest = ref<PluginManifest | null>(null)
const expandedPermissions = ref<PluginPermissionRecord[]>([])
const expandedLoading = ref(false)

onMounted(() => {
  void refresh()
})

async function refresh() {
  loading.value = true
  try {
    plugins.value = await invoke<InstalledPlugin[]>('plugin_list')
    void ensureNames(plugins.value.map((p) => p.author_did))
    const lookups = await Promise.all(
      plugins.value.map((p) =>
        invoke<PluginAttestationStatus>('plugin_attestation_status', {
          pluginCid: p.plugin_cid,
        }).catch(() => null),
      ),
    )
    const next: Record<string, PluginAttestationStatus> = {}
    plugins.value.forEach((p, i) => {
      const s = lookups[i]
      if (s) next[p.plugin_cid] = s
    })
    attestation.value = next
  } catch (e) {
    installError.value = t('plugins.installed.errors.listFailed', { error: String(e) })
  } finally {
    loading.value = false
  }
}

function attestationBadge(cid: string): { label: string; variant: 'success' | 'warning' | 'secondary' } {
  const s = attestation.value[cid]
  if (!s) return { label: t('plugins.badge.statusPending'), variant: 'secondary' }
  if (s.advisories.some((a) => a.kind === 'known_flawed')) {
    return { label: t('plugins.badge.knownFlawed'), variant: 'warning' }
  }
  if (s.attested) return { label: t('plugins.badge.attested'), variant: 'success' }
  return { label: t('plugins.badge.unattested'), variant: 'secondary' }
}

async function install() {
  const path = installPath.value.trim()
  if (!path) {
    installError.value = t('plugins.installed.errors.enterPath')
    return
  }
  installing.value = true
  installError.value = null
  installSuccess.value = null
  try {
    const installed = await invoke<InstalledPlugin>('plugin_install_from_file', {
      directory: path,
    })
    installSuccess.value = t('plugins.installed.errors.installSuccess', {
      name: installed.name,
      version: installed.version,
    })
    installPath.value = ''
    await refresh()
  } catch (e) {
    installError.value = t('plugins.installed.errors.installFailed', { error: String(e) })
  } finally {
    installing.value = false
  }
}

async function toggleExpand(cid: string) {
  if (expandedCid.value === cid) {
    expandedCid.value = null
    expandedManifest.value = null
    expandedPermissions.value = []
    return
  }
  expandedCid.value = cid
  expandedLoading.value = true
  expandedManifest.value = null
  expandedPermissions.value = []
  try {
    const [m, perms] = await Promise.all([
      invoke<PluginManifest>('plugin_get_manifest', { pluginCid: cid }),
      invoke<PluginPermissionRecord[]>('plugin_list_permissions', { pluginCid: cid }),
    ])
    expandedManifest.value = m
    expandedPermissions.value = perms
  } catch (e) {
    installError.value = t('plugins.installed.errors.detailsFailed', { error: String(e) })
  } finally {
    expandedLoading.value = false
  }
}

async function uninstall(cid: string, name: string) {
  // eslint-disable-next-line no-alert
  if (!confirm(t('plugins.installed.uninstallConfirm', { name }))) {
    return
  }
  try {
    await invoke('plugin_uninstall', { pluginCid: cid })
    if (expandedCid.value === cid) {
      expandedCid.value = null
      expandedManifest.value = null
      expandedPermissions.value = []
    }
    await refresh()
  } catch (e) {
    installError.value = t('plugins.installed.errors.uninstallFailed', { error: String(e) })
  }
}

async function revoke(cid: string, capability: PluginCapability) {
  try {
    await invoke('plugin_revoke_capability', { pluginCid: cid, capability })
    expandedPermissions.value = await invoke<PluginPermissionRecord[]>('plugin_list_permissions', {
      pluginCid: cid,
    })
  } catch (e) {
    installError.value = t('plugins.installed.errors.revokeFailed', { error: String(e) })
  }
}

const empty = computed(() => !loading.value && plugins.value.length === 0)

function shortCid(cid: string): string {
  return cid.length > 16 ? `${cid.slice(0, 12)}…${cid.slice(-4)}` : cid
}

</script>

<template>
  <div class="mx-auto max-w-3xl px-4 py-6 md:px-6 md:py-8">
    <header class="mb-6">
      <h1 class="text-2xl font-bold text-foreground">{{ $t('plugins.installed.title') }}</h1>
      <p class="mt-1 text-sm text-muted-foreground">
        {{ $t('plugins.installed.intro') }}
      </p>
    </header>

    <!-- Install from local file -->
    <section class="mb-8 rounded-2xl border border-border bg-card/50 p-4">
      <h2 class="text-sm font-semibold text-foreground">{{ $t('plugins.installed.installHeading') }}</h2>
      <p class="mt-1 text-xs text-muted-foreground">
        {{ $t('plugins.installed.installHintPrefix') }} <code class="font-mono">manifest.json</code>,
        <code class="font-mono">manifest.sig</code>{{ $t('plugins.installed.installHintAnd') }} <code class="font-mono">ui/</code>
        {{ $t('plugins.installed.installHintSuffix') }}
      </p>
      <div class="mt-3 flex gap-2">
        <AppInput
          v-model="installPath"
          :placeholder="$t('plugins.installed.installPlaceholder')"
          class="flex-1"
          :disabled="installing"
        />
        <AppButton :loading="installing" @click="install">{{ $t('plugins.installed.actions.add') }}</AppButton>
      </div>
      <AppAlert v-if="installError" variant="error" class="mt-3">{{ installError }}</AppAlert>
      <AppAlert v-if="installSuccess" variant="success" class="mt-3">{{ installSuccess }}</AppAlert>
    </section>

    <!-- Installed list -->
    <section>
      <h2 class="mb-3 text-sm font-semibold text-foreground">{{ $t('plugins.installed.installedHeading') }}</h2>

      <div v-if="loading" class="flex justify-center p-10">
        <AppSpinner />
      </div>

      <EmptyState
        v-else-if="empty"
        :title="$t('plugins.installed.emptyTitle')"
        :description="$t('plugins.installed.emptyDescription')"
      />

      <ul v-else class="space-y-2">
        <li
          v-for="p in plugins"
          :key="p.plugin_cid"
          class="rounded-xl border border-border bg-card/40"
        >
          <div class="flex items-start justify-between gap-3 p-4">
            <div class="min-w-0 flex-1">
              <div class="flex flex-wrap items-center gap-2">
                <h3 class="text-sm font-semibold text-foreground">{{ p.name }}</h3>
                <AppBadge variant="secondary">v{{ p.version }}</AppBadge>
                <AppBadge variant="secondary">{{ p.source }}</AppBadge>
                <AppBadge :variant="attestationBadge(p.plugin_cid).variant">
                  {{ attestationBadge(p.plugin_cid).label }}
                </AppBadge>
              </div>
              <div class="mt-1 flex flex-wrap items-center gap-x-3 gap-y-1 text-[11px] text-muted-foreground">
                <span>{{ $t('plugins.meta.author') }}: {{ displayName(p.author_did) }}</span>
                <span>{{ $t('plugins.meta.installed') }}: {{ new Date(p.installed_at).toLocaleString() }}</span>
                <details>
                  <summary class="cursor-pointer">{{ $t('common.advanced.toggle') }}</summary>
                  <span>{{ $t('plugins.meta.contentId') }}: <code class="font-mono">{{ shortCid(p.plugin_cid) }}</code></span>
                </details>
              </div>
            </div>
            <div class="flex gap-2">
              <AppButton size="sm" variant="ghost" @click="toggleExpand(p.plugin_cid)">
                {{ expandedCid === p.plugin_cid ? $t('plugins.installed.actions.hide') : $t('plugins.installed.actions.details') }}
              </AppButton>
              <AppButton size="sm" variant="danger" @click="uninstall(p.plugin_cid, p.name)">
                {{ $t('common.actions.remove') }}
              </AppButton>
            </div>
          </div>

          <!-- Expanded detail -->
          <div
            v-if="expandedCid === p.plugin_cid"
            class="border-t border-border/50 bg-background/40 p-4"
          >
            <AppSpinner v-if="expandedLoading" />
            <template v-else-if="expandedManifest">
              <p v-if="expandedManifest.description" class="text-sm text-muted-foreground">
                {{ expandedManifest.description }}
              </p>

              <dl class="mt-3 grid grid-cols-2 gap-x-4 gap-y-2 text-xs">
                <dt class="text-muted-foreground">{{ $t('plugins.installed.detail.kinds') }}</dt>
                <dd class="text-foreground">{{ expandedManifest.kinds.join(', ') }}</dd>
                <dt class="text-muted-foreground">{{ $t('plugins.installed.detail.declaredCapabilities') }}</dt>
                <dd class="text-foreground">
                  {{
                    expandedManifest.capabilities.length
                      ? expandedManifest.capabilities.join(', ')
                      : $t('plugins.installed.detail.none')
                  }}
                </dd>
                <dt class="text-muted-foreground">{{ $t('plugins.installed.detail.platforms') }}</dt>
                <dd class="text-foreground">
                  {{
                    expandedManifest.platforms.length
                      ? expandedManifest.platforms.join(', ')
                      : $t('plugins.installed.detail.unspecified')
                  }}
                </dd>
                <dt class="text-muted-foreground">{{ $t('plugins.installed.detail.apiVersion') }}</dt>
                <dd class="text-foreground">{{ expandedManifest.api_version }}</dd>
              </dl>

              <h4 class="mt-4 text-xs font-semibold uppercase tracking-wide text-muted-foreground">
                {{ $t('plugins.installed.detail.grantedHeading') }}
              </h4>
              <p
                v-if="expandedPermissions.length === 0"
                class="mt-1 text-xs text-muted-foreground"
              >
                {{ $t('plugins.installed.detail.noGrants') }}
              </p>
              <ul v-else class="mt-2 space-y-1">
                <li
                  v-for="perm in expandedPermissions"
                  :key="perm.capability"
                  class="flex items-center justify-between rounded-md bg-muted/20 px-2 py-1 text-xs"
                >
                  <span>
                    <span class="font-medium text-foreground">{{ perm.capability }}</span>
                    <span class="ms-2 text-muted-foreground">({{ perm.scope }})</span>
                  </span>
                  <button
                    class="text-destructive transition-colors hover:text-destructive/80"
                    @click="revoke(p.plugin_cid, perm.capability)"
                  >
                    {{ $t('plugins.installed.detail.revoke') }}
                  </button>
                </li>
              </ul>
            </template>
          </div>
        </li>
      </ul>
    </section>
  </div>
</template>
