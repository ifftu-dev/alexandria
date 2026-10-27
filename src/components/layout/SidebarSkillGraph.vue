<script setup lang="ts">
import { onMounted } from 'vue'
import { useI18n } from 'vue-i18n'
import { useSkillAtlas } from '@/composables/useSkillAtlas'
import type { AtlasField, AtlasNode } from '@/utils/skillAtlas'
import '@/assets/css/skill-atlas.css'

const { t } = useI18n()
const atlas = useSkillAtlas()
const { model, isLoading, error } = atlas
onMounted(() => { void atlas.ensureLoaded() })
function fieldName(field: AtlasField): string {
  return field.id === null ? t('skills.atlas.unassigned') : field.name ?? t('skills.atlas.unknownField', { id: field.id })
}
function earned(field: AtlasField): readonly AtlasNode[] {
  return model.value?.nodes.filter(node => node.fieldId === field.id && node.status === 'earned').slice(0, 2) ?? []
}
function expand(field?: AtlasField, node?: AtlasNode): void {
  if (field) atlas.updateView('modal', { focusedFieldId: field.id, unassignedFocused: field.id === null,
    selectedSkillId: node?.skill.id ?? null, camera: null, inspectorCollapsed: false })
  atlas.setModalOpen(true)
}
</script>

<template>
  <section class="sidebar-atlas skill-atlas-colors" :aria-label="t('skills.atlas.title')">
    <template v-if="model">
      <button class="expand" type="button" @click="expand()">{{ t('skills.graph.expandTitle') }} <span aria-hidden="true">↗</span></button>
      <p>{{ t('skills.atlas.fieldCounts', model.counts) }}</p>
      <div class="sidebar-fields">
        <article v-for="(field, index) in model.fields" :key="field.id ?? 'unassigned'" :style="{ '--field': field.id === null ? 'var(--app-muted-foreground)' : `var(--atlas-field-${index % 8})` }">
          <button class="field-name" type="button" @click="expand(field)">{{ fieldName(field) }}</button>
          <p>{{ t('skills.atlas.fieldCounts', field) }}</p>
          <button v-for="node in earned(field)" :key="node.skill.id" class="earned-skill" type="button" @click="expand(field, node)">
            <span dir="auto">{{ node.skill.name }}</span>
            <span><span :aria-label="t('skills.atlas.earned')">✓</span> {{ node.earnedBloom ? `${node.earnedBloom.rank + 1} ${t(`common.bloom.${node.earnedBloom.name}`)}` : t('skills.atlas.levelUnavailable') }}</span>
          </button>
          <p v-if="field.earned > 2">{{ t('skills.atlas.moreEarned', { count: field.earned - 2 }) }}</p>
        </article>
      </div>
    </template>
    <p v-else-if="isLoading" role="status">{{ t('skills.visibility.loading') }}</p>
    <p v-else>{{ t('skills.graph.noSkillsLoaded') }}</p>
    <div v-if="error" role="alert"><p>{{ error }}</p><button type="button" @click="atlas.refresh('retry')">{{ t('common.actions.retry') }}</button></div>
  </section>
</template>

<style scoped>
.sidebar-atlas { min-width: 0; color: var(--app-foreground); font-size: 12px; line-height: 1.5; }
.sidebar-atlas button { text-align: start; min-height: 36px; width: 100%; border-radius: 6px; padding: 6px; overflow-wrap: anywhere; }
.sidebar-atlas button:hover { background: var(--app-muted); }
.sidebar-atlas p { color: var(--app-muted-foreground); font-size: 11px; margin: 4px 6px; }
.expand { display: flex; justify-content: space-between; font-weight: 600; border: 1px solid var(--app-border); }
.sidebar-fields { max-height: 280px; overflow-y: auto; margin-block-start: 8px; }
.sidebar-fields article { border-inline-start: 3px solid var(--field); margin-block: 10px; padding-inline-start: 6px; }
.field-name { font-weight: 600; }
.earned-skill { display: flex; flex-direction: column; gap: 2px; }
.earned-skill > span:last-child { color: var(--app-muted-foreground); }
</style>
