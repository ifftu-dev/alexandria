<script setup lang="ts">
import { computed, ref } from 'vue'
import SkillAtlasExplorer from '@/components/skills/SkillAtlasExplorer.vue'
import { buildSkillAtlas } from '@/utils/skillAtlas'
import type { AtlasFeedback, AtlasViewState } from '@/composables/useSkillAtlas'
import type { SkillGraphEdge, SkillInfo, VerifiableCredential } from '@/types'
import taxonomy from '../../bootstrap/public_taxonomy.json'

const params = new URLSearchParams(location.search)
const subjects = new Map(taxonomy.subjects.map(subject => [subject.id, subject]))
const fields = new Map(taxonomy.subject_fields.map(field => [field.id, field]))
const skills: SkillInfo[] = taxonomy.skills.map(skill => {
  const subject = subjects.get(skill.subject_id)
  const field = subject ? fields.get(subject.subject_field_id) : undefined
  return { ...skill, subject_name: subject?.name ?? null, subject_field_id: field?.id ?? null,
    subject_field_name: field?.name ?? null,
    prerequisite_count: taxonomy.skill_prerequisites.filter(edge => edge.skill_id === skill.id).length,
    dependent_count: taxonomy.skill_prerequisites.filter(edge => edge.prerequisite_id === skill.id).length }
})
const skillById = new Map(skills.map(skill => [skill.id, skill]))
const edges: SkillGraphEdge[] = taxonomy.skill_prerequisites.map(edge => ({ ...edge,
  skill_name: skillById.get(edge.skill_id)?.name ?? edge.skill_id,
  skill_bloom: skillById.get(edge.skill_id)?.bloom_level ?? '',
  prerequisite_name: skillById.get(edge.prerequisite_id)?.name ?? edge.prerequisite_id,
  prerequisite_bloom: skillById.get(edge.prerequisite_id)?.bloom_level ?? '',
}))
if (params.get('scenario') === 'stress') {
  skills.push({ id: 'fixture-unassigned', name: 'A very long skill label about community stewardship, collaborative problem solving, and inclusive systems design — نام کی مکمل وضاحت',
    description: 'Synthetic stress case: unknown field, long bilingual label, malformed earned level, and an unloaded prerequisite.',
    subject_id: null, subject_name: null, subject_field_id: null, subject_field_name: null,
    bloom_level: 'evaluate', prerequisite_count: 1, dependent_count: 0, created_at: null })
  edges.push({ skill_id: 'fixture-unassigned', skill_name: 'Long-label skill', skill_bloom: 'evaluate',
    prerequisite_id: 'fixture-missing', prerequisite_name: 'An unloaded prerequisite', prerequisite_bloom: 'apply' })
}
const revision = ref(0)
function credential(skillId: string, level: unknown): VerifiableCredential {
  return { '@context': [], type: [], issuer: 'did:fixture:issuer', validFrom: '2026-10-27T00:00:00Z',
    credentialSubject: { id: 'did:fixture:learner', skillId, level },
    proof: { type: '', cryptosuite: '', created: '', verificationMethod: '', proofPurpose: '', proofValue: '' } }
}
const model = computed(() => buildSkillAtlas({ skills, edges, subjectDid: 'did:fixture:learner', subjectFields: taxonomy.subject_fields,
  activeCredentials: params.get('scenario') === 'empty-earned' ? [] : [
    ...skills.filter((_, index) => index % 3 === 0).map((skill, index) => credential(skill.id, Math.min(5, index % 6 + revision.value))),
    ...(params.get('scenario') === 'stress' ? [credential('fixture-unassigned', 'malformed')] : []),
  ],
}))
const view = ref<AtlasViewState>({ selectedSkillId: null, focusedFieldId: null, unassignedFocused: false,
  query: '', bloomEmphasis: null, neighborhood: 'none', neighborhoodDepth: 1, camera: null, inspectorCollapsed: false })
const feedback = ref<AtlasFeedback | null>(null)
const instance = ref(0)
const lastAction = ref('')
function improve(): void {
  revision.value++
  feedback.value = { revision: revision.value, earnedIds: [], improvedIds: model.value.nodes.filter(node => node.status === 'earned').map(node => node.skill.id) }
}
</script>

<template>
  <main class="fixture-atlas">
    <div class="fixture-banner">
      <span>Production explorer · bootstrap taxonomy · illustrative achievements</span>
      <button type="button" @click="instance++">Remount explorer</button>
      <button type="button" @click="improve">Simulate evidence update</button>
      <output>{{ lastAction }}</output>
    </div>
    <SkillAtlasExplorer :key="instance" :model="model" :view="view" :feedback="feedback" :reduced-motion="params.get('motion') === 'reduced'"
      @update:view="view = $event" @open-skill="lastAction = `open-skill: ${$event}`" @learn-skill="lastAction = `learn-skill: ${$event}`"
      @feedback-consumed="feedback?.revision === $event && (feedback = null)" />
  </main>
</template>

<style scoped>
.fixture-atlas { height: 100%; overflow: auto; padding: 28px; max-width: 1600px; margin: auto; }
.fixture-banner { display: flex; gap: 12px; align-items: center; flex-wrap: wrap; margin-block-end: 28px; padding-block-end: 16px; border-block-end: 1px solid var(--app-border); font-size: 12px; color: var(--app-muted-foreground); }
.fixture-banner button { padding: 8px; border: 1px solid var(--app-border); border-radius: 8px; background: var(--app-card); min-height: 44px; }
@media (max-width: 700px) { .fixture-atlas { height: 100%; overflow: auto; padding: 12px; } .fixture-banner { margin-block-end: 16px; } }
</style>
