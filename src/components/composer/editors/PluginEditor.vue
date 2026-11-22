<script setup lang="ts">
// Plugin element editor: bind an installed plugin (e.g. a code editor) and
// author its per-element config, stored as a blob → plugin_config_cid.
import { computed, onMounted, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useLocalApi } from '@/composables/useLocalApi'
import { AppBadge, AppButton } from '@/components/ui'
import PluginHost from '@/components/plugin/PluginHost.vue'
import type { Element, InstalledPlugin, PluginManifest } from '@/types'

const props = defineProps<{ element: Element }>()
const emit = defineEmits<{ updated: [Element]; dirty: [boolean] }>()

const { invoke } = useLocalApi()
const { t } = useI18n()

const plugins = ref<InstalledPlugin[]>([])
const selectedCid = ref(props.element.plugin_cid ?? '')
const configJson = ref('')
let savedConfigJson = ''
const configLoaded = ref(false)
const dirty = ref(false)
watch(dirty, value => emit('dirty', value), { flush: 'sync' })
const saving = ref(false)
const error = ref('')

const enabledPlugins = computed(() => plugins.value.filter(p => p.enabled))
const selectedPlugin = computed(() => plugins.value.find(p => p.plugin_cid === selectedCid.value) ?? null)

const visualEditor = computed(() => {
  const raw = selectedPlugin.value?.manifest_json
  if (!raw) return false
  try { return (JSON.parse(raw) as PluginManifest).authoring === true } catch { return false }
})
const authorJson = ref(props.element.content_inline ?? '')
function stageConfig(json: string) {
  authorJson.value = json
  dirty.value = true
}
function changePlugin() {
  configJson.value = selectedCid.value === props.element.plugin_cid ? savedConfigJson : ''
  authorJson.value = selectedCid.value === props.element.plugin_cid ? props.element.content_inline ?? '' : ''
  dirty.value = true
}

const configValid = computed(() => {
  if (!configJson.value.trim()) return true
  try {
    JSON.parse(configJson.value)
    return true
  } catch {
    return false
  }
})

onMounted(async () => {
  plugins.value = await invoke<InstalledPlugin[]>('plugin_list').catch(() => [])
  // Load existing config blob for display, best-effort.
  if (props.element.plugin_config_cid) {
    try {
      configJson.value = await invoke<string>('content_get_text', {
        hash: props.element.plugin_config_cid,
      })
    } catch { /* config stays blank; saving writes a fresh blob */ }
  }
  savedConfigJson = configJson.value
  configLoaded.value = true
})

async function save() {
  if (!selectedCid.value) {
    error.value = t('instructor.editors.plugin.errPickFirst')
    return
  }
  if (!visualEditor.value && !configValid.value) {
    error.value = t('instructor.editors.plugin.errFixConfig')
    return
  }
  saving.value = true
  error.value = ''
  try {
    let configCid: string | null = selectedCid.value === props.element.plugin_cid ? props.element.plugin_config_cid ?? null : null
    if (!visualEditor.value && configJson.value.trim()) {
      const bytes = Array.from(new TextEncoder().encode(configJson.value))
      const result = await invoke<{ hash: string }>('content_add', { data: bytes })
      configCid = result.hash
    }
    const updated = await invoke<Element>('update_element', {
      elementId: props.element.id,
      req: {
        plugin_cid: selectedCid.value,
        plugin_version: selectedPlugin.value?.version ?? null,
        ...(visualEditor.value
          ? { content_inline: authorJson.value || '{}' }
          : { plugin_config_cid: configCid }),
      },
    })
    savedConfigJson = configJson.value
    emit('updated', updated)
    dirty.value = false
  } catch (e) {
    error.value = String(e)
  } finally {
    saving.value = false
  }
}
</script>

<template>
  <div class="space-y-4">
    <div class="flex items-center justify-between">
      <h3 class="text-sm font-semibold text-foreground">{{ $t('instructor.editors.plugin.heading') }}</h3>
      <AppButton v-if="dirty" size="xs" :loading="saving" @click="save">{{ $t('common.actions.save') }}</AppButton>
    </div>

    <div>
      <label class="mb-1 block text-xs font-medium text-muted-foreground">{{ $t('instructor.editors.plugin.installedLabel') }}</label>
      <select
        v-model="selectedCid"
        :disabled="saving || !configLoaded"
        class="w-full rounded-md border border-border bg-background px-3 py-2 text-sm"
        @change="changePlugin"
      >
        <option value="">{{ $t('instructor.editors.plugin.pickOption') }}</option>
        <option v-for="p in enabledPlugins" :key="p.plugin_cid" :value="p.plugin_cid">
          {{ p.name }} v{{ p.version }}
        </option>
      </select>
      <p v-if="!enabledPlugins.length" class="mt-1 text-xs text-warning">
        {{ $t('instructor.editors.plugin.noneInstalled') }}
      </p>
    </div>

    <div v-if="selectedPlugin" class="flex items-center gap-2 text-xs text-muted-foreground">
      <AppBadge size="xs">{{ selectedPlugin.source }}</AppBadge>
      <code class="truncate">{{ selectedPlugin.plugin_cid.slice(0, 24) }}…</code>
    </div>

    <PluginHost
      v-if="visualEditor && configLoaded"
      :key="selectedCid"
      class="min-h-128"
      :element="{ ...element, plugin_cid: selectedCid, content_inline: authorJson || null, content_cid: selectedCid === element.plugin_cid ? element.content_cid : null }"
      mode="author"
      @author-config="stageConfig"
    />
    <div v-else>
      <label class="mb-1 block text-xs font-medium text-muted-foreground">
        {{ $t('instructor.editors.plugin.configLabel') }}
      </label>
      <textarea
        v-model="configJson"
        rows="14"
        class="w-full rounded-md border bg-background p-3 font-mono text-xs"
        :class="configValid ? 'border-border' : 'border-error'"
        :placeholder="configLoaded ? '{ }' : $t('common.actions.loading')"
        @input="dirty = true"
      />
      <p v-if="!configValid" class="text-xs text-error">{{ $t('instructor.editors.plugin.invalidJson') }}</p>
    </div>

    <p v-if="error" class="text-sm text-error">{{ error }}</p>
  </div>
</template>
