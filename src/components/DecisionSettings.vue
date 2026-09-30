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
const busy = ref(false)
let epoch = 0
const tasks: { value: DecisionTask; label: string }[] = [
  { value: 'job_description', label: 'decisionJob' }, { value: 'learning_goal', label: 'decisionGoal' },
  { value: 'document_claim', label: 'decisionDocument' }, { value: 'studio_review', label: 'decisionStudio' },
  { value: 'search', label: 'decisionSearch' }, { value: 'tutor', label: 'decisionTutor' },
]
const remove = onProfileLocked(() => { epoch++; document.value = null; key.value = ''; error.value = '' })
onBeforeUnmount(() => { epoch++; remove() })
onMounted(async () => {
  const generation = epoch
  try {
    const next = await invoke<StudioDocument<DecisionSettings>>('decision_settings')
    if (generation === epoch) document.value = next
  } catch (e) { if (generation === epoch) error.value = String(e) }
})
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
      <AppInput v-model="key" type="password" autocomplete="new-password" :label="t('instructor.studio.decisionKey')" />
      <div class="flex flex-wrap gap-2"><AppButton :loading="busy" @click="save()">{{ t('instructor.studio.decisionSave') }}</AppButton><AppButton variant="ghost" :disabled="busy" @click="save(true)">{{ t('instructor.studio.decisionClearKey') }}</AppButton></div>
    </template>
    <p v-if="error" class="text-sm text-error" role="alert">{{ error }}</p>
  </section>
</template>
