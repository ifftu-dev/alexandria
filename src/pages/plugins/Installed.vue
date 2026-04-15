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
import { useLocalApi } from '@/composables/useLocalApi'
import { AppButton, AppInput, AppSpinner, AppAlert, EmptyState, AppBadge } from '@/components/ui'
import type {
  InstalledPlugin,
  PluginManifest,
  PluginCapability,
  PluginPermissionRecord,
} from '@/types'

const { invoke } = useLocalApi()

const plugins = ref<InstalledPlugin[]>([])
const loading = ref(true)
const installPath = ref('')
const installing = ref(false)
const installError = ref<string | null>(null)
const installSuccess = ref<string | null>(null)

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
  } catch (e) {
    installError.value = `Failed to list plugins: ${e}`
  } finally {
    loading.value = false
  }
}

async function install() {
  const path = installPath.value.trim()
  if (!path) {
    installError.value = 'Enter the path to a plugin bundle directory.'
    return
  }
  installing.value = true
  installError.value = null
  installSuccess.value = null
  try {
    const installed = await invoke<InstalledPlugin>('plugin_install_from_file', {
      directory: path,
    })
    installSuccess.value = `Installed "${installed.name}" v${installed.version}.`
    installPath.value = ''
    await refresh()
  } catch (e) {
    installError.value = `Install failed: ${e}`
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
    installError.value = `Failed to load plugin details: ${e}`
  } finally {
    expandedLoading.value = false
  }
}

async function uninstall(cid: string, name: string) {
  // eslint-disable-next-line no-alert
  if (!confirm(`Uninstall "${name}"? This removes the plugin from disk. Courses that use it will stop working until you re-install.`)) {
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
    installError.value = `Uninstall failed: ${e}`
  }
}

async function revoke(cid: string, capability: PluginCapability) {
  try {
    await invoke('plugin_revoke_capability', { pluginCid: cid, capability })
    expandedPermissions.value = await invoke<PluginPermissionRecord[]>('plugin_list_permissions', {
      pluginCid: cid,
    })
  } catch (e) {
    installError.value = `Revoke failed: ${e}`
  }
}

const empty = computed(() => !loading.value && plugins.value.length === 0)

function shortCid(cid: string): string {
  return cid.length > 16 ? `${cid.slice(0, 12)}…${cid.slice(-4)}` : cid
}

function shortDid(did: string): string {
  return did.length > 24 ? `${did.slice(0, 16)}…${did.slice(-6)}` : did
}
</script>

<template>
  <div class="mx-auto max-w-3xl px-4 py-6 md:px-6 md:py-8">
    <header class="mb-6">
      <h1 class="text-2xl font-bold text-foreground">Plugins</h1>
      <p class="mt-1 text-sm text-muted-foreground">
        Community-authored learning and assessment elements. Plugins run in a
        sandboxed iframe with no network access; you grant capabilities (microphone,
        camera, etc.) per-plugin on first use.
      </p>
    </header>

    <!-- Install from local file -->
    <section class="mb-8 rounded-2xl border border-border bg-card/50 p-4">
      <h2 class="text-sm font-semibold text-foreground">Install from local directory</h2>
      <p class="mt-1 text-xs text-muted-foreground">
        Point to a directory containing <code class="font-mono">manifest.json</code>,
        <code class="font-mono">manifest.sig</code>, and a <code class="font-mono">ui/</code>
        bundle. Phase 3 will add discovery from the P2P plugin catalog.
      </p>
      <div class="mt-3 flex gap-2">
        <AppInput
          v-model="installPath"
          placeholder="/absolute/path/to/plugin-bundle"
          class="flex-1"
          :disabled="installing"
        />
        <AppButton :loading="installing" @click="install">Install</AppButton>
      </div>
      <AppAlert v-if="installError" variant="error" class="mt-3">{{ installError }}</AppAlert>
      <AppAlert v-if="installSuccess" variant="success" class="mt-3">{{ installSuccess }}</AppAlert>
    </section>

    <!-- Installed list -->
    <section>
      <h2 class="mb-3 text-sm font-semibold text-foreground">Installed plugins</h2>

      <div v-if="loading" class="flex justify-center p-10">
        <AppSpinner />
      </div>

      <EmptyState
        v-else-if="empty"
        title="No plugins installed"
        description="Install a plugin above to add new learning and assessment element types."
      />

      <ul v-else class="space-y-2">
        <li
          v-for="p in plugins"
          :key="p.plugin_cid"
          class="rounded-xl border border-border bg-card/40"
        >
          <div class="flex items-start justify-between gap-3 p-4">
            <div class="min-w-0 flex-1">
              <div class="flex items-center gap-2">
                <h3 class="text-sm font-semibold text-foreground">{{ p.name }}</h3>
                <AppBadge variant="secondary">v{{ p.version }}</AppBadge>
                <AppBadge variant="secondary">{{ p.source }}</AppBadge>
              </div>
              <div class="mt-1 flex flex-wrap items-center gap-x-3 gap-y-1 text-[11px] text-muted-foreground">
                <span>Author: <code class="font-mono">{{ shortDid(p.author_did) }}</code></span>
                <span>CID: <code class="font-mono">{{ shortCid(p.plugin_cid) }}</code></span>
                <span>Installed: {{ new Date(p.installed_at).toLocaleString() }}</span>
              </div>
            </div>
            <div class="flex gap-2">
              <AppButton size="sm" variant="ghost" @click="toggleExpand(p.plugin_cid)">
                {{ expandedCid === p.plugin_cid ? 'Hide' : 'Details' }}
              </AppButton>
              <AppButton size="sm" variant="danger" @click="uninstall(p.plugin_cid, p.name)">
                Uninstall
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
                <dt class="text-muted-foreground">Kinds</dt>
                <dd class="text-foreground">{{ expandedManifest.kinds.join(', ') }}</dd>
                <dt class="text-muted-foreground">Declared capabilities</dt>
                <dd class="text-foreground">
                  {{
                    expandedManifest.capabilities.length
                      ? expandedManifest.capabilities.join(', ')
                      : 'none'
                  }}
                </dd>
                <dt class="text-muted-foreground">Platforms</dt>
                <dd class="text-foreground">
                  {{
                    expandedManifest.platforms.length
                      ? expandedManifest.platforms.join(', ')
                      : 'unspecified'
                  }}
                </dd>
                <dt class="text-muted-foreground">API version</dt>
                <dd class="text-foreground">{{ expandedManifest.api_version }}</dd>
              </dl>

              <h4 class="mt-4 text-xs font-semibold uppercase tracking-wide text-muted-foreground">
                Granted permissions
              </h4>
              <p
                v-if="expandedPermissions.length === 0"
                class="mt-1 text-xs text-muted-foreground"
              >
                No persistent grants. Capabilities will be requested on first use.
              </p>
              <ul v-else class="mt-2 space-y-1">
                <li
                  v-for="perm in expandedPermissions"
                  :key="perm.capability"
                  class="flex items-center justify-between rounded-md bg-muted/20 px-2 py-1 text-xs"
                >
                  <span>
                    <span class="font-medium text-foreground">{{ perm.capability }}</span>
                    <span class="ml-2 text-muted-foreground">({{ perm.scope }})</span>
                  </span>
                  <button
                    class="text-destructive transition-colors hover:text-destructive/80"
                    @click="revoke(p.plugin_cid, perm.capability)"
                  >
                    Revoke
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
