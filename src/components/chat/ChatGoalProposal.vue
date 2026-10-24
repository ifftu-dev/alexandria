<script setup lang="ts">
import { computed, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { useRouter } from 'vue-router'
import { AppButton } from '@/components/ui'
import { useChat } from '@/composables/useChat'
import { useGoals } from '@/composables/useGoals'
import type { ChatArtifact, SkillSuggestion } from '@/types'

/** A goal a model proposed. The Goals page's own resolver matches skills to
 * its description and the learner picks them; only then is the goal added,
 * through the same service the Goals page uses. */
const props = defineProps<{ item: ChatArtifact }>()
const { t } = useI18n()
const router = useRouter()
const chat = useChat()
const { goals, resolveGoal, addGoal } = useGoals()

const busy = ref(false)
const error = ref('')
const suggestions = ref<SkillSuggestion[] | null>(null)
const chosen = ref(new Set<string>())

const text = computed(() => (props.item.body.kind === 'goal' ? props.item.body.text : ''))
const goalId = computed(() => (props.item.body.kind === 'goal' ? props.item.body.goal_id : undefined))
/** The goal was added and is still among the learner's goals. */
const present = computed(() => !!goalId.value && goals.value.some((g) => g.id === goalId.value))

async function findSkills() {
  busy.value = true
  error.value = ''
  try {
    const res = await resolveGoal({ kind: 'learning_goal', text: text.value })
    suggestions.value = res.suggestions
    chosen.value = new Set(res.suggestions.filter((s) => s.score >= 0.6).map((s) => s.skill_id))
  } catch {
    error.value = t('chat.goalProposal.resolveFailed')
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

async function add() {
  const ids = (suggestions.value ?? []).filter((s) => chosen.value.has(s.skill_id)).map((s) => s.skill_id)
  if (!ids.length) return
  busy.value = true
  error.value = ''
  try {
    const goal = await addGoal({
      label: props.item.title,
      goalSkillIds: ids,
      kind: 'learning_goal',
      resolutionProvenance: 'goal_parsed',
    })
    await chat.goalAdded(props.item.id, goal.id)
  } catch {
    error.value = t('chat.goalProposal.addFailed')
  } finally {
    busy.value = false
  }
}

async function dismiss() {
  busy.value = true
  try {
    await chat.deleteArtifact(props.item.id)
  } catch {
    error.value = t('chat.goalProposal.addFailed')
  } finally {
    busy.value = false
  }
}
</script>

<template>
  <p v-if="item.status === 'added'" class="flex flex-wrap items-center gap-1 text-xs" :class="present ? 'text-success' : 'text-muted-foreground'">
    <svg v-if="present" class="h-3 w-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M5 12l5 5L20 7" /></svg>
    {{ present ? t('chat.goalProposal.added') : t('chat.goalProposal.removed') }} · {{ item.title }}
    <button v-if="present" type="button" class="underline underline-offset-2 hover:text-foreground" @click="router.push('/goals')">{{ t('chat.goalProposal.open') }}</button>
  </p>
  <div v-else class="rounded-2xl bg-muted/60 px-3.5 py-3 text-[13px]">
    <p class="font-semibold">{{ t('chat.goalProposal.title') }}</p>
    <p class="mt-0.5 text-muted-foreground">{{ t('chat.goalProposal.hint') }}</p>
    <p class="mt-1.5 text-sm font-medium text-foreground">{{ item.title }}</p>
    <p class="mt-0.5 text-muted-foreground">{{ text }}</p>

    <div v-if="suggestions" class="mt-2 space-y-1">
      <p v-if="!suggestions.length" class="text-muted-foreground">{{ t('chat.goalProposal.noSkills') }}</p>
      <template v-else>
        <p class="text-muted-foreground">{{ t('chat.goalProposal.pick') }}</p>
        <label v-for="s in suggestions" :key="s.skill_id" class="flex items-center gap-2 rounded-lg border border-border px-2 py-1.5 text-foreground">
          <input type="checkbox" :checked="chosen.has(s.skill_id)" :disabled="busy" @change="toggle(s.skill_id)" />
          <span class="flex-1">{{ s.name }}</span>
          <span v-if="s.score >= 0.6" class="text-xs text-success">{{ t('chat.goalProposal.strong') }}</span>
        </label>
      </template>
    </div>

    <p v-if="error" class="mt-1.5 text-error">{{ error }}</p>
    <div class="mt-2 flex flex-wrap gap-1.5">
      <AppButton v-if="!suggestions" size="xs" type="button" :loading="busy" @click="findSkills">{{ t('chat.goalProposal.find') }}</AppButton>
      <AppButton v-else-if="suggestions.length" size="xs" type="button" :loading="busy" :disabled="!chosen.size" @click="add">{{ t('chat.goalProposal.add', { count: chosen.size }, chosen.size) }}</AppButton>
      <AppButton v-else variant="secondary" size="xs" type="button" @click="router.push('/goals')">{{ t('chat.goalProposal.openGoals') }}</AppButton>
      <AppButton variant="ghost" size="xs" type="button" :disabled="busy" @click="dismiss">{{ t('chat.artifacts.dismiss') }}</AppButton>
    </div>
  </div>
</template>
