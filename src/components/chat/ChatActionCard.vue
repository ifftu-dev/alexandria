<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { useRouter } from 'vue-router'
import { chatErrorCode, useChat } from '@/composables/useChat'
import { useGoals } from '@/composables/useGoals'
import type { ChatArtifact, SkillSuggestion } from '@/types'

/** Something the chat asked to save or change, under the answer it came
 * from. Nothing is written until the learner allows it; the app then does
 * the write itself and checks everything again. Once decided, the card
 * becomes a one-line record of what was allowed or declined. */
const props = defineProps<{ item: ChatArtifact }>()
const { t } = useI18n()
const router = useRouter()
const chat = useChat()
const { goals, resolveGoal } = useGoals()

const busy = ref(false)
const open = ref(false)
const error = ref<{ text: string; to?: string } | null>(null)

const body = computed(() => props.item.body)
const proposed = computed(() => props.item.status === 'proposed')

// A new goal: the Goals page's resolver matches skills to its description
// and the learner picks the ones to keep.
const suggestions = ref<SkillSuggestion[] | null>(null)
const chosen = ref(new Set<string>())
const finding = ref(false)
async function findSkills() {
  if (body.value.kind !== 'goal') return
  finding.value = true
  try {
    const res = await resolveGoal({ kind: 'learning_goal', text: body.value.text })
    suggestions.value = res.suggestions
    chosen.value = new Set(res.suggestions.filter((s) => s.score >= 0.6).map((s) => s.skill_id))
  } catch {
    suggestions.value = []
  } finally {
    finding.value = false
  }
}
function toggle(id: string) {
  const next = new Set(chosen.value)
  if (next.has(id)) next.delete(id)
  else next.add(id)
  chosen.value = next
}
onMounted(() => {
  if (proposed.value && body.value.kind === 'goal') void findSkills()
})

// A goal change, measured against the goal as it is now.
const goal = computed(() => {
  const b = body.value
  const id = b.kind === 'goal_change' || b.kind === 'enroll' || b.kind === 'goal' ? b.goal_id : undefined
  return id ? goals.value.find((g) => g.id === id) ?? null : null
})
const pending = computed(() => {
  const b = body.value
  const g = goal.value
  if (b.kind !== 'goal_change' || !g) return null
  const add = b.add.filter((s) => !g.goal_skill_ids.includes(s.id))
  const remove = b.remove.filter((s) => g.goal_skill_ids.includes(s.id))
  const rename = b.rename && b.rename !== g.label ? b.rename : null
  const left = g.goal_skill_ids.filter((id) => !remove.some((s) => s.id === id)).length + add.length
  return { add, remove, rename, left }
})

const cards = computed(() => (body.value.kind === 'flashcards' ? body.value.cards : []))

const ask = computed(() => {
  const b = body.value
  switch (b.kind) {
    case 'note': return t('chat.actions.ask.note')
    case 'flashcards': return t('chat.actions.ask.flashcards', { count: b.cards.length }, b.cards.length)
    case 'course_note': return t('chat.actions.ask.courseNote', { lesson: b.lesson })
    case 'goal': return t('chat.actions.ask.goal')
    case 'goal_change': return t('chat.actions.ask.goalChange', { goal: goal.value?.label ?? props.item.title })
    case 'enroll': return t('chat.actions.ask.enroll')
  }
  return ''
})

/** Why Allow is not available yet, if it isn't. */
const blocked = computed(() => {
  const b = body.value
  if (b.kind === 'goal') {
    if (finding.value || !suggestions.value) return t('chat.actions.goal.finding')
    if (!suggestions.value.length) return t('chat.actions.goal.noSkills')
    if (!chosen.value.size) return t('chat.actions.goal.pickOne')
  }
  if (b.kind === 'goal_change') {
    if (!goal.value) return t('chat.actions.change.goalMissing')
    if (pending.value && !pending.value.left) return t('chat.actions.change.wouldEmpty')
  }
  return ''
})

const coursePage = computed(() => {
  const b = body.value
  if (b.kind === 'course_note') return `/learn/${b.course_id}`
  if (b.kind === 'enroll') return `/courses/${b.course_id}`
  return undefined
})

const done = computed<{ text: string; to?: string; muted?: boolean }>(() => {
  const b = body.value
  switch (b.kind) {
    case 'note':
    case 'flashcards':
      return { text: t('chat.actions.done.saved') }
    case 'course_note':
      return { text: t('chat.actions.done.courseNote', { lesson: b.lesson }), to: coursePage.value }
    case 'goal':
      return goal.value
        ? { text: t('chat.actions.done.goal'), to: '/goals' }
        : { text: t('chat.actions.done.goalRemoved'), muted: true }
    case 'goal_change':
      return { text: t('chat.actions.done.goalChange'), to: '/goals' }
    case 'enroll':
      return { text: t('chat.actions.done.enroll'), to: coursePage.value }
  }
  return { text: '' }
})

