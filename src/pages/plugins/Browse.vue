<script setup lang="ts">
/**
 * Plugin discovery page (Phase 3).
 *
 * Lists every plugin known to this node — built-ins seeded at startup,
 * locally-installed plugins, and anything seen on the
 * `/alexandria/plugins/1.0` gossip topic.
 */

import { ref, onMounted, computed } from 'vue'
import { useI18n } from 'vue-i18n'
import { useLocalApi } from '@/composables/useLocalApi'
import { useDisplayNames } from '@/composables/useDisplayNames'
import { AppSpinner, AppAlert, AppBadge, EmptyState } from '@/components/ui'
import type { PluginCatalogEntry } from '@/types'

const { t } = useI18n()
const { invoke } = useLocalApi()
const { displayName, ensureNames } = useDisplayNames()

const entries = ref<PluginCatalogEntry[]>([])
const loading = ref(true)
const loadError = ref<string | null>(null)

onMounted(async () => {
  try {
    entries.value = await invoke<PluginCatalogEntry[]>('plugin_browse_catalog')
    void ensureNames(entries.value.map((e) => e.author_did))
  } catch (e) {
    loadError.value = t('plugins.browse.loadFailed', { error: String(e) })
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

function shortCid(cid: string): string {
  return cid.length > 16 ? `${cid.slice(0, 12)}…${cid.slice(-4)}` : cid
}
</script>

<template>
  <div class="mx-auto max-w-4xl px-4 py-6 md:px-6 md:py-8">
    <header class="mb-6">
      <h1 class="text-2xl font-bold text-foreground">{{ $t('plugins.browse.title') }}</h1>
      <p class="mt-1 text-sm text-muted-foreground">
        {{ $t('plugins.browse.intro') }}
      </p>
    </header>

    <div v-if="loading" class="flex justify-center p-10">
      <AppSpinner />
    </div>

    <AppAlert v-else-if="loadError" variant="error">{{ loadError }}</AppAlert>

    <template v-else>
      <section v-if="grouped.builtins.length > 0" class="mb-8">
        <h2 class="mb-3 text-sm font-semibold uppercase tracking-wide text-muted-foreground">
          {{ $t('plugins.browse.builtin') }}
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
                <AppBadge variant="secondary">{{ $t('plugins.badge.builtin') }}</AppBadge>
              </div>
              <p v-if="e.description" class="mt-1 text-xs text-muted-foreground">
                {{ e.description }}
              </p>
              <div class="mt-1 flex flex-wrap items-center gap-x-3 gap-y-1 text-[11px] text-muted-foreground">
                <span>{{ $t('plugins.meta.kinds') }}: {{ e.kinds.join(', ') }}</span>
                <span v-if="e.has_grader">{{ $t('plugins.meta.graded') }}</span>
                <details>
                  <summary class="cursor-pointer">{{ $t('common.advanced.toggle') }}</summary>
                  <span>{{ $t('plugins.meta.contentId') }}: <code class="font-mono">{{ shortCid(e.plugin_cid) }}</code></span>
                </details>
              </div>
            </div>
          </li>
        </ul>
      </section>

      <section>
        <h2 class="mb-3 text-sm font-semibold uppercase tracking-wide text-muted-foreground">
          {{ $t('plugins.browse.community') }}
        </h2>
        <EmptyState
          v-if="grouped.community.length === 0"
          :title="$t('plugins.browse.emptyTitle')"
          :description="$t('plugins.browse.emptyDescription')"
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
              </div>
              <p v-if="e.description" class="mt-1 text-xs text-muted-foreground">
                {{ e.description }}
              </p>
              <div class="mt-1 flex flex-wrap items-center gap-x-3 gap-y-1 text-[11px] text-muted-foreground">
                <span>{{ $t('plugins.meta.author') }}: {{ displayName(e.author_did) }}</span>
                <span>{{ $t('plugins.meta.kinds') }}: {{ e.kinds.join(', ') }}</span>
                <span v-if="e.capabilities.length > 0">
                  {{ $t('plugins.meta.caps') }}: {{ e.capabilities.join(', ') }}
                </span>
                <details>
                  <summary class="cursor-pointer">{{ $t('common.advanced.toggle') }}</summary>
                  <span>{{ $t('plugins.meta.contentId') }}: <code class="font-mono">{{ shortCid(e.plugin_cid) }}</code></span>
                </details>
              </div>
            </div>
          </li>
        </ul>
      </section>
    </template>
  </div>
</template>
