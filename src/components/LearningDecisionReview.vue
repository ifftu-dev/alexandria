<script setup lang="ts">
import { onBeforeUnmount, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useLocalApi } from '@/composables/useLocalApi'
import { onProfileLocked } from '@/composables/useProfiles'
import { AppButton } from '@/components/ui'
import type { DecisionTask, LearningDecisionReview } from '@/types'

const props = defineProps<{ source: string; task: DecisionTask }>()
const emit = defineEmits<{ suggestion: [skill: { skill_id: string; name: string; matched: string; score: number }] }>()
const { t } = useI18n()
const { invoke } = useLocalApi()
const result = ref<LearningDecisionReview | null>(null)
const busy = ref(false)
const error = ref('')
let epoch = 0
function clear() { epoch++; result.value = null; error.value = ''; busy.value = false }
watch(() => [props.source, props.task], clear)
const remove = onProfileLocked(clear)
onBeforeUnmount(() => { clear(); remove() })
async function check() {
  const generation = ++epoch
  busy.value = true; error.value = ''; result.value = null
  try {
    const next = await invoke<LearningDecisionReview>('decision_learning_review', { source: props.source, task: props.task })
    if (generation === epoch) result.value = next
  } catch (e) { if (generation === epoch) error.value = String(e) }
  finally { if (generation === epoch) busy.value = false }
}
function add(id: string) {
  if (!result.value) return
  emit('suggestion', { skill_id: id, name: result.value.names[id] ?? id, matched: result.value.evidence[id] ?? '', score: 0 })
}
</script>

<template>
  <section class="space-y-3 rounded-lg border border-border p-3">
    <p class="text-xs text-muted-foreground">{{ t('instructor.studio.decisionDisclosure') }}</p>
    <AppButton variant="secondary" :loading="busy" :disabled="!source.trim()" @click="check">{{ t('instructor.studio.decisionRun') }}</AppButton>
    <p v-if="error" role="status" class="text-sm text-muted-foreground">{{ error }}</p>
    <p v-if="result && !result.record" role="status" class="text-sm">{{ t('instructor.studio.decisionShadowComplete') }}</p>
    <template v-if="result?.record">
      <p class="text-xs text-muted-foreground">{{ result.record.model }} · {{ result.record.rubric_version }}</p>
      <div v-for="decision in result.record.decisions" :key="decision.skill_id" class="space-y-2 border-t border-border pt-2 text-sm">
        <p><strong>{{ result.names[decision.skill_id] }}</strong> · {{ decision.relation.value }} · {{ decision.bloom.value }} · {{ Math.round(decision.relation.confidence * 100) }}%</p>
        <blockquote v-if="result.evidence[decision.skill_id]" class="border-l-2 border-border pl-3">{{ result.evidence[decision.skill_id] }}</blockquote>
        <AppButton v-if="['required', 'preferred'].includes(decision.relation.value) && result.evidence[decision.skill_id]" size="sm" variant="ghost" @click="add(decision.skill_id)">{{ t('instructor.studio.decisionAdd') }}</AppButton>
      </div>
      <p v-if="!result.record.decisions.length" class="text-sm">{{ t('instructor.studio.decisionNoResult') }}</p>
    </template>
  </section>
</template>