function describe(e: unknown): { text: string; to?: string } {
  const code = chatErrorCode(e)
  const raw = String(e instanceof Error ? e.message : e)
  if (code === 'not_enrolled') return { text: t('chat.actions.errors.notEnrolled'), to: coursePage.value }
  if (code === 'needs_plugins' || code === 'open_course') return { text: t('chat.actions.errors.openCourse'), to: coursePage.value }
  if (code === 'not_found' || code === 'invalid_input') return { text: t('chat.actions.errors.stale') }
  if (code === 'assessment_item') return { text: t('chat.actions.errors.assessmentItem') }
  if (/assessment/i.test(raw)) return { text: t('chat.errors.assessment_active') }
  return { text: t('chat.actions.errors.generic') }
}

async function allow() {
  if (blocked.value) return
  busy.value = true
  error.value = null
  try {
    const picked = body.value.kind === 'goal'
      ? (suggestions.value ?? []).filter((s) => chosen.value.has(s.skill_id)).map((s) => s.skill_id)
      : undefined
    await chat.allowAction(props.item.id, picked)
  } catch (e) {
    error.value = describe(e)
  } finally {
    busy.value = false
  }
}

async function decline() {
  busy.value = true
  error.value = null
  try {
    await chat.declineAction(props.item.id)
  } catch (e) {
    error.value = describe(e)
  } finally {
    busy.value = false
  }
}
</script>

<template>
  <p v-if="item.status === 'declined'" class="act-record text-muted-foreground">
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"><circle cx="12" cy="12" r="8.5" /><path d="M6 18L18 6" /></svg>
    <span>{{ t('chat.actions.declined') }} · <span class="truncate">{{ item.title }}</span></span>
  </p>
  <p v-else-if="!proposed" class="act-record" :class="done.muted ? 'text-muted-foreground' : 'text-success'">
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M5 12l5 5L20 7" /></svg>
    <span>{{ done.text }} · <span class="truncate">{{ item.title }}</span></span>
    <button v-if="done.to" type="button" class="act-link" @click="router.push(done.to)">{{ t('chat.actions.open') }}</button>
  </p>

  <section v-else class="act" :aria-label="ask">
    <div class="act-head">
      <span class="act-ic" aria-hidden="true">
        <svg v-if="body.kind === 'note' || body.kind === 'course_note'" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linejoin="round"><path d="M6 3h9l4 4v14H6z" /><path d="M9 12h6M9 16h4" /></svg>
        <svg v-else-if="body.kind === 'flashcards'" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8"><rect x="3" y="6" width="14" height="12" rx="2" /><path d="M7 3h12a2 2 0 0 1 2 2v10" /></svg>
        <svg v-else-if="body.kind === 'enroll'" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linejoin="round"><path d="M3 9l9-5 9 5-9 5z" /><path d="M7 11v5c3 2 7 2 10 0v-5" /></svg>
        <svg v-else viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8"><circle cx="12" cy="12" r="8" /><circle cx="12" cy="12" r="4" /><circle cx="12" cy="12" r="0.8" fill="currentColor" /></svg>
      </span>
      <div class="min-w-0 flex-1">
        <p class="act-asking">{{ t('chat.actions.asking') }}</p>
        <p class="act-ask">{{ ask }}</p>
      </div>
    </div>

    <div class="act-body">
      <p class="act-title">{{ item.title }}</p>

      <template v-if="body.kind === 'note' || body.kind === 'course_note'">
        <p class="whitespace-pre-wrap" :class="{ 'line-clamp-4': !open }">{{ body.text }}</p>
        <button v-if="body.text.length > 240" type="button" class="act-link mt-1" :aria-expanded="open" @click="open = !open">
          {{ open ? t('chat.actions.less') : t('chat.actions.more') }}
        </button>
      </template>

      <template v-else-if="body.kind === 'flashcards'">
        <dl class="grid gap-1.5">
          <div v-for="(card, i) in open ? cards : cards.slice(0, 3)" :key="i">
            <dt class="font-medium text-foreground">{{ card.front }}</dt>
            <dd>{{ card.back }}</dd>
          </div>
        </dl>
        <button v-if="cards.length > 3" type="button" class="act-link mt-1" :aria-expanded="open" @click="open = !open">
          {{ open ? t('chat.actions.less') : t('chat.actions.moreCards', { count: cards.length - 3 }) }}
        </button>
      </template>

      <template v-else-if="body.kind === 'goal'">
        <p>{{ body.text }}</p>
        <div v-if="suggestions && suggestions.length" class="mt-2 grid gap-1">
          <p>{{ t('chat.actions.goal.pick') }}</p>
          <label v-for="s in suggestions" :key="s.skill_id" class="act-skill">
            <input type="checkbox" :checked="chosen.has(s.skill_id)" :disabled="busy" @change="toggle(s.skill_id)" />
            <span class="flex-1">{{ s.name }}</span>
            <span v-if="s.score >= 0.6" class="text-xs text-success">{{ t('chat.actions.goal.strong') }}</span>
          </label>
        </div>
      </template>

      <template v-else-if="body.kind === 'goal_change'">
        <p>{{ body.text }}</p>
        <ul v-if="pending" class="mt-1.5 grid gap-0.5">
          <li v-if="pending.rename" class="text-foreground">{{ t('chat.actions.change.rename', { name: pending.rename }) }}</li>
          <li v-for="s in pending.add" :key="`add-${s.id}`" class="text-success">+ {{ s.name }}</li>
          <li v-for="s in pending.remove" :key="`remove-${s.id}`" class="line-through">− {{ s.name }}</li>
          <li v-if="!pending.rename && !pending.add.length && !pending.remove.length">{{ t('chat.actions.change.nothingLeft') }}</li>
        </ul>
      </template>

      <template v-else-if="body.kind === 'enroll'">
        <p>{{ body.text }}</p>
        <p v-if="goal" class="mt-1 text-xs">{{ t('chat.actions.enrollFor', { goal: goal.label }) }}</p>
      </template>
    </div>

    <p v-if="error" class="act-error" role="alert">
      {{ error.text }}
      <button v-if="error.to" type="button" class="act-link" @click="router.push(error.to)">{{ t('chat.actions.openCourse') }}</button>
    </p>
    <p v-else-if="blocked" class="act-hint">{{ blocked }}</p>

    <div class="act-foot">
      <button type="button" class="act-allow" :disabled="busy || !!blocked" @click="allow">{{ t('chat.actions.allow') }}</button>
      <button type="button" class="act-deny" :disabled="busy" @click="decline">{{ t('chat.actions.decline') }}</button>
      <span class="act-promise">{{ t('chat.actions.promise') }}</span>
    </div>
  </section>
