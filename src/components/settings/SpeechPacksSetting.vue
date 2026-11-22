<script setup lang="ts">
/**
 * Download and remove pronunciation-check language packs.
 *
 * Self-contained (no props): plugin settings of type `speech_packs` render it
 * as-is. Packs are device-wide, shared by every profile. Languages on the
 * multilingual model share one download, so its size is shown once for the
 * group and adding a second language costs nothing.
 */
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { useLocalApi } from '@/composables/useLocalApi'
import { AppAlert, AppBadge, AppButton, AppSpinner } from '@/components/ui'
import type { SpeechPack, SpeechPackProgress } from '@/types'

const { t, locale } = useI18n()
const { invoke } = useLocalApi()

const packs = ref<SpeechPack[]>([])
const loading = ref(true)
const loadError = ref<string | null>(null)
/** Pack ids with an install or removal in flight. */
const busy = ref<Set<string>>(new Set())
const progress = ref<Record<string, { downloaded: number; total: number }>>({})
const errors = ref<Record<string, string>>({})

let unlisten: UnlistenFn | null = null
let unmounted = false

type ListFormatConstructor = new (
  locale: string,
  options: { type: 'conjunction' },
) => { format(list: string[]): string }

function isProgress(value: unknown): value is SpeechPackProgress {
  if (!value || typeof value !== 'object') return false
  const v = value as Record<string, unknown>
  return typeof v.lang === 'string' && typeof v.downloaded === 'number' && typeof v.total === 'number'
}

async function load() {
  try {
    packs.value = await invoke<SpeechPack[]>('speech_packs_list')
    loadError.value = null
  } catch (e) {
    loadError.value = String(e)
  } finally {
    loading.value = false
  }
}

onMounted(async () => {
  const stop = await listen<unknown>('speech-pack-progress', (event) => {
    if (!isProgress(event.payload)) return
    const { lang, downloaded, total } = event.payload
    progress.value = { ...progress.value, [lang]: { downloaded, total } }
  })
  if (unmounted) stop()
  else unlisten = stop
  await load()
})

onBeforeUnmount(() => {
  unmounted = true
  unlisten?.()
})

const displayNames = computed(() => {
  try {
    return new Intl.DisplayNames([locale.value], { type: 'language' })
  } catch {
    return null
  }
})

function packName(pack: SpeechPack): string {
  if (pack.upgrades) return t('speech.packs.enhancedName')
  return displayNames.value?.of(pack.lang) ?? pack.name
}

function formatBytes(bytes: number): string {
  const mb = bytes / (1024 * 1024)
  const value = new Intl.NumberFormat(locale.value, { maximumFractionDigits: mb < 100 ? 1 : 0 }).format(mb)
  return t('speech.packs.megabytes', { value })
}

/** Packs grouped by model, the English packs first. */
const groups = computed(() => {
  const byModel = new Map<string, SpeechPack[]>()
  for (const pack of packs.value) {
    const list = byModel.get(pack.model_id) ?? []
    list.push(pack)
    byModel.set(pack.model_id, list)
  }
  const singles: SpeechPack[] = []
  const shared: SpeechPack[][] = []
  for (const list of byModel.values()) {
    if (list.length === 1 && list[0]) singles.push(list[0])
    else shared.push(list)
  }
  return { singles, shared }
})

function sharedGroupSize(list: SpeechPack[]): number {
  return list[0]?.size_bytes ?? 0
}

function sharedModelPresent(list: SpeechPack[]): boolean {
  return list.some((pack) => pack.download_bytes === 0)
}

function modelBusy(pack: SpeechPack): boolean {
  return packs.value.some((other) => other.model_id === pack.model_id && busy.value.has(other.lang))
}

function percent(lang: string): number | null {
  const p = progress.value[lang]
  if (!p || p.total <= 0) return null
  return Math.min(100, Math.round((p.downloaded / p.total) * 100))
}

function without<T>(record: Record<string, T>, key: string): Record<string, T> {
  const copy = { ...record }
  delete copy[key]
  return copy
}

function setBusy(lang: string, on: boolean) {
  const next = new Set(busy.value)
  if (on) next.add(lang)
  else next.delete(lang)
  busy.value = next
}

async function run(pack: SpeechPack, action: () => Promise<SpeechPack[]>) {
  setBusy(pack.lang, true)
  errors.value = without(errors.value, pack.lang)
  try {
    packs.value = await action()
  } catch (e) {
    errors.value = { ...errors.value, [pack.lang]: String(e) }
    await load()
  } finally {
    setBusy(pack.lang, false)
    progress.value = without(progress.value, pack.lang)
  }
}

function install(pack: SpeechPack) {
  void run(pack, () => invoke<SpeechPack[]>('speech_pack_install', { lang: pack.lang }))
}

function remove(pack: SpeechPack) {
  void run(pack, () => invoke<SpeechPack[]>('speech_pack_remove', { lang: pack.lang }))
}

function sharedWithNames(pack: SpeechPack): string {
  const names = pack.shared_with
    .map((code) => packs.value.find((p) => p.lang === code))
    .filter((p): p is SpeechPack => p !== undefined)
    .map(packName)
  // `Intl.ListFormat` is newer than the TS lib target; use it when present.
  const ListFormat = (Intl as unknown as { ListFormat?: ListFormatConstructor }).ListFormat
  return ListFormat ? new ListFormat(locale.value, { type: 'conjunction' }).format(names) : names.join(', ')
}
</script>

