<script setup lang="ts">
import { onBeforeUnmount, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useLocalApi } from '@/composables/useLocalApi'
import { AppButton, AppTextarea } from '@/components/ui'
import type { StudioDocument, StudioRun } from '@/types'

const props = defineProps<{ runId: string; draftDirty?: boolean }>()
const emit = defineEmits<{ applied: []; close: [] }>()
const { t } = useI18n()
const { invoke } = useLocalApi()
const run = ref<StudioDocument<StudioRun> | null>(null)
const output = ref('')
const outputStep = ref(0)
const error = ref('')
const busy = ref(false)
let timer: ReturnType<typeof setTimeout> | undefined
let generation = 0
function update(next: StudioDocument<StudioRun>) {
  const becameReview = next.value.status === 'review' && run.value?.value.status !== 'review'
  run.value = next
  if (becameReview) {
    const candidates = next.value.steps.map((s, i) => ({ s, i })).filter(({ s }) => s.role !== 'review' && s.output)
    outputStep.value = candidates[candidates.length - 1]?.i ?? 0
    output.value = next.value.steps[outputStep.value]?.output ?? ''
  }
}
async function poll(epoch: number) {
  try {
    const next = await invoke<StudioDocument<StudioRun>>('studio_get_run', { runId: props.runId })
    if (epoch !== generation) return
    update(next)
    if (next.value.status === 'running') timer = setTimeout(() => void poll(epoch), 1000)
  } catch (e) { if (epoch === generation) error.value = String(e) }
}
watch(() => props.runId, () => { clearTimeout(timer); run.value = null; output.value = ''; error.value = ''; void poll(++generation) }, { immediate: true })
onBeforeUnmount(() => { generation++; clearTimeout(timer) })
type RunAction = 'studio_start_run' | 'studio_stop_run' | 'studio_apply_run' | 'studio_undo_run'
async function action(command: RunAction) {
  if (!run.value || (props.draftDirty && ['studio_apply_run', 'studio_undo_run'].includes(command))) return
  busy.value = true; error.value = ''; clearTimeout(timer)
  const epoch = ++generation
  try {
    const args = { runId: run.value.id, revision: run.value.revision, text: output.value }
    const handlers = {
      studio_start_run: () => invoke<StudioDocument<StudioRun>>('studio_start_run', args),
      studio_stop_run: () => invoke<StudioDocument<StudioRun>>('studio_stop_run', args),
      studio_apply_run: () => invoke<StudioDocument<StudioRun>>('studio_apply_run', args),
      studio_undo_run: () => invoke<StudioDocument<StudioRun>>('studio_undo_run', args),
    }
    const next = await handlers[command]()
    if (epoch !== generation) return
    update(next)
    if (next.value.status === 'running') timer = setTimeout(() => void poll(epoch), 500)
    if (['applied', 'undone'].includes(next.value.status)) emit('applied')
  } catch (e) { if (epoch === generation) error.value = String(e) } finally { if (epoch === generation) busy.value = false }
}
</script>

<template>
  <section class="space-y-4 rounded-xl border border-border bg-card p-5" :aria-label="t('instructor.studio.run')">
    <header class="flex items-start justify-between gap-3"><div><h2 class="font-semibold">{{ run?.value.workflow_name ?? t('instructor.studio.run') }}</h2><p v-if="run" class="mt-1 text-xs text-muted-foreground" role="status">{{ t(`instructor.studio.runStates.${run.value.status}`) }}</p></div><AppButton type="button" variant="ghost" size="sm" @click="emit('close')">{{ t('common.actions.close') }}</AppButton></header>
    <p v-if="error || run?.value.error" role="alert" class="text-sm text-error">{{ error || run?.value.error }}</p>
    <template v-if="run">
      <p v-if="run.value.status === 'prepared'" class="rounded-lg bg-primary/5 p-3 text-sm">{{ t('instructor.studio.runDisclosure') }}</p>
      <ol class="divide-y divide-border">
        <li v-for="(step, i) in run.value.steps" :key="i" class="py-3"><div class="flex items-center gap-3 text-sm"><span class="flex h-6 w-6 shrink-0 items-center justify-center rounded-full bg-muted text-xs">{{ step.output ? '✓' : i + 1 }}</span><strong>{{ t(`instructor.studio.roleNames.${step.role}`) }}</strong><span class="ml-auto text-xs text-muted-foreground">{{ step.connection.name }} · {{ step.connection.location }}</span></div><details class="mt-2 text-xs text-muted-foreground"><summary class="cursor-pointer">{{ t('instructor.studio.effectivePrompt') }}</summary><pre class="mt-2 whitespace-pre-wrap break-words rounded-lg bg-background p-3 font-sans">{{ step.effective_prompt }}</pre></details><details v-if="step.output" class="mt-2 text-xs"><summary class="cursor-pointer">{{ t('instructor.studio.output') }}</summary><pre class="mt-2 whitespace-pre-wrap break-words rounded-lg bg-background p-3 font-sans">{{ step.output }}</pre></details></li>
      </ol>
      <details v-if="run.value.status === 'prepared'" class="text-xs text-muted-foreground"><summary class="cursor-pointer">{{ t('instructor.studio.sharedContext') }}</summary><pre class="mt-2 whitespace-pre-wrap break-words rounded-lg bg-background p-3">{{ run.value.context }}</pre></details>
      <div v-if="run.value.status === 'review'" class="space-y-4 border-t border-border pt-4">
        <label class="block text-sm"><span class="mb-2 block">{{ t('instructor.studio.chooseOutput') }}</span><select v-model="outputStep" class="w-full rounded-lg border border-border bg-background p-2" @change="output = run.value.steps[outputStep]?.output ?? ''"><option v-for="(step, i) in run.value.steps" :key="i" :value="i">{{ t(`instructor.studio.roleNames.${step.role}`) }}</option></select></label>
        <details class="text-sm"><summary class="cursor-pointer">{{ t('instructor.studio.originalLesson') }}</summary><pre class="mt-2 whitespace-pre-wrap break-words rounded-lg bg-background p-3 font-sans">{{ run.value.original_content || t('instructor.studio.emptyLesson') }}</pre></details>
        <p v-if="draftDirty" role="status" class="text-sm text-warning">{{ t('instructor.studio.saveLessonFirst') }}</p>
        <AppTextarea v-model="output" :label="t('instructor.studio.reviewDraft')" :rows="12" :maxlength="128000" />
        <p class="text-xs text-muted-foreground">{{ t('instructor.studio.applyHelp') }}</p>
        <AppButton type="button" :loading="busy" :disabled="!output.trim() || draftDirty" @click="action('studio_apply_run')">{{ t('instructor.studio.approveApply') }}</AppButton>
      </div>
      <AppButton type="button" v-if="['prepared', 'paused', 'failed'].includes(run.value.status)" :loading="busy" @click="action('studio_start_run')">{{ t(run.value.status === 'prepared' ? 'instructor.studio.startRun' : 'instructor.studio.resume') }}</AppButton>
      <AppButton type="button" v-if="run.value.status === 'running'" variant="secondary" :loading="busy" @click="action('studio_stop_run')">{{ t('instructor.studio.stop') }}</AppButton>
      <AppButton type="button" v-if="run.value.status === 'applied'" variant="secondary" :loading="busy" :disabled="draftDirty" @click="action('studio_undo_run')">{{ t('instructor.studio.undo') }}</AppButton>
    </template>
  </section>
</template>
