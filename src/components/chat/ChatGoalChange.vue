<script setup lang="ts">
import { computed, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { useRouter } from 'vue-router'
import { AppButton } from '@/components/ui'
import { useChat } from '@/composables/useChat'
import { useGoals } from '@/composables/useGoals'
import type { ChatArtifact } from '@/types'

/** A change a model proposed to one of the learner's goals. The app already
 * matched its skill names to real skills; the learner sees the difference
 * against the goal as it is now and applies it through the Goals service. */
const props = defineProps<{ item: ChatArtifact }>()
const { t } = useI18n()
const router = useRouter()
const chat = useChat()
const { goals, updateGoal } = useGoals()

const busy = ref(false)
const error = ref('')

const change = computed(() => (props.item.body.kind === 'goal_change' ? props.item.body : null))
const goal = computed(() => goals.value.find((g) => g.id === change.value?.goal_id) ?? null)
/** The change measured against the goal as it is now. */
const pending = computed(() => {
  const c = change.value
  const g = goal.value
  if (!c || !g) return null
  const add = c.add.filter((s) => !g.goal_skill_ids.includes(s.id))
  const remove = c.remove.filter((s) => g.goal_skill_ids.includes(s.id))
  const rename = c.rename && c.rename !== g.label ? c.rename : null
  const next = [...g.goal_skill_ids.filter((id) => !remove.some((s) => s.id === id)), ...add.map((s) => s.id)]
  return { add, remove, rename, next, empty: !add.length && !remove.length && !rename }
})

async function apply() {
  const c = change.value
  const p = pending.value
  if (!c || !p || !p.next.length) return
  busy.value = true
  error.value = ''
  try {
    if (!p.empty) await updateGoal(c.goal_id, { label: p.rename ?? undefined, goalSkillIds: p.next })
    await chat.goalChanged(props.item.id)
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
  <p v-if="item.status === 'added'" class="flex flex-wrap items-center gap-1 text-[11px] text-success">
    <svg class="h-3 w-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M5 12l5 5L20 7" /></svg>
    {{ t('chat.goalChange.applied') }} · {{ item.title }}
    <button type="button" class="underline underline-offset-2 hover:text-foreground" @click="router.push('/goals')">{{ t('chat.goalProposal.open') }}</button>
  </p>
  <div v-else-if="change" class="rounded-xl border border-dashed border-border bg-background px-3 py-2 text-xs">
    <p class="font-semibold">{{ t('chat.goalChange.title') }}</p>
    <p class="mt-0.5 text-muted-foreground">{{ t('chat.goalChange.hint') }}</p>
    <p class="mt-1.5 text-sm font-medium text-foreground">{{ goal?.label ?? item.title }}</p>
    <p class="mt-0.5 text-muted-foreground">{{ change.text }}</p>

    <p v-if="!goal" class="mt-2 text-muted-foreground">{{ t('chat.goalChange.goalMissing') }}</p>
    <template v-else-if="pending">
      <p v-if="pending.empty" class="mt-2 text-muted-foreground">{{ t('chat.goalChange.nothingLeft') }}</p>
      <ul v-else class="mt-2 space-y-0.5">
        <li v-if="pending.rename" class="text-foreground">{{ t('chat.goalChange.rename', { name: pending.rename }) }}</li>
        <li v-for="s in pending.add" :key="`add-${s.id}`" class="text-success">+ {{ s.name }}</li>
        <li v-for="s in pending.remove" :key="`remove-${s.id}`" class="text-muted-foreground line-through">− {{ s.name }}</li>
      </ul>
      <p v-if="!pending.next.length" class="mt-1 text-error">{{ t('chat.goalChange.wouldEmpty') }}</p>
    </template>

    <p v-if="error" class="mt-1.5 text-error">{{ error }}</p>
    <div class="mt-2 flex flex-wrap gap-1.5">
      <AppButton v-if="goal && pending && pending.next.length" size="xs" type="button" :loading="busy" @click="apply">{{ pending.empty ? t('chat.goalChange.done') : t('chat.goalChange.apply') }}</AppButton>
      <AppButton variant="ghost" size="xs" type="button" :disabled="busy" @click="dismiss">{{ t('chat.artifacts.dismiss') }}</AppButton>
    </div>
  </div>
</template>
