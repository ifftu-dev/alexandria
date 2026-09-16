<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { onBeforeRouteLeave } from 'vue-router'
import { useI18n } from 'vue-i18n'
import { useStudio } from '@/composables/useStudio'
import { useLocalApi } from '@/composables/useLocalApi'
import { AppButton, AppInput, AppTextarea, AppTabs, ConfirmDialog } from '@/components/ui'
import StudioRunPanel from '@/components/composer/StudioRunPanel.vue'
import type { StudioDocument, StudioRun, StudioWorkflow } from '@/types'

const { t } = useI18n()
const { invoke } = useLocalApi()
const studio = useStudio()
const tab = ref('library')
const draft = ref<StudioDocument<StudioWorkflow> | null>(null)
const baseline = ref('')
const dirty = computed(() => draft.value !== null && JSON.stringify(draft.value) !== baseline.value)
const selected = ref(0)
const step = computed(() => draft.value?.value.steps[selected.value])
const runs = ref<StudioDocument<StudioRun>[]>([])
const activeRun = ref<string | null>(null)
const error = ref('')
const saving = ref(false)
const leave = ref(false)
let leaveResolve: ((allow: boolean) => void) | null = null
function confirmLeave() {
  if (!dirty.value) return true
  leave.value = true
  return new Promise<boolean>(resolve => { leaveResolve = resolve })
}
onBeforeRouteLeave(confirmLeave)
async function back() { if (await confirmLeave()) draft.value = null }
function answerLeave(allow: boolean) { leave.value = false; leaveResolve?.(allow); leaveResolve = null }
async function load() {
  try { await studio.refresh(); runs.value = await invoke<StudioDocument<StudioRun>[]>('studio_list_runs') } catch (e) { error.value = String(e) }
}
onMounted(load)
function edit(item?: StudioDocument<StudioWorkflow>) {
  const id = crypto.randomUUID()
  draft.value = item ? structuredClone(item) : { id, revision: 0, value: { id, name: t('instructor.studio.newWorkflow'), initial_prompt: '', steps: ['plan', 'draft', 'review', 'content'].map(role => ({ id: crypto.randomUUID(), role, connection_id: null, initial_prompt: '' })) } }
  baseline.value = item ? JSON.stringify(draft.value) : ''
  selected.value = 0
}
function move(offset: number) {
  if (!draft.value) return
  const to = selected.value + offset
  if (to < 0 || to >= draft.value.value.steps.length) return
  const [item] = draft.value.value.steps.splice(selected.value, 1)
  if (item) draft.value.value.steps.splice(to, 0, item)
  selected.value = to
}
async function save() {
  if (!draft.value) return
  saving.value = true; error.value = ''
  try {
    draft.value = await invoke<StudioDocument<StudioWorkflow>>('studio_save_workflow', { document: draft.value })
    baseline.value = JSON.stringify(draft.value)
    await studio.refresh()
  } catch (e) { error.value = String(e) } finally { saving.value = false }
}
function addStep() { draft.value?.value.steps.push({ id: crypto.randomUUID(), role: 'draft', connection_id: null, initial_prompt: '' }); selected.value = (draft.value?.value.steps.length ?? 1) - 1 }
function removeStep() { draft.value?.value.steps.splice(selected.value, 1); selected.value = Math.max(0, selected.value - 1) }
</script>

