<script setup lang="ts">
// Plugin element editor: bind an installed plugin (e.g. codejudge) and
// author its per-element config, stored as a blob → plugin_config_cid.
import { computed, onMounted, ref } from 'vue'
import { useLocalApi } from '@/composables/useLocalApi'
import { AppBadge, AppButton } from '@/components/ui'
import type { Element, InstalledPlugin } from '@/types'

const props = defineProps<{ element: Element }>()
const emit = defineEmits<{ updated: [Element] }>()

const { invoke } = useLocalApi()

const plugins = ref<InstalledPlugin[]>([])
const selectedCid = ref(props.element.plugin_cid ?? '')
const configJson = ref('')
const configLoaded = ref(false)
const dirty = ref(false)
const saving = ref(false)
const error = ref('')

const enabledPlugins = computed(() => plugins.value.filter(p => p.enabled))
const selectedPlugin = computed(() => plugins.value.find(p => p.plugin_cid === selectedCid.value) ?? null)

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
      const bytes = await invoke<number[]>('content_get', { hash: props.element.plugin_config_cid })
      configJson.value = new TextDecoder().decode(new Uint8Array(bytes))
    } catch { /* config stays blank; saving writes a fresh blob */ }
  }
  configLoaded.value = true
})

async function save() {
  if (!selectedCid.value) {
    error.value = 'Pick a plugin first.'
    return
  }
  if (!configValid.value) {
    error.value = 'Fix the config JSON before saving.'
    return
  }
  saving.value = true
  error.value = ''
  try {
    let configCid: string | null = props.element.plugin_config_cid ?? null
    if (configJson.value.trim()) {
      const bytes = Array.from(new TextEncoder().encode(configJson.value))
      const result = await invoke<{ hash: string }>('content_add', { data: bytes })
      configCid = result.hash
    }
    const updated = await invoke<Element>('update_element', {
      elementId: props.element.id,
      req: {
        plugin_cid: selectedCid.value,
        plugin_version: selectedPlugin.value?.version ?? null,
        plugin_config_cid: configCid,
      },
    })
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
      <h3 class="text-sm font-semibold text-foreground">Plugin binding</h3>
      <AppButton v-if="dirty" size="xs" :loading="saving" @click="save">Save</AppButton>
    </div>

    <div>
      <label class="mb-1 block text-xs font-medium text-muted-foreground">Installed plugin</label>
      <select
        v-model="selectedCid"
        class="w-full rounded-md border border-border bg-background px-3 py-2 text-sm"
        @change="dirty = true"
      >
        <option value="">Pick a plugin…</option>
        <option v-for="p in enabledPlugins" :key="p.plugin_cid" :value="p.plugin_cid">
          {{ p.name }} v{{ p.version }}
        </option>
      </select>
      <p v-if="!enabledPlugins.length" class="mt-1 text-xs text-warning">
        No enabled plugins installed — add one under Settings → Plugins.
      </p>
    </div>

    <div v-if="selectedPlugin" class="flex items-center gap-2 text-xs text-muted-foreground">
      <AppBadge size="xs">{{ selectedPlugin.source }}</AppBadge>
      <code class="truncate">{{ selectedPlugin.plugin_cid.slice(0, 24) }}…</code>
    </div>

    <div>
      <label class="mb-1 block text-xs font-medium text-muted-foreground">
        Element config (JSON passed to the plugin at init — e.g. a codejudge problem definition)
      </label>
      <textarea
        v-model="configJson"
        rows="14"
        class="w-full rounded-md border bg-background p-3 font-mono text-xs"
        :class="configValid ? 'border-border' : 'border-error'"
        :placeholder="configLoaded ? '{ }' : 'Loading…'"
        @input="dirty = true"
      />
      <p v-if="!configValid" class="text-xs text-error">Invalid JSON.</p>
    </div>

    <p v-if="error" class="text-sm text-error">{{ error }}</p>
  </div>
</template>
