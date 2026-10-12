<script setup lang="ts">
// The developer override for which Alexandria Cloud the app talks to, so a
// local Cloud can be tried without digging through All settings. Saved on
// blur or Enter rather than per keystroke: every save re-points the app.
import { computed, onMounted, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useSettings } from '@/composables/useSettings'
import { AppButton } from '@/components/ui'

const KEY = 'organisations.cloud_origin'

const { t } = useI18n()
const { byKey, initialize, setSetting, resetSetting } = useSettings()
const value = ref('')
const message = ref('')

const stored = computed(() => byKey.value.get(KEY)?.current_value ?? '')
watch(stored, (v) => { value.value = v }, { immediate: true })

async function save() {
  if (value.value.trim() === stored.value) return
  message.value = ''
  try {
    if (value.value.trim()) await setSetting(KEY, value.value.trim())
    else await resetSetting(KEY)
    message.value = t('organisations.settings.cloudSaved')
  } catch (e) {
    message.value = String(e)
  }
}

onMounted(() => { void initialize().catch(() => {}) })
</script>

<template>
  <div class="shrink-0 space-y-2 rounded-xl border border-border bg-card p-4">
    <label for="cloud-origin" class="block text-sm font-medium text-foreground">{{ t('organisations.settings.cloudLabel') }}</label>
    <p class="text-xs text-muted-foreground">{{ t('organisations.settings.cloudDesc') }}</p>
    <div class="flex flex-wrap items-center gap-2">
      <input
        id="cloud-origin"
        v-model="value"
        type="url"
        spellcheck="false"
        class="input min-w-0 flex-1 font-mono"
        placeholder="http://127.0.0.1:8787"
        @blur="save"
        @keydown.enter="save"
      >
      <AppButton size="sm" variant="outline" @click="save">{{ t('common.actions.save') }}</AppButton>
    </div>
    <p v-if="message" role="status" class="text-xs text-muted-foreground">{{ message }}</p>
  </div>
</template>
