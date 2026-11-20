<script setup lang="ts">
/**
 * The sidebar's skill graph: the Web graph in compact form, fully working in
 * place. It shares the expanded graph's view state (`'modal'`) for selection,
 * field focus and the Names, Arrows and Related toggles, and the dragged dots
 * of every Web surface; it keeps its own camera.
 */
import { computed, onMounted, useId } from 'vue'
import { useI18n } from 'vue-i18n'
import { atlasFieldColor, createAtlasPresenter } from '@/components/skills/atlasPresentation'
import WebGraph from '@/components/skills/WebGraph.vue'
import { useSkillAtlas } from '@/composables/useSkillAtlas'
import { useSkillAtlasActions } from '@/composables/useSkillAtlasActions'
import type { AtlasField, AtlasNode } from '@/utils/skillAtlas'

const { t } = useI18n()
const present = createAtlasPresenter(t)
const atlas = useSkillAtlas()
const { model, isLoading, error, modalView: view, webPositions, sidebarWebCamera } = atlas
// Open skill returns to the expanded graph only when it was open, so from here it returns to the sidebar as it was.
const { openSkill, actionError } = useSkillAtlasActions('modal')
onMounted(() => { void atlas.ensureLoaded() })
const summaryId = useId()
const summary = computed(() => model.value ? t('skills.atlas.sidebar.summary', model.value.counts) : '')
const selected = computed<AtlasNode | null>(() => {
  const id = view.value.selectedSkillId
  return id ? model.value?.nodeById.get(id) ?? null : null
})
function stateWords(node: AtlasNode): string {
  return node.status === 'earned' ? t('skills.atlas.sidebar.captionEarned', { level: present.evidence(node) }) : present.state(node)
}
function expand(): void { atlas.setModalOpen(true) }
/** Expand from the caption: the expanded graph opens on the selection with its inspector showing. */
function expandSelected(): void {
  atlas.updateView('modal', { inspectorCollapsed: false })
  atlas.setModalOpen(true)
}
function select(id: string | null): void { atlas.updateView('modal', { selectedSkillId: id }) }
function fieldKey(field: AtlasField): string { return field.id ?? 'unassigned' }
</script>

<template>
  <section class="sidebar-atlas" :aria-label="t('skills.atlas.title')">
    <template v-if="model">
      <button class="expand" type="button" @click="expand()">{{ t('skills.graph.expandTitle') }} <span aria-hidden="true">↗</span></button>
      <div class="sidebar-web">
        <WebGraph compact :model="model" :positions="webPositions" :label="t('skills.atlas.sidebar.graph')" :described-by="summaryId"
          :selected-id="view.selectedSkillId" :names="view.webNames" :arrows="view.webArrows" :related="view.webRelated" :camera="sidebarWebCamera"
          @select="select" @update:camera="atlas.setSidebarWebCamera" @drag="atlas.setWebPositions" />
        <p :id="summaryId" class="sr-only">{{ summary }}</p>
      </div>
      <div class="sidebar-caption">
        <template v-if="selected">
          <p class="caption-skill"><span class="caption-name" dir="auto">{{ selected.skill.name }}</span> <span>{{ stateWords(selected) }}</span></p>
          <div class="caption-actions">
            <button type="button" :aria-label="t('skills.atlas.sidebar.openLabel', { name: selected.skill.name })" @click="openSkill(selected.skill.id)">{{ t('skills.atlas.open') }}</button>
            <button type="button" :aria-label="t('skills.atlas.sidebar.expandLabel', { name: selected.skill.name })" @click="expandSelected">{{ t('skills.atlas.sidebar.expand') }} <span aria-hidden="true">↗</span></button>
          </div>
        </template>
        <p v-else>{{ t('skills.atlas.fieldCounts', model.counts) }}</p>
        <p v-if="actionError" role="alert">{{ actionError }}</p>
      </div>
      <ul class="sidebar-fields">
        <li v-for="field in model.fields" :key="fieldKey(field)" :style="{ '--field': atlasFieldColor(field) }">
          <button class="field-name" type="button">
            <i class="field-dot" aria-hidden="true" /><span dir="auto">{{ present.fieldName(field) }}</span>
            <small>{{ t('skills.atlas.fieldEarned', { earned: field.earned, total: field.total }) }}</small>
          </button>
        </li>
      </ul>
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
.sidebar-web { position: relative; margin-block-start: 6px; border: 1px solid var(--app-border); border-radius: 8px; overflow: hidden; --web-graph-height: 180px; }
.sidebar-fields { list-style: none; margin: 4px 0 0; padding: 0; }
.field-name { display: flex; align-items: center; gap: 6px; }
.field-name > span { flex: 1; min-width: 0; font-weight: 600; }
.field-name small { color: var(--app-muted-foreground); font-size: 11px; white-space: nowrap; }
.field-dot { flex: none; width: 8px; height: 8px; border-radius: 50%; background: var(--field); }
</style>
