<script setup lang="ts">
// Set a learning goal. Exam / curriculum / job-role pick from curated,
// DAO-ratified templates that resolve directly to target skills; a job
// description (link or pasted text) is parsed on-device into skill
// *suggestions* the learner confirms before they become a goal.
import { onMounted, ref, watch } from 'vue'
import { useGoals } from '@/composables/useGoals'
import { AppButton, AppInput, AppBadge } from '@/components/ui'
import type { GoalTemplate, SkillSuggestion } from '@/types'

const emit = defineEmits<{ (e: 'added'): void }>()

const { listGoalTemplates, resolveGoal, addGoal } = useGoals()

type Tab = 'exam' | 'curriculum' | 'job_role' | 'jd'
const tab = ref<Tab>('job_role')
const tabs: { id: Tab; label: string }[] = [
  { id: 'job_role', label: 'Job role' },
  { id: 'exam', label: 'Exam' },
  { id: 'curriculum', label: 'Curriculum' },
  { id: 'jd', label: 'Job description' },
]

const templates = ref<GoalTemplate[]>([])
const selectedKey = ref('')
const busy = ref(false)
const error = ref('')

// JD path
const jdMode = ref<'link' | 'paste'>('paste')
const jdText = ref('')
const jdUrl = ref('')
const suggestions = ref<SkillSuggestion[]>([])
const chosen = ref<Set<string>>(new Set())
const jdLabel = ref('')

async function loadTemplates() {
  if (tab.value === 'jd') return
  templates.value = await listGoalTemplates(tab.value)
  selectedKey.value = ''
}
onMounted(loadTemplates)
watch(tab, () => {
  error.value = ''
  suggestions.value = []
  void loadTemplates()
})

// Curriculum templates carry board+grade; group is nicer but a flat select
// keyed by `key` is enough for the genesis set.
async function addTemplateGoal() {
  const tpl = templates.value.find((t) => t.key === selectedKey.value)
  if (!tpl) return
  busy.value = true
  error.value = ''
  try {
    const res = await resolveGoal(
      tab.value === 'curriculum'
        ? { kind: 'curriculum', board: tpl.board ?? '', grade: tpl.grade ?? '' }
        : { kind: tab.value as 'exam' | 'job_role', key: tpl.key },
    )
    await addGoal({
      label: res.label,
      goalSkillIds: res.goal_skill_ids,
      kind: tab.value as Exclude<Tab, 'jd'>,
      sourceKey: tpl.key,
      resolutionProvenance: res.resolution_provenance,
      taxonomyVersion: res.taxonomy_version,
    })
    emit('added')
  } catch (e) {
    error.value = String(e)
  } finally {
    busy.value = false
  }
}

async function parseJd() {
  busy.value = true
  error.value = ''
  suggestions.value = []
  try {
    const res = await resolveGoal(
      jdMode.value === 'link'
        ? { kind: 'jd_link', url: jdUrl.value.trim() }
        : { kind: 'jd_text', text: jdText.value },
    )
    suggestions.value = res.suggestions
    jdLabel.value = res.label
    // Pre-check strong matches (score >= 0.6).
    chosen.value = new Set(res.suggestions.filter((s) => s.score >= 0.6).map((s) => s.skill_id))
    if (!res.suggestions.length) error.value = 'No matching skills found — try a different description.'
  } catch (e) {
    error.value = String(e)
  } finally {
    busy.value = false
  }
}

function toggle(id: string) {
  const next = new Set(chosen.value)
  if (next.has(id)) next.delete(id)
  else next.add(id)
  chosen.value = next
}

async function addJdGoal() {
  const ids = suggestions.value.filter((s) => chosen.value.has(s.skill_id)).map((s) => s.skill_id)
  if (!ids.length) return
  busy.value = true
  try {
    await addGoal({
      label: jdLabel.value,
      goalSkillIds: ids,
      kind: 'jd',
      sourceUrl: jdMode.value === 'link' ? jdUrl.value.trim() : undefined,
      resolutionProvenance: 'jd_parsed',
    })
    suggestions.value = []
    jdText.value = ''
    jdUrl.value = ''
    emit('added')
  } finally {
    busy.value = false
  }
}
</script>

<template>
  <div class="space-y-5">
    <div class="flex flex-wrap gap-2">
      <button
        v-for="t in tabs"
        :key="t.id"
        class="rounded-full border px-3 py-1.5 text-sm transition-colors"
        :class="tab === t.id
          ? 'border-primary bg-primary/10 text-primary'
          : 'border-border text-muted-foreground hover:border-primary/40'"
        @click="tab = t.id"
      >
        {{ t.label }}
      </button>
    </div>

    <!-- Curated template picker -->
    <div v-if="tab !== 'jd'" class="space-y-3">
      <select
        v-model="selectedKey"
        class="w-full rounded-lg border border-border bg-input px-3 py-2 text-sm text-foreground"
      >
        <option value="" disabled>Select a {{ tab.replace('_', ' ') }}…</option>
        <option v-for="tpl in templates" :key="tpl.key" :value="tpl.key">{{ tpl.label }}</option>
      </select>
      <AppButton :disabled="!selectedKey" :loading="busy" @click="addTemplateGoal">
        Set as goal
      </AppButton>
    </div>

    <!-- Job-description parser -->
    <div v-else class="space-y-3">
      <div class="flex gap-2 text-sm">
        <button
          class="rounded px-2 py-1"
          :class="jdMode === 'paste' ? 'text-primary' : 'text-muted-foreground'"
          @click="jdMode = 'paste'"
        >Paste text</button>
        <button
          class="rounded px-2 py-1"
          :class="jdMode === 'link' ? 'text-primary' : 'text-muted-foreground'"
          @click="jdMode = 'link'"
        >From link</button>
      </div>
      <AppInput
        v-if="jdMode === 'link'"
        v-model="jdUrl"
        label="Job posting URL"
        placeholder="https://…"
      />
      <textarea
        v-else
        v-model="jdText"
        rows="6"
        class="w-full rounded-lg border border-border bg-input px-3 py-2 text-sm text-foreground"
        placeholder="Paste the job description here…"
      />
      <AppButton
        variant="outline"
        :loading="busy"
        :disabled="jdMode === 'link' ? !jdUrl.trim() : !jdText.trim()"
        @click="parseJd"
      >
        Find skills
      </AppButton>

      <div v-if="suggestions.length" class="space-y-2">
        <p class="text-sm text-muted-foreground">
          Found these skills — confirm which to add to your goal:
        </p>
        <label
          v-for="s in suggestions"
          :key="s.skill_id"
          class="flex items-center gap-3 rounded-lg border border-border p-2.5 text-sm"
        >
          <input type="checkbox" :checked="chosen.has(s.skill_id)" @change="toggle(s.skill_id)" />
          <span class="flex-1 text-foreground">{{ s.name }}</span>
          <AppBadge variant="success" v-if="s.score >= 0.6">strong</AppBadge>
          <span class="text-xs text-muted-foreground">matched “{{ s.matched }}”</span>
        </label>
        <AppButton :loading="busy" :disabled="!chosen.size" @click="addJdGoal">
          Add {{ chosen.size }} skill{{ chosen.size === 1 ? '' : 's' }} as goal
        </AppButton>
      </div>
    </div>

    <p v-if="error" class="text-sm text-error">{{ error }}</p>
  </div>
</template>
