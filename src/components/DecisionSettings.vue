<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { useLocalApi } from '@/composables/useLocalApi'
import { onProfileLocked } from '@/composables/useProfiles'
import { AppButton, AppInput } from '@/components/ui'
import type { DecisionSettings, DecisionTask, StudioDocument } from '@/types'

const { t } = useI18n()
const { invoke } = useLocalApi()
const document = ref<StudioDocument<DecisionSettings> | null>(null)
const key = ref('')
const error = ref('')
const notice = ref('')
const busy = ref(false)
let epoch = 0
const tasks: { value: DecisionTask; label: string }[] = [
  { value: 'job_description', label: 'decisionJob' }, { value: 'learning_goal', label: 'decisionGoal' },
  { value: 'document_claim', label: 'decisionDocument' }, { value: 'studio_review', label: 'decisionStudio' },
  { value: 'search', label: 'decisionSearch' }, { value: 'tutor', label: 'decisionTutor' },
]
const remove = onProfileLocked(() => { epoch++; document.value = null; key.value = ''; error.value = ''; notice.value = '' })
onBeforeUnmount(() => { epoch++; remove() })
onMounted(async () => {
  const generation = epoch
  try {
    const next = await invoke<StudioDocument<DecisionSettings>>('decision_settings')
    if (generation === epoch) document.value = next
  } catch (e) { if (generation === epoch) error.value = String(e) }
})
async function exportShadow() {
  const generation = epoch
  busy.value = true; error.value = ''; notice.value = ''
  try {
    const samples = await invoke<unknown[]>('decision_export_shadow')
    if (generation !== epoch) return
    const url = URL.createObjectURL(new Blob([JSON.stringify({ schema_version: 1, independently_labelled: false, samples }, null, 2)], { type: 'application/json' }))
    const anchor = window.document.createElement('a')
    anchor.href = url; anchor.download = 'learning-shadow.json'; anchor.click()
    URL.revokeObjectURL(url)
  } catch (e) { if (generation === epoch) error.value = String(e) }
  finally { if (generation === epoch) busy.value = false }
}
async function clearShadow() {
  const generation = epoch
  busy.value = true; error.value = ''; notice.value = ''
  try {
    await invoke('decision_clear_shadow')
    const next = await invoke<StudioDocument<DecisionSettings>>('decision_settings')
    if (generation === epoch) { document.value = next; notice.value = t('instructor.studio.decisionShadowCleared') }
  } catch (e) { if (generation === epoch) error.value = String(e) }
  finally { if (generation === epoch) busy.value = false }
}
async function save(clearKey = false) {
  if (!document.value) return
  const generation = epoch
  busy.value = true; error.value = ''
  try {
    const next = await invoke<StudioDocument<DecisionSettings>>('decision_save_settings', { document: document.value, apiKey: clearKey ? '' : key.value || null })
    if (generation === epoch) { document.value = next; key.value = '' }
  } catch (e) { if (generation === epoch) error.value = String(e) }
  finally { if (generation === epoch) busy.value = false }
}
</script>

<template>
  <section class="space-y-4 rounded-xl border border-border bg-card p-5">
    <h2 class="font-semibold">{{ t('instructor.studio.decisionTitle') }}</h2>
    <p class="text-sm text-muted-foreground">{{ t('instructor.studio.decisionDisclosure') }}</p>
    <template v-if="document">
      <label class="flex items-center gap-2 text-sm"><input v-model="document.value.cloud_allowed" type="checkbox" />{{ t('instructor.studio.decisionCloud') }}</label>
      <label class="block text-sm">{{ t('instructor.studio.decisionMode') }}
        <select v-model="document.value.mode" class="ml-3 rounded border border-border bg-background p-2">
          <option value="off">{{ t('instructor.studio.decisionOff') }}</option><option value="shadow">{{ t('instructor.studio.decisionShadow') }}</option><option value="assist">{{ t('instructor.studio.decisionAssist') }}</option>
        </select>
      </label>
      <fieldset class="flex flex-wrap gap-4"><legend class="mb-2 text-sm">{{ t('instructor.studio.decisionTasks') }}</legend>
        <label v-for="task in tasks" :key="task.value" class="flex items-center gap-2 text-sm"><input v-model="document.value.tasks" type="checkbox" :value="task.value" />{{ t(`instructor.studio.${task.label}`) }}</label>
      </fieldset>
      <label class="flex items-start gap-2 text-sm"><input v-model="document.value.retain_learning_shadow" type="checkbox" />{{ t('instructor.studio.decisionRetainShadow') }}</label>
      <p class="text-xs text-muted-foreground">{{ t('instructor.studio.decisionShadowRetention') }}</p>
      <div class="flex flex-wrap gap-2"><AppButton variant="ghost" :disabled="busy" @click="exportShadow">{{ t('instructor.studio.decisionExportShadow') }}</AppButton><AppButton variant="ghost" :disabled="busy" @click="clearShadow">{{ t('instructor.studio.decisionClearShadow') }}</AppButton></div>
      <AppInput v-model="key" type="password" autocomplete="new-password" :label="t('instructor.studio.decisionKey')" />
      <div class="flex flex-wrap gap-2"><AppButton :loading="busy" @click="save()">{{ t('instructor.studio.decisionSave') }}</AppButton><AppButton variant="ghost" :disabled="busy" @click="save(true)">{{ t('instructor.studio.decisionClearKey') }}</AppButton></div>
    </template>
    <p v-if="notice" role="status" class="text-sm">{{ notice }}</p>
    <p v-if="error" class="text-sm text-error" role="alert">{{ error }}</p>
  </section>
</template>