<template>
  <div class="mx-auto max-w-5xl space-y-6">
    <header class="flex flex-wrap items-center justify-between gap-4"><div><h1 class="text-2xl font-bold">{{ t('instructor.studio.workflows') }}</h1><p class="mt-2 text-sm text-muted-foreground">{{ t('instructor.studio.workflowIntro') }}</p></div><AppButton type="button" v-if="!draft" @click="edit()">{{ t('instructor.studio.newWorkflow') }}</AppButton></header>
    <p v-if="error" role="alert" class="text-sm text-error">{{ error }}</p>
    <template v-if="draft">
      <form class="space-y-5" @submit.prevent="save">
        <div class="rounded-xl border border-border bg-card p-5 space-y-4"><AppInput v-model="draft.value.name" :label="t('instructor.studio.workflowName')" :maxlength="200" required /><AppTextarea v-model="draft.value.initial_prompt" :label="t('instructor.studio.workflowPrompt')" :rows="3" :maxlength="16000" /><p class="text-xs text-muted-foreground">{{ t('instructor.studio.workflowPromptHelp') }}</p></div>
        <div class="grid gap-5 md:grid-cols-[minmax(200px,0.8fr)_minmax(0,1.2fr)]">
          <div class="space-y-3"><ol class="space-y-3"><li v-for="(item, i) in draft.value.steps" :key="item.id"><button type="button" class="flex w-full items-center gap-3 rounded-xl border bg-card p-4 text-start text-sm" :class="selected === i ? 'border-primary ring-2 ring-primary/10' : 'border-border'" :aria-current="selected === i ? 'step' : undefined" @click="selected = i"><span class="text-muted-foreground">{{ i + 1 }}</span><span>{{ t(`instructor.studio.roleNames.${item.role}`) }}</span></button></li></ol><AppButton type="button" variant="secondary" :disabled="draft.value.steps.length >= 12" @click="addStep">{{ t('instructor.studio.addStep') }}</AppButton><p class="rounded-lg bg-muted/50 p-3 text-xs text-muted-foreground">{{ t('instructor.studio.approvalCheckpoint') }}</p></div>
          <section v-if="step" class="rounded-xl border border-border bg-card p-5 space-y-4">
            <h2 class="font-semibold">{{ t('instructor.studio.stepSettings', { number: selected + 1 }) }}</h2>
            <label class="block text-sm"><span class="mb-2 block">{{ t('instructor.studio.role') }}</span><select v-model="step.role" class="w-full rounded-lg border border-border bg-background p-2.5"><option v-for="role in ['plan', 'draft', 'review', 'content', 'image', 'audio', 'video']" :key="role" :value="role">{{ t(`instructor.studio.roleNames.${role}`) }}</option></select></label>
            <label class="block text-sm"><span class="mb-2 block">{{ t('instructor.studio.modelOverride') }}</span><select v-model="step.connection_id" class="w-full rounded-lg border border-border bg-background p-2.5"><option :value="null">{{ t('instructor.studio.inheritModel') }}</option><option v-for="item in studio.connections.value" :key="item.id" :value="item.id">{{ item.value.name }}</option></select></label>
            <AppTextarea v-model="step.initial_prompt" :label="t('instructor.studio.stepPrompt')" :rows="7" :maxlength="16000" />
            <p class="text-xs text-muted-foreground">{{ t('instructor.studio.stepPromptHelp') }}</p>
            <div class="flex flex-wrap gap-2"><AppButton type="button" variant="secondary" size="sm" :disabled="selected === 0" @click="move(-1)">{{ t('instructor.studio.moveUp') }}</AppButton><AppButton type="button" variant="secondary" size="sm" :disabled="selected === draft.value.steps.length - 1" @click="move(1)">{{ t('instructor.studio.moveDown') }}</AppButton><AppButton type="button" variant="danger" size="sm" :disabled="draft.value.steps.length === 1" @click="removeStep">{{ t('common.actions.delete') }}</AppButton></div>
          </section>
        </div>
        <footer class="flex flex-wrap items-center gap-3 border-t border-border pt-5"><AppButton type="submit" :loading="saving" :disabled="!dirty">{{ t('instructor.studio.saveWorkflow') }}</AppButton><AppButton type="button" variant="ghost" @click="back">{{ t('common.actions.back') }}</AppButton><span class="text-xs text-muted-foreground">{{ t(dirty ? 'instructor.studio.unsaved' : 'instructor.studio.saved') }}</span></footer>
      </form>
    </template>
    <template v-else>
      <AppTabs v-model="tab" :tabs="[{ key: 'library', label: t('instructor.studio.library') }, { key: 'runs', label: t('instructor.studio.runs') }]" />
      <div v-if="tab === 'library'" class="divide-y divide-border rounded-xl border border-border bg-card"><div v-for="item in studio.workflows.value" :key="item.id" class="flex items-center justify-between gap-4 p-5"><div><h2 class="font-semibold">{{ item.value.name }}</h2><p class="mt-1 text-xs text-muted-foreground">{{ t('instructor.studio.stepsCount', { count: item.value.steps.length }) }}</p></div><AppButton type="button" variant="secondary" @click="edit(JSON.parse(JSON.stringify(item)) as StudioDocument<StudioWorkflow>)">{{ t('common.actions.edit') }}</AppButton></div><p class="p-5 text-sm text-muted-foreground">{{ t('instructor.studio.runFromLesson') }}</p></div>
      <div v-else class="space-y-4"><AppButton type="button" variant="secondary" @click="load">{{ t('instructor.studio.refresh') }}</AppButton><StudioRunPanel v-if="activeRun" :run-id="activeRun" @close="activeRun = null; load()" /><div v-for="item in runs" :key="item.id" class="flex flex-wrap items-center gap-4 rounded-xl border border-border bg-card p-5"><div class="min-w-0 flex-1"><h2 class="font-semibold">{{ item.value.workflow_name }}</h2><p class="text-xs text-muted-foreground">{{ new Date(item.value.created_at).toLocaleString() }}</p></div><span class="text-xs">{{ t(`instructor.studio.runStates.${item.value.status}`) }}</span><AppButton type="button" variant="secondary" @click="activeRun = item.id">{{ t('instructor.studio.openRun') }}</AppButton></div></div>
    </template>
    <ConfirmDialog :open="leave" :title="t('instructor.studio.unsaved')" :message="t('instructor.studio.discardMessage')" :confirm-label="t('instructor.studio.discard')" @confirm="answerLeave(true)" @cancel="answerLeave(false)" />
  </div>
</template>