</template>

<style scoped>
.act { border-radius: 0.875rem; border: 1px solid color-mix(in srgb, var(--app-primary) 22%, var(--app-border)); background: color-mix(in srgb, var(--app-primary) 4%, var(--app-background)); font-size: 0.8125rem; color: var(--app-muted-foreground); }
.act-head { display: flex; align-items: flex-start; gap: 0.625rem; padding: 0.75rem 0.875rem 0.25rem; }
.act-ic { display: grid; flex: none; place-items: center; width: 1.75rem; height: 1.75rem; border-radius: 0.5rem; background: color-mix(in srgb, var(--app-primary) 12%, transparent); color: var(--app-primary); }
.act-ic svg { width: 0.9375rem; height: 0.9375rem; }
.act-asking { font-size: 0.75rem; color: var(--app-muted-foreground); }
.act-ask { font-weight: 600; color: var(--app-foreground); line-height: 1.35; }
.act-body { padding: 0.25rem 0.875rem 0.5rem 3.25rem; }
.act-title { font-weight: 500; color: var(--app-foreground); margin-bottom: 0.125rem; }
.act-skill { display: flex; align-items: center; gap: 0.5rem; border-radius: 0.5rem; padding: 0.3rem 0.5rem; color: var(--app-foreground); background: color-mix(in srgb, var(--app-foreground) 5%, transparent); }
.act-hint, .act-error { padding: 0 0.875rem 0.5rem 3.25rem; font-size: 0.75rem; }
.act-error { color: var(--app-error); }
.act-foot { display: flex; flex-wrap: wrap; align-items: center; gap: 0.375rem; padding: 0.5rem 0.875rem 0.75rem 3.25rem; }
.act-allow { border-radius: 999px; padding: 0.3rem 0.9rem; font-weight: 600; background: var(--app-primary); color: var(--app-primary-foreground); }
.act-deny { border-radius: 999px; padding: 0.3rem 0.8rem; font-weight: 500; color: var(--app-foreground); background: color-mix(in srgb, var(--app-foreground) 7%, transparent); }
.act-allow:disabled, .act-deny:disabled { opacity: 0.5; }
.act-allow:not(:disabled):hover { filter: brightness(1.08); }
.act-deny:not(:disabled):hover { background: color-mix(in srgb, var(--app-foreground) 11%, transparent); }
.act-promise { margin-inline-start: auto; font-size: 0.75rem; }
.act-link { text-decoration: underline; text-underline-offset: 2px; }
.act-link:hover { color: var(--app-foreground); }
.act-record { display: flex; flex-wrap: wrap; align-items: center; gap: 0.25rem 0.375rem; font-size: 0.75rem; }
.act-record svg { width: 0.8125rem; height: 0.8125rem; flex: none; }
@media (max-width: 420px) {
  .act-body, .act-hint, .act-error, .act-foot { padding-inline-start: 0.875rem; }
  .act-promise { margin-inline-start: 0; width: 100%; }
}
</style>
