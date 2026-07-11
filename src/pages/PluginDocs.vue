<script setup lang="ts">
/**
 * Full-page plugin documentation viewer. Renders the plugin's bundled
 * README as sanitized Markdown with inlined screenshots. Reached by
 * clicking a plugin card in Settings → Plugins.
 */
import { ref, onMounted, nextTick, computed } from 'vue'
import { useI18n } from 'vue-i18n'
import { useRoute, useRouter } from 'vue-router'
import { useLocalApi } from '@/composables/useLocalApi'
import { useDisplayNames } from '@/composables/useDisplayNames'
import { renderMarkdown } from '@/utils/markdown'
import { AppSpinner, AppButton, AppBadge } from '@/components/ui'
import type { InstalledPlugin, PluginManifest } from '@/types'

const { t } = useI18n()
const route = useRoute()
const router = useRouter()
const { invoke } = useLocalApi()
const { displayName, ensureNames } = useDisplayNames()

const cid = computed(() => String(route.params.cid ?? ''))
const plugin = ref<InstalledPlugin | null>(null)
const manifest = ref<PluginManifest | null>(null)
const thumbnail = ref<string>('')
const html = ref('')
const loading = ref(true)
const docsContainer = ref<HTMLElement | null>(null)

onMounted(async () => {
  try {
    const [list, m] = await Promise.all([
      invoke<InstalledPlugin[]>('plugin_list'),
      invoke<PluginManifest>('plugin_get_manifest', { pluginCid: cid.value }).catch(() => null),
    ])
    plugin.value = list.find((p) => p.plugin_cid === cid.value) ?? null
    manifest.value = m
    if (m?.author_did) void ensureNames([m.author_did])
    if (m?.icon_path) {
      thumbnail.value = await invoke<string>('plugin_read_asset_data_url', {
        pluginCid: cid.value,
        path: m.icon_path,
      }).catch(() => '')
    }
    const text = await invoke<string>('plugin_get_docs', { pluginCid: cid.value })
    html.value = text
      ? renderMarkdown(text)
      : `<p class="text-muted-foreground">${t('plugins.docs.noReadme')}</p>`
    await nextTick()
    await resolveImages()
  } catch (e) {
    html.value = `<p class="text-destructive">${t('plugins.docs.loadError', { error: String(e) })}</p>`
  } finally {
    loading.value = false
  }
})

async function resolveImages() {
  const root = docsContainer.value
  if (!root) return
  const imgs = Array.from(root.querySelectorAll('img[data-rel]')) as HTMLImageElement[]
  await Promise.all(
    imgs.map(async (img) => {
      const rel = img.getAttribute('data-rel')
      if (!rel) return
      try {
        const url = await invoke<string>('plugin_read_asset_data_url', {
          pluginCid: cid.value,
          path: rel,
        })
        if (url) img.src = url
        else img.remove()
      } catch {
        img.remove()
      }
    }),
  )
}

function openDonate() {
  const url = manifest.value?.donate_url
  if (url) window.open(url, '_blank', 'noopener,noreferrer')
}

function monogram(name: string): string {
  return (name.trim()[0] || '?').toUpperCase()
}
function thumbGradient(c: string): string {
  let h = 0
  for (let i = 0; i < c.length; i++) h = (h * 31 + c.charCodeAt(i)) >>> 0
  const a = h % 360
  return `linear-gradient(135deg, hsl(${a} 70% 55%), hsl(${(a + 60) % 360} 70% 45%))`
}
</script>