<template>
  <section class="space-y-4" :aria-label="t('speech.packs.title')">
    <div class="space-y-1">
      <h4 class="text-sm font-semibold text-foreground">{{ t('speech.packs.title') }}</h4>
      <p class="text-sm text-muted-foreground">{{ t('speech.packs.intro') }}</p>
      <p class="text-xs text-muted-foreground">{{ t('speech.packs.honesty') }}</p>
    </div>

    <div v-if="loading" class="flex justify-center py-4">
      <AppSpinner />
    </div>
    <AppAlert v-else-if="loadError" variant="error">
      {{ t('speech.packs.loadError') }}
    </AppAlert>

    <template v-else>
      <ul class="speech-list">
        <li v-for="pack in groups.singles" :key="pack.lang" class="speech-row">
          <div class="min-w-0 flex-1">
            <div class="flex flex-wrap items-center gap-2">
              <span class="font-medium text-foreground">{{ packName(pack) }}</span>
              <span v-if="!pack.upgrades && pack.native_name !== packName(pack)" class="text-muted-foreground" :lang="pack.lang">
                {{ pack.native_name }}
              </span>
              <AppBadge v-if="pack.installed" variant="success">{{ t('speech.packs.installed') }}</AppBadge>
            </div>
            <p class="text-xs text-muted-foreground">
              <template v-if="pack.upgrades">{{ t('speech.packs.enhancedHint') }} · </template>
              {{ formatBytes(pack.size_bytes) }}
            </p>
            <div v-if="percent(pack.lang) !== null" class="speech-progress" role="progressbar" :aria-valuenow="percent(pack.lang) ?? 0" aria-valuemin="0" aria-valuemax="100" :aria-label="t('speech.packs.downloading', { percent: percent(pack.lang) ?? 0 })">
              <div class="speech-progress__bar" :style="{ width: `${percent(pack.lang) ?? 0}%` }" />
            </div>
            <p v-if="errors[pack.lang]" class="text-xs text-destructive">
              {{ t('speech.packs.error', { error: errors[pack.lang] }) }}
            </p>
          </div>
          <AppButton
            v-if="pack.installed"
            variant="outline"
            size="sm"
            :loading="busy.has(pack.lang)"
            :disabled="modelBusy(pack)"
            @click="remove(pack)"
          >
            {{ t('speech.packs.remove') }}
          </AppButton>
          <AppButton
            v-else
            variant="primary"
            size="sm"
            :loading="busy.has(pack.lang)"
            :disabled="modelBusy(pack)"
            @click="install(pack)"
          >
            {{ t('speech.packs.download') }}
          </AppButton>
        </li>
      </ul>

      <div v-for="list in groups.shared" :key="list[0]?.model_id" class="space-y-2">
        <div>
          <h5 class="text-sm font-semibold text-foreground">{{ t('speech.packs.sharedTitle') }}</h5>
          <p class="text-xs text-muted-foreground">
            {{
              sharedModelPresent(list)
                ? t('speech.packs.sharedPresent', { size: formatBytes(sharedGroupSize(list)) })
                : t('speech.packs.sharedMissing', { size: formatBytes(sharedGroupSize(list)) })
            }}
          </p>
        </div>
        <ul class="speech-list">
          <li v-for="pack in list" :key="pack.lang" class="speech-row">
            <div class="min-w-0 flex-1">
              <div class="flex flex-wrap items-center gap-2">
                <span class="font-medium text-foreground">{{ packName(pack) }}</span>
                <span v-if="pack.native_name !== packName(pack)" class="text-muted-foreground" :lang="pack.lang">
                  {{ pack.native_name }}
                </span>
                <AppBadge v-if="pack.installed" variant="success">{{ t('speech.packs.installed') }}</AppBadge>
                <AppBadge v-if="pack.limited_accuracy" variant="warning">{{ t('speech.packs.limited') }}</AppBadge>
              </div>
              <p v-if="!pack.installed && pack.download_bytes > 0" class="text-xs text-muted-foreground">
                {{ t('speech.packs.sharedWith', { size: formatBytes(pack.download_bytes), languages: sharedWithNames(pack) }) }}
              </p>
              <div v-if="percent(pack.lang) !== null" class="speech-progress" role="progressbar" :aria-valuenow="percent(pack.lang) ?? 0" aria-valuemin="0" aria-valuemax="100" :aria-label="t('speech.packs.downloading', { percent: percent(pack.lang) ?? 0 })">
                <div class="speech-progress__bar" :style="{ width: `${percent(pack.lang) ?? 0}%` }" />
              </div>
              <p v-if="errors[pack.lang]" class="text-xs text-destructive">
                {{ t('speech.packs.error', { error: errors[pack.lang] }) }}
              </p>
            </div>
            <AppButton
              v-if="pack.installed"
              variant="outline"
              size="sm"
              :loading="busy.has(pack.lang)"
              :disabled="modelBusy(pack)"
              @click="remove(pack)"
            >
              {{ t('speech.packs.remove') }}
            </AppButton>
            <AppButton
              v-else
              variant="primary"
              size="sm"
              :loading="busy.has(pack.lang)"
              :disabled="modelBusy(pack)"
              @click="install(pack)"
            >
              {{ pack.download_bytes === 0 ? t('speech.packs.add') : t('speech.packs.download') }}
            </AppButton>
          </li>
        </ul>
      </div>
    </template>
  </section>
</template>

<style scoped>
.speech-list {
  display: flex;
  flex-direction: column;
  border: 1px solid var(--color-border);
  border-radius: var(--radius-lg);
}

.speech-row {
  display: flex;
  align-items: center;
  gap: 0.75rem;
  padding: 0.75rem 1rem;
  font-size: 0.875rem;
}

.speech-row + .speech-row {
  border-top: 1px solid var(--color-border);
}

.speech-progress {
  margin-top: 0.375rem;
  height: 0.25rem;
  border-radius: 9999px;
  background: var(--color-muted);
  overflow: hidden;
}

.speech-progress__bar {
  height: 100%;
  background: var(--color-primary);
}
</style>
