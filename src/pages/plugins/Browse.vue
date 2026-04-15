<script setup lang="ts">
/**
 * Plugin discovery page (Phase 3).
 *
 * Lists every plugin known to this node — built-ins seeded at startup,
 * locally-installed plugins, and anything seen on the
 * `/alexandria/plugins/1.0` gossip topic. Each row shows the DAO
 * attestation badge so users can tell at a glance whether a plugin's
 * graded submissions are credential-eligible under the default
 * verifier policy.
 */

import { ref, onMounted, computed } from 'vue'
import { useLocalApi } from '@/composables/useLocalApi'
import { AppSpinner, AppAlert, AppBadge, EmptyState } from '@/components/ui'
import type {
  PluginCatalogEntry,
  PluginAttestationStatus,
} from '@/types'

const { invoke } = useLocalApi()

const entries = ref<PluginCatalogEntry[]>([])
const loading = ref(true)
const loadError = ref<string | null>(null)

/** Per-plugin attestation status, keyed by plugin_cid. */
const status = ref<Record<string, PluginAttestationStatus>>({})

onMounted(async () => {
  try {
    entries.value = await invoke<PluginCatalogEntry[]>('plugin_browse_catalog')
    // Fan out attestation lookups in parallel.
    const lookups = await Promise.all(
      entries.value.map((e) =>
        invoke<PluginAttestationStatus>('plugin_attestation_status', {
          pluginCid: e.plugin_cid,
        }).catch(() => null),
      ),
    )
    const next: Record<string, PluginAttestationStatus> = {}
    entries.value.forEach((e, i) => {
      const s = lookups[i]
      if (s) next[e.plugin_cid] = s
    })
    status.value = next
  } catch (e) {
    loadError.value = `Failed to load plugin catalog: ${e}`
  } finally {
    loading.value = false
  }
})

const grouped = computed(() => {
  const builtins: PluginCatalogEntry[] = []
  const community: PluginCatalogEntry[] = []
  for (const e of entries.value) {
    if (e.source === 'builtin') builtins.push(e)
    else community.push(e)
  }
  return { builtins, community }
})

function attestationBadge(cid: string): { label: string; variant: 'success' | 'warning' | 'secondary' } {
  const s = status.value[cid]
  if (!s) return { label: 'Unknown', variant: 'secondary' }
  if (s.advisories.some((a) => a.kind === 'known_flawed')) {
    return { label: 'Known flawed', variant: 'warning' }
  }
  if (s.attested) return { label: 'DAO attested', variant: 'success' }
  return { label: 'Unattested', variant: 'secondary' }
}

function shortCid(cid: string): string {
  return cid.length > 16 ? `${cid.slice(0, 12)}…${cid.slice(-4)}` : cid
}

function shortDid(did: string): string {
  return did.length > 24 ? `${did.slice(0, 16)}…${did.slice(-6)}` : did
}
</script>

<template>
  <div class="mx-auto max-w-4xl px-4 py-6 md:px-6 md:py-8">
    <header class="mb-6">
      <h1 class="text-2xl font-bold text-foreground">Browse plugins</h1>
      <p class="mt-1 text-sm text-muted-foreground">
        Built-in element types and any community plugins this node has heard of.
        DAO-attested plugins can issue credentials recognized under the default
        verifier policy; unattested plugins still run, but their grades are
        progress-only until the DAO recognizes them.
      </p>
    </header>

    <div v-if="loading" class="flex justify-center p-10">
      <AppSpinner />
    </div>

    <AppAlert v-else-if="loadError" variant="error">{{ loadError }}</AppAlert>

    <template v-else>
      <section v-if="grouped.builtins.length > 0" class="mb-8">
        <h2 class="mb-3 text-sm font-semibold uppercase tracking-wide text-muted-foreground">
          Built-in
        </h2>
        <ul class="space-y-2">
          <li
            v-for="e in grouped.builtins"
            :key="e.plugin_cid"
            class="flex items-start justify-between gap-3 rounded-xl border border-border bg-card/40 p-4"
          >
            <div class="min-w-0 flex-1">
              <div class="flex items-center gap-2">
                <h3 class="text-sm font-semibold text-foreground">{{ e.name }}</h3>
                <AppBadge variant="secondary">v{{ e.version }}</AppBadge>
                <AppBadge variant="secondary">built-in</AppBadge>
                <AppBadge :variant="attestationBadge(e.plugin_cid).variant">
                  {{ attestationBadge(e.plugin_cid).label }}
                </AppBadge>
              </div>
              <p v-if="e.description" class="mt-1 text-xs text-muted-foreground">
                {{ e.description }}
              </p>
              <div class="mt-1 flex flex-wrap items-center gap-x-3 gap-y-1 text-[11px] text-muted-foreground">
                <span>Kinds: {{ e.kinds.join(', ') }}</span>
                <span v-if="e.has_grader">Graded</span>
                <span>CID: <code class="font-mono">{{ shortCid(e.plugin_cid) }}</code></span>
              </div>
            </div>
          </li>
        </ul>
      </section>

      <section>
        <h2 class="mb-3 text-sm font-semibold uppercase tracking-wide text-muted-foreground">
          Community
        </h2>
        <EmptyState
          v-if="grouped.community.length === 0"
          title="No community plugins yet"
          description="Plugin announcements arrive over the P2P network. Once peers publish, they'll show up here."
        />
        <ul v-else class="space-y-2">
          <li
            v-for="e in grouped.community"
            :key="e.plugin_cid"
            class="flex items-start justify-between gap-3 rounded-xl border border-border bg-card/40 p-4"
          >
            <div class="min-w-0 flex-1">
              <div class="flex flex-wrap items-center gap-2">
                <h3 class="text-sm font-semibold text-foreground">{{ e.name }}</h3>
                <AppBadge variant="secondary">v{{ e.version }}</AppBadge>
                <AppBadge variant="secondary">{{ e.source }}</AppBadge>
                <AppBadge :variant="attestationBadge(e.plugin_cid).variant">
                  {{ attestationBadge(e.plugin_cid).label }}
                </AppBadge>
              </div>
              <p v-if="e.description" class="mt-1 text-xs text-muted-foreground">
                {{ e.description }}
              </p>
              <div class="mt-1 flex flex-wrap items-center gap-x-3 gap-y-1 text-[11px] text-muted-foreground">
                <span>Author: <code class="font-mono">{{ shortDid(e.author_did) }}</code></span>
                <span>Kinds: {{ e.kinds.join(', ') }}</span>
                <span v-if="e.capabilities.length > 0">
                  Caps: {{ e.capabilities.join(', ') }}
                </span>
                <span>CID: <code class="font-mono">{{ shortCid(e.plugin_cid) }}</code></span>
              </div>
            </div>
          </li>
        </ul>
      </section>
    </template>
  </div>
</template>