<template>
  <div class="mx-auto w-full max-w-3xl px-4 py-6 md:px-6 md:py-8">
    <button
      class="mb-5 inline-flex items-center gap-1.5 text-sm text-muted-foreground transition-colors hover:text-foreground"
      @click="router.push('/settings/plugins')"
    >
      <svg class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
        <path stroke-linecap="round" stroke-linejoin="round" d="M15 19l-7-7 7-7" />
      </svg>
      {{ $t('plugins.docs.back') }}
    </button>

    <header class="mb-6 flex items-start gap-4">
      <div
        class="flex h-16 w-16 shrink-0 items-center justify-center overflow-hidden rounded-2xl text-2xl font-bold text-white shadow-sm"
        :style="thumbnail ? undefined : { background: thumbGradient(cid) }"
      >
        <img v-if="thumbnail" :src="thumbnail" :alt="plugin?.name ?? ''" class="h-full w-full object-cover" />
        <span v-else>{{ monogram(plugin?.name ?? manifest?.name ?? '?') }}</span>
      </div>
      <div class="min-w-0">
        <h1 class="text-2xl font-bold text-foreground">
          {{ plugin?.name ?? manifest?.name ?? $t('plugins.docs.fallbackName') }}
        </h1>
        <div class="mt-1.5 flex flex-wrap items-center gap-2">
          <AppBadge v-if="plugin" variant="secondary">v{{ plugin.version }}</AppBadge>
          <AppBadge v-if="plugin" variant="secondary">{{ plugin.source }}</AppBadge>
          <span v-if="manifest?.author_did" class="text-xs text-muted-foreground">
            {{ $t('plugins.docs.by', { name: displayName(manifest.author_did) }) }}
          </span>
        </div>
        <p v-if="manifest?.description" class="mt-2 text-sm text-muted-foreground">
          {{ manifest.description }}
        </p>
        <div v-if="manifest?.donate_url" class="mt-3">
          <AppButton size="sm" variant="outline" @click="openDonate">
            {{ $t('plugins.docs.donate') }}
          </AppButton>
        </div>
      </div>
    </header>

    <div v-if="loading" class="flex justify-center p-10">
      <AppSpinner />
    </div>
    <!-- eslint-disable-next-line vue/no-v-html -->
    <div v-else ref="docsContainer" class="plugin-docs" v-html="html" />
  </div>
</template>

<style scoped>
.plugin-docs :deep(h1),
.plugin-docs :deep(h2),
.plugin-docs :deep(h3) {
  font-weight: 700;
  color: var(--app-foreground);
  margin: 1.2em 0 0.4em;
  line-height: 1.25;
}
.plugin-docs :deep(h1) { font-size: 1.4rem; }
.plugin-docs :deep(h2) { font-size: 1.2rem; }
.plugin-docs :deep(h3) { font-size: 1.05rem; }
.plugin-docs :deep(p) {
  margin: 0.65em 0;
  font-size: 0.9rem;
  line-height: 1.65;
  color: color-mix(in srgb, var(--app-foreground) 88%, transparent);
}
.plugin-docs :deep(ul),
.plugin-docs :deep(ol) {
  margin: 0.65em 0;
  padding-left: 1.4em;
  font-size: 0.9rem;
  line-height: 1.65;
}
.plugin-docs :deep(li) { margin: 0.2em 0; }
.plugin-docs :deep(a) { color: var(--app-primary); text-decoration: underline; }
.plugin-docs :deep(code) {
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  font-size: 0.8em;
  background: color-mix(in srgb, var(--app-foreground) 8%, transparent);
  padding: 0.1em 0.35em;
  border-radius: 0.3em;
}
.plugin-docs :deep(pre) {
  background: color-mix(in srgb, var(--app-foreground) 7%, transparent);
  border: 1px solid var(--app-border);
  border-radius: 0.6rem;
  padding: 0.85rem 1rem;
  overflow-x: auto;
  margin: 0.85em 0;
}
.plugin-docs :deep(pre code) { background: none; padding: 0; font-size: 0.8rem; line-height: 1.5; }
.plugin-docs :deep(blockquote) {
  border-left: 3px solid var(--app-border);
  padding-left: 0.9em;
  margin: 0.7em 0;
  color: var(--app-muted-foreground);
}
.plugin-docs :deep(hr) { border: 0; border-top: 1px solid var(--app-border); margin: 1.3em 0; }
.plugin-docs :deep(img) {
  display: block;
  max-width: 100%;
  height: auto;
  border-radius: 0.6rem;
  border: 1px solid var(--app-border);
  margin: 1em 0;
}
</style>
