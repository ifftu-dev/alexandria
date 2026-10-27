<script setup lang="ts">
import '@/assets/css/skill-atlas.css'
import { computed, nextTick, onBeforeUnmount, onMounted, ref, useId, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { atlasLayout, NODE_HEIGHT, NODE_WIDTH } from './atlasLayout'
import { useMotion } from '@/composables/useMotion'
import { BLOOM_ORDER } from '@/utils/bloom'
import { createAtlasViewState, type AtlasCamera, type AtlasViewState } from '@/utils/skillAtlasView'
import type { AtlasField, AtlasModel, AtlasNode, BloomRank } from '@/utils/skillAtlas'
import type { AtlasFeedback } from '@/composables/useSkillAtlas'

const props = defineProps<{ model: AtlasModel; view: AtlasViewState; feedback?: AtlasFeedback | null; reducedMotion?: boolean }>()
const emit = defineEmits<{ 'update:view': [view: AtlasViewState]; 'open-skill': [id: string]; 'learn-skill': [id: string]; 'feedback-consumed': [revision: number] }>()
const { t } = useI18n()
const id = useId()
const { reduced: systemReducedMotion } = useMotion()
const motionReduced = computed(() => props.reducedMotion === true || systemReducedMotion.value)
const highlightedIds = ref<readonly string[]>([])
const feedbackRevision = ref<number | null>(null)
let seenFeedbackRevision = props.feedback?.revision ?? -1
let feedbackTimer: ReturnType<typeof setTimeout> | undefined
function finishFeedback(): void {
  if (feedbackTimer !== undefined) clearTimeout(feedbackTimer)
  feedbackTimer = undefined
  highlightedIds.value = []
  const revision = feedbackRevision.value
  feedbackRevision.value = null
  if (revision !== null) emit('feedback-consumed', revision)
}
function visibilityChanged(): void { if (document.visibilityState === 'hidden') finishFeedback() }
watch(() => props.feedback, feedback => {
  if (feedback && feedback.revision <= seenFeedbackRevision) return
  finishFeedback()
  if (!feedback) return
  seenFeedbackRevision = feedback.revision
  if (document.visibilityState === 'hidden') { emit('feedback-consumed', feedback.revision); return }
  highlightedIds.value = [...new Set([...feedback.earnedIds, ...feedback.improvedIds])]
  feedbackRevision.value = feedback.revision
  feedbackTimer = setTimeout(finishFeedback, 650)
})
onMounted(() => {
  // A remount is navigation, never a new achievement. The shared owner also expires feedback.
  if (props.feedback) emit('feedback-consumed', props.feedback.revision)
  document.addEventListener('visibilitychange', visibilityChanged)
})
onBeforeUnmount(() => {
  finishFeedback()
  document.removeEventListener('visibilitychange', visibilityChanged)
})
const layout = computed(() => atlasLayout(props.model))
const nodesById = computed(() => new Map(props.model.nodes.map(node => [node.skill.id, node])))
const selected = computed(() => nodesById.value.get(props.view.selectedSkillId ?? ''))
const fieldFocused = computed(() => props.view.focusedFieldId !== null || props.view.unassignedFocused)
const overview = computed(() => !fieldFocused.value && props.view.camera === null)
const viewport = ref<HTMLElement>()
const width = ref(800)
const height = 520
let resizeObserver: ResizeObserver | undefined
onMounted(() => {
  if (viewport.value && typeof ResizeObserver !== 'undefined') {
    resizeObserver = new ResizeObserver(entries => { width.value = Math.max(280, entries[0]?.contentRect.width ?? 800) })
    resizeObserver.observe(viewport.value)
  }
})
onBeforeUnmount(() => resizeObserver?.disconnect())
const focusedRegion = computed(() => layout.value.regions.find(region => region.field.id === props.view.focusedFieldId))
const camera = computed<AtlasCamera>(() => props.view.camera ?? {
  x: (focusedRegion.value?.x ?? 0) + 400,
  y: (focusedRegion.value?.y ?? 0) + 300,
  zoom: 1,
})
const visibleNodes = computed(() => props.model.nodes.filter(node => {
  const point = layout.value.positions.get(node.skill.id)
  if (!point) return false
  const halfWidth = width.value / camera.value.zoom / 2, halfHeight = height / camera.value.zoom / 2
  return point.x + NODE_WIDTH >= camera.value.x - halfWidth && point.x <= camera.value.x + halfWidth &&
    point.y + NODE_HEIGHT >= camera.value.y - halfHeight && point.y <= camera.value.y + halfHeight
}))
const viewBox = computed(() => `${camera.value.x - width.value / camera.value.zoom / 2} ${camera.value.y - height / camera.value.zoom / 2} ${width.value / camera.value.zoom} ${height / camera.value.zoom}`)
const neighborhoodIds = computed(() => {
  const ids = new Set<string>()
  if (!selected.value || props.view.neighborhood === 'none') return ids
  ids.add(selected.value.skill.id)
  let frontier = [selected.value]
  for (let step = 0; step < props.view.neighborhoodDepth; step++) {
    const next: AtlasNode[] = []
    for (const node of frontier) for (const relation of node[props.view.neighborhood]) {
      if (ids.has(relation.skillId)) continue
      ids.add(relation.skillId)
      const related = nodesById.value.get(relation.skillId)
      if (related) next.push(related)
    }
    frontier = next
  }
  return ids
})
const matches = computed(() => {
  const query = props.view.query.trim().toLocaleLowerCase()
  return props.model.nodes.filter(node => !query || `${node.skill.name} ${node.skill.description ?? ''} ${node.skill.subject_name ?? ''} ${fieldName(node.fieldId)}`.toLocaleLowerCase().includes(query))
})
const bloomMatches = computed(() => matches.value.filter(bloomMatch))
function bloomMatch(node: AtlasNode): boolean { return props.view.bloomEmphasis !== null && node.earnedBloom?.rank === props.view.bloomEmphasis }
const matchIds = computed(() => new Set(matches.value.map(node => node.skill.id)))
function fieldName(fieldId: string | null): string {
  if (fieldId === null) return t('skills.atlas.unassigned')
  return props.model.fields.find(field => field.id === fieldId)?.name ?? t('skills.atlas.unknownField', { id: fieldId })
}
function status(node: AtlasNode): string {
  if (node.status === 'ready') return t('skills.atlas.ready')
  return node.earnedBloom ? `${node.earnedBloom.rank + 1} ${t(`common.bloom.${node.earnedBloom.name}`)}` : t('skills.atlas.levelUnavailable')
}
function color(fieldId: string | null): string { if (fieldId === null) return 'var(--app-muted-foreground)'; return `var(--atlas-field-${layout.value.regions.find(region => region.field.id === fieldId)?.color ?? 0})` }
function update(patch: Partial<AtlasViewState>): void { emit('update:view', { ...props.view, ...patch }) }
function preserveOverviewFocus(): void {
  if (!viewport.value?.querySelector('.atlas-overview')?.contains(document.activeElement)) return
  void nextTick(() => {
    if (document.activeElement === document.body) viewport.value?.querySelector<SVGSVGElement>('.atlas-map')?.focus({ preventScroll: true })
  })
}
function select(skillId: string): void {
  const node = nodesById.value.get(skillId)
  const point = layout.value.positions.get(skillId)
  if (!node || !point) return
  update({ selectedSkillId: skillId, focusedFieldId: node.fieldId, unassignedFocused: node.fieldId === null,
    inspectorCollapsed: false, camera: { x: point.x + NODE_WIDTH / 2, y: point.y + NODE_HEIGHT / 2, zoom: camera.value.zoom } })
  preserveOverviewFocus()
}
function focus(field: AtlasField): void {
  const region = layout.value.regions.find(item => item.field.id === field.id)
  if (!region) return
  update({ focusedFieldId: field.id, unassignedFocused: field.id === null,
    camera: { x: region.x + (width.value < 600 ? 144 : 400), y: region.y + 290, zoom: 1 } })
  preserveOverviewFocus()
}
function dimmed(node: AtlasNode): boolean {
  if (node.skill.id === props.view.selectedSkillId) return false
  return (props.view.query !== '' && !matchIds.value.has(node.skill.id)) ||
    (props.view.bloomEmphasis !== null && node.earnedBloom?.rank !== props.view.bloomEmphasis) ||
    (neighborhoodIds.value.size > 0 && !neighborhoodIds.value.has(node.skill.id))
}
function zoom(delta: number): void { update({ camera: { ...camera.value, zoom: Math.min(1.8, Math.max(1, camera.value.zoom + delta)) } }) }
const worldBounds = computed(() => ({
  x: Math.max(800, ...layout.value.regions.map(region => region.x + region.width)),
  y: Math.max(520, ...layout.value.regions.map(region => region.y + region.height)),
}))
function moveCamera(origin: AtlasCamera, x: number, y: number): void {
  update({ camera: { ...origin,
    x: Math.max(0, Math.min(worldBounds.value.x, origin.x + x / origin.zoom)),
    y: Math.max(0, Math.min(worldBounds.value.y, origin.y + y / origin.zoom)) } })
}
function pan(x: number, y: number): void { moveCamera(camera.value, x, y) }
let drag: { id: number; x: number; y: number; camera: AtlasCamera; moved: boolean; touch: boolean } | null = null
let suppressMapClick = false
const dragging = ref(false)
function pointerDown(event: PointerEvent): void {
  // Native vertical scrolling and browser pinch zoom stay available on touch.
  if (event.isPrimary === false) { drag = null; dragging.value = false; return }
  if (event.button !== 0) return
  suppressMapClick = false
  drag = { id: event.pointerId, x: event.clientX, y: event.clientY, camera: { ...camera.value }, moved: false, touch: event.pointerType === 'touch' }
}
function pointerMove(event: PointerEvent): void {
  if (!drag || event.pointerId !== drag.id) return
  const dx = event.clientX - drag.x, dy = event.clientY - drag.y
  if (!drag.moved && Math.hypot(dx, dy) < 6) return
  if (drag.touch && !drag.moved && Math.abs(dy) > Math.abs(dx)) { drag = null; return }
  drag.moved = true
  dragging.value = true
  suppressMapClick = true
  const map = event.currentTarget as SVGSVGElement
  map.setPointerCapture?.(event.pointerId)
  moveCamera(drag.camera, -dx, drag.touch ? 0 : -dy)
}
function pointerEnd(event: PointerEvent): void {
  if (drag?.id !== event.pointerId) return
  drag = null
  dragging.value = false
  const map = event.currentTarget as SVGSVGElement
  if (map.hasPointerCapture?.(event.pointerId)) map.releasePointerCapture(event.pointerId)
}
function mapClick(event: MouseEvent): void {
  if (suppressMapClick && event.detail !== 0) { event.stopPropagation(); event.preventDefault() }
}
function keyPan(event: KeyboardEvent): void {
  if (event.target !== event.currentTarget) return
  const moves: Record<string, [number, number]> = { ArrowLeft: [-180, 0], ArrowRight: [180, 0], ArrowUp: [0, -180], ArrowDown: [0, 180] }
  const move = moves[event.key]
  if (move) { event.preventDefault(); pan(...move) }
}
function escapeFocus(event: KeyboardEvent): void {
  if (event.key !== 'Escape' || props.view.neighborhood === 'none') return
  event.stopPropagation()
  update({ neighborhood: 'none' })
}
function reset(): void { update(createAtlasViewState()) }
const edges = computed(() => props.model.edges.flatMap((edge, index) => {
  const from = layout.value.positions.get(edge.prerequisite_id)
  const to = layout.value.positions.get(edge.skill_id)
  if (!from || !to) return [] // Unloaded endpoints remain explicit in the inspector.
  const x1 = from.x + NODE_WIDTH, y1 = from.y + NODE_HEIGHT / 2
  const x2 = to.x, y2 = to.y + NODE_HEIGHT / 2
  const bend = Math.max(42, Math.min(160, Math.abs(x2 - x1) / 2))
  return [{ index, path: `M ${x1} ${y1} C ${x1 + bend} ${y1}, ${x2 - bend} ${y2}, ${x2} ${y2}`,
    active: neighborhoodIds.value.size > 0
      ? neighborhoodIds.value.has(edge.prerequisite_id) && neighborhoodIds.value.has(edge.skill_id)
      : edge.prerequisite_id === props.view.selectedSkillId || edge.skill_id === props.view.selectedSkillId,
    label: `${edge.prerequisite_name} → ${edge.skill_name}` }]
}))
function connections(field: AtlasField): AtlasField[] {
  const connected = new Set<string | null>()
  for (const edge of props.model.edges) {
    const from = nodesById.value.get(edge.prerequisite_id), to = nodesById.value.get(edge.skill_id)
    if (!from || !to || from.fieldId === to.fieldId) continue
    if (from.fieldId === field.id) connected.add(to.fieldId)
    if (to.fieldId === field.id) connected.add(from.fieldId)
  }
  return props.model.fields.filter(item => connected.has(item.id))
}
function earnedPreview(field: AtlasField): AtlasNode[] {
  return props.model.nodes.filter(node => node.fieldId === field.id && node.status === 'earned')
    .sort((a, b) => Number(bloomMatch(b)) - Number(bloomMatch(a))).slice(0, 2)
}
function setBloom(event: Event): void {
  const value = (event.target as HTMLSelectElement).value
  update({ bloomEmphasis: value === '' ? null : Number(value) as BloomRank })
}
</script>

<template>
  <section class="atlas skill-atlas-colors" :class="{ 'motion-reduced': motionReduced }" :aria-label="t('skills.atlas.title')" @keydown="escapeFocus">
    <header class="atlas-heading">
      <div><p class="atlas-eyebrow">{{ t('skills.atlas.title') }}</p><h2>{{ t('skills.atlas.fieldCounts', model.counts) }}</h2></div>
      <button type="button" @click="reset">{{ t('skills.atlas.reset') }}</button>
    </header>
    <div class="atlas-toolbar">
      <label class="atlas-search"><span>{{ t('skills.atlas.search') }}</span><input type="search" :value="view.query" @input="update({ query: ($event.target as HTMLInputElement).value })"></label>
      <button v-if="view.query" type="button" @click="update({ query: '' })">{{ t('skills.atlas.clearSearch') }}</button>
      <label><span>{{ t('skills.atlas.bloomLegend') }}</span><select :value="view.bloomEmphasis ?? ''" @change="setBloom"><option value="">{{ t('skills.atlas.allBloom') }}</option><option v-for="(level, rank) in BLOOM_ORDER" :key="level" :value="rank">{{ rank + 1 }} {{ t(`common.bloom.${level}`) }}</option></select></label>
    </div>
    <p class="atlas-legend">{{ t('skills.atlas.legend') }}</p>
    <p role="status" class="atlas-feedback" aria-live="polite"><template v-if="highlightedIds.length">{{ t('skills.atlas.updated') }}: {{ highlightedIds.slice(0, 3).map(skillId => nodesById.get(skillId)?.skill.name).filter(Boolean).join(', ') }}{{ highlightedIds.length > 3 ? '…' : '' }}</template></p>
    <div class="atlas-workspace">
      <div class="atlas-main">
        <p v-if="view.bloomEmphasis !== null" class="bloom-summary" role="status">{{ t('skills.atlas.bloomSummary', { level: `${view.bloomEmphasis + 1} ${t(`common.bloom.${BLOOM_ORDER[view.bloomEmphasis]}`)}`, count: bloomMatches.length }) }}</p>
        <nav class="atlas-fields" :aria-label="t('skills.atlas.overview')">
          <button type="button" :aria-pressed="overview" @click="update({ focusedFieldId: null, unassignedFocused: false, camera: null, neighborhood: 'none' })">{{ t('skills.atlas.overview') }}</button>
          <button v-for="field in model.fields" :key="field.id === null ? 'unassigned' : `field:${field.id}`" type="button" :style="{ '--field': color(field.id) }" :aria-pressed="fieldFocused && field.id === view.focusedFieldId" @click="focus(field)"><i class="field-dot" aria-hidden="true" />{{ fieldName(field.id) }}</button>
        </nav>
        <div ref="viewport" class="atlas-viewport">
          <div v-if="overview" class="atlas-overview">
            <p class="atlas-overview-hint">{{ t('skills.atlas.overviewHint') }}</p>
            <article v-for="region in layout.regions" :key="region.field.id === null ? 'unassigned' : `field:${region.field.id}`" class="field-card" :style="{ '--field': color(region.field.id) }">
              <button class="field-title" type="button" :aria-label="t('skills.atlas.focusField', { field: fieldName(region.field.id) })" @click="focus(region.field)"><span>{{ fieldName(region.field.id) }}</span><span aria-hidden="true">↗</span></button>
              <p>{{ t('skills.atlas.fieldCounts', { ...region.field }) }}</p>
              <div class="field-progress" aria-hidden="true"><span :style="{ width: `${region.field.earned / region.field.total * 100}%` }" /></div>
              <button v-for="node in earnedPreview(region.field)" :key="node.skill.id" class="field-earned" :class="{ 'is-bloom-match': bloomMatch(node) }" type="button" :aria-label="`${node.skill.name} · ${status(node)}`" @click="select(node.skill.id)"><span class="skill-name" dir="auto">{{ node.skill.name }}</span><span><span aria-hidden="true">✓ </span>{{ status(node) }}</span></button>
              <p v-if="region.field.earned > 2" class="atlas-muted">{{ t('skills.atlas.moreEarned', { count: region.field.earned - 2 }) }}</p>
              <div v-if="connections(region.field).length" class="field-connections"><span>{{ t('skills.atlas.fieldConnections') }}</span><button v-for="other in connections(region.field)" :key="other.id === null ? 'unassigned' : `field:${other.id}`" type="button" @click="focus(other)">{{ fieldName(other.id) }} <span aria-hidden="true">↗</span></button></div>
            </article>
            <p v-if="!model.nodes.length">{{ t('skills.atlas.empty') }}</p>
          </div>
          <template v-else>
            <div class="map-tools" :aria-label="t('skills.atlas.map')">
              <button type="button" :aria-label="t('skills.atlas.zoomIn')" :disabled="camera.zoom >= 1.8" @click="zoom(.2)">+</button>
              <button type="button" :aria-label="t('skills.atlas.zoomOut')" :disabled="camera.zoom <= 1" @click="zoom(-.2)">−</button>
              <span>{{ Math.round(camera.zoom * 100) }}%</span>
              <button type="button" :aria-label="t('skills.atlas.panLeft')" @click="pan(-180, 0)">←</button>
              <button type="button" :aria-label="t('skills.atlas.panUp')" @click="pan(0, -180)">↑</button>
              <button type="button" :aria-label="t('skills.atlas.panDown')" @click="pan(0, 180)">↓</button>
              <button type="button" :aria-label="t('skills.atlas.panRight')" @click="pan(180, 0)">→</button>
            </div>
            <svg class="atlas-map" :viewBox="viewBox" :height="height" tabindex="0" role="group" :aria-label="t('skills.atlas.map')" :aria-describedby="`${id}-map-help`" @keydown="keyPan" :class="{ dragging }" @pointerdown="pointerDown" @pointermove="pointerMove" @pointerup="pointerEnd" @pointercancel="pointerEnd" @lostpointercapture="pointerEnd" @click.capture="mapClick">
              <defs><marker :id="`${id}-arrow`" viewBox="0 0 8 8" refX="8" refY="4" markerWidth="7" markerHeight="7" orient="auto-start-reverse"><path d="M 0 0 L 8 4 L 0 8 z" fill="context-stroke" /></marker></defs>
              <g v-for="region in layout.regions" :key="region.field.id === null ? 'unassigned' : `field:${region.field.id}`" :style="{ '--field': color(region.field.id) }">
                <rect :x="region.x" :y="region.y" :width="region.width" :height="region.height" rx="24" class="map-region" />
                <foreignObject :x="region.x + 32" :y="region.y + 18" width="736" height="72"><div class="region-heading"><strong>{{ fieldName(region.field.id) }}</strong><span>{{ t('skills.atlas.fieldCounts', { ...region.field }) }}</span></div></foreignObject>
              </g>
              <path v-for="edge in edges" :key="edge.index" :d="edge.path" class="map-edge" :class="{ 'map-edge-active': edge.active }" :marker-end="`url(#${id}-arrow)`"><title>{{ edge.label }}</title></path>
              <foreignObject v-for="node in visibleNodes" :key="node.skill.id" :x="layout.positions.get(node.skill.id)?.x" :y="layout.positions.get(node.skill.id)?.y" :width="NODE_WIDTH" :height="NODE_HEIGHT" :style="{ '--field': color(node.fieldId) }">
                <button class="map-node" :class="{ 'is-selected': node.skill.id === view.selectedSkillId, 'is-bloom-match': bloomMatch(node), 'is-dimmed': dimmed(node), 'is-updated': highlightedIds.includes(node.skill.id) }" type="button" :aria-label="`${node.skill.name} · ${status(node)}`" :aria-pressed="node.skill.id === view.selectedSkillId" @click="select(node.skill.id)">
                  <span class="skill-name" dir="auto">{{ node.skill.name }}</span><span class="node-evidence"><span v-if="node.status === 'earned'" :aria-label="t('skills.atlas.earned')">✓</span><span v-else aria-hidden="true">○</span><span>{{ status(node) }}</span></span>
                </button>
              </foreignObject>
            </svg>
            <p :id="`${id}-map-help`" class="map-help">{{ t('skills.atlas.mapHelp') }}</p>
          </template>
        </div>
        <details class="atlas-list" open>
          <summary>{{ t('skills.atlas.list') }} <span>{{ t('skills.atlas.results', { count: matches.length }) }}</span></summary>
          <ul :aria-label="t('skills.atlas.list')"><li v-for="node in matches" :key="node.skill.id"><button :class="{ 'is-bloom-match': bloomMatch(node) }" type="button" :style="{ '--field': color(node.fieldId) }" :aria-label="`${node.skill.name} · ${status(node)}`" :aria-pressed="node.skill.id === view.selectedSkillId" @click="select(node.skill.id)"><i class="field-dot" aria-hidden="true" /><span><strong dir="auto">{{ node.skill.name }}</strong><small>{{ fieldName(node.fieldId) }}</small></span><span class="list-evidence"><span v-if="node.status === 'earned'" :aria-label="t('skills.atlas.earned')">✓ </span>{{ status(node) }}</span></button></li></ul>
          <p v-if="!matches.length">{{ t('skills.atlas.noResults') }}</p>
        </details>
      </div>
      <aside class="atlas-inspector" :class="{ 'is-collapsed': view.inspectorCollapsed }" :aria-label="t('skills.atlas.inspector')">
        <header><span>{{ t('skills.atlas.inspector') }}</span><button type="button" :aria-expanded="!view.inspectorCollapsed" :aria-controls="`${id}-inspector`" @click="update({ inspectorCollapsed: !view.inspectorCollapsed })">{{ t(view.inspectorCollapsed ? 'skills.atlas.expand' : 'skills.atlas.collapse') }}</button></header>
        <div :id="`${id}-inspector`" v-show="!view.inspectorCollapsed" class="inspector-content">
          <template v-if="selected">
            <p class="inspector-field" :style="{ '--field': color(selected.fieldId) }"><i class="field-dot" aria-hidden="true" />{{ fieldName(selected.fieldId) }}</p>
            <h2 dir="auto">{{ selected.skill.name }}</h2>
            <p class="inspector-status"><span v-if="selected.status === 'earned'" aria-hidden="true">✓ </span>{{ t(`skills.atlas.${selected.status}`) }}</p>
            <p dir="auto">{{ selected.skill.description }}</p>
            <dl><template v-if="selected.status === 'earned'"><dt>{{ t('skills.atlas.currentBloom') }}</dt><dd>{{ status(selected) }}</dd></template><dt>{{ t('skills.atlas.intrinsicBloom') }}</dt><dd>{{ selected.intrinsicBloom ? t(`common.bloom.${selected.intrinsicBloom}`) : t('skills.atlas.unavailable') }}</dd></dl>
            <div class="inspector-actions"><button class="atlas-primary" type="button" :aria-label="t('skills.atlas.open')" @click="emit('open-skill', selected.skill.id)">{{ t('skills.atlas.open') }} <span aria-hidden="true">↗</span></button><button type="button" @click="emit('learn-skill', selected.skill.id)">{{ t('skills.atlas.learn') }}</button></div>
            <p v-if="selected.prerequisitesSatisfied" class="atlas-muted">{{ t('skills.atlas.satisfied') }}</p>
            <p v-else-if="selected.prerequisites.some(relation => !relation.loaded) || selected.prerequisites.length < selected.skill.prerequisite_count" class="atlas-muted">{{ t('skills.atlas.incomplete') }}</p>
            <div class="neighborhood-controls"><button type="button" :aria-pressed="view.neighborhood === 'prerequisites'" @click="update({ neighborhood: view.neighborhood === 'prerequisites' ? 'none' : 'prerequisites' })">{{ t('skills.atlas.focusPrerequisites') }}</button><button type="button" :aria-pressed="view.neighborhood === 'dependents'" @click="update({ neighborhood: view.neighborhood === 'dependents' ? 'none' : 'dependents' })">{{ t('skills.atlas.focusDependents') }}</button><button v-if="view.neighborhood !== 'none'" type="button" @click="update({ neighborhood: 'none' })">{{ t('skills.atlas.allConnections') }}</button><label v-if="view.neighborhood !== 'none'"><span>{{ t('skills.atlas.depth') }}</span><select :value="view.neighborhoodDepth" @change="update({ neighborhoodDepth: Number(($event.target as HTMLSelectElement).value) === 2 ? 2 : 1 })"><option :value="1">{{ t('skills.atlas.oneStep') }}</option><option :value="2">{{ t('skills.atlas.twoSteps') }}</option></select></label></div>
            <section v-for="direction in (['prerequisites', 'dependents'] as const)" :key="direction" class="inspector-relations">
              <h3>{{ t(`skills.atlas.${direction}`) }} <span>{{ selected[direction].length }}</span></h3>
              <ul><li v-for="relation in selected[direction]" :key="relation.skillId"><button v-if="relation.loaded" type="button" :aria-label="t('skills.atlas.inspect', { name: relation.name })" @click="select(relation.skillId)"><strong dir="auto">{{ relation.name }}</strong><small>{{ fieldName(nodesById.get(relation.skillId)?.fieldId ?? null) }} · {{ relation.earned ? t('skills.atlas.earned') : t('skills.atlas.ready') }}</small></button><p v-else>{{ relation.name }} · {{ t('skills.atlas.notLoaded') }}</p></li></ul>
              <p v-if="!selected[direction].length" class="atlas-muted">{{ t('skills.atlas.noRelations') }}</p>
            </section>
          </template>
          <p v-else class="inspector-prompt">{{ t('skills.atlas.selectPrompt') }}</p>
        </div>
      </aside>
    </div>
  </section>
</template>

<style scoped>
.atlas { color: var(--app-foreground); font-size: 14px; line-height: 1.5; min-width: 0; }
.atlas button, .atlas input, .atlas select { font: inherit; color: inherit; }
.atlas button { cursor: pointer; min-height: 44px; padding: 8px 12px; border: 1px solid var(--app-border); border-radius: 9px; background: var(--app-card); text-align: start; overflow-wrap: anywhere; }
.atlas button:hover { background: var(--app-muted); }
.atlas button:focus-visible, .atlas input:focus-visible, .atlas select:focus-visible, .atlas summary:focus-visible, .atlas-map:focus-visible { outline: 3px solid var(--app-ring); outline-offset: 2px; }
.atlas button:disabled { opacity: .45; cursor: default; }
.atlas button[aria-pressed=true] { border-color: var(--app-primary); box-shadow: inset 0 0 0 1px var(--app-primary); }
.atlas-feedback:not(:empty) { padding: 10px 14px; margin-block-end: 12px; border-inline-start: 3px solid var(--app-primary); background: var(--app-muted); border-radius: 8px; }
.atlas .map-node.is-updated { outline: 2px solid var(--app-primary); outline-offset: -2px; }
.atlas:not(.motion-reduced) .map-node.is-updated { animation: atlas-evidence 800ms ease-out 1; }
@keyframes atlas-evidence { from { box-shadow: inset 0 0 0 5px var(--app-primary); } to { box-shadow: inset 0 0 0 0 transparent; } }
@media (prefers-reduced-motion: reduce) { .atlas .map-node.is-updated { animation: none !important; } }
.atlas-heading { display: flex; align-items: center; justify-content: space-between; gap: 16px; margin-block-end: 20px; }
.atlas-eyebrow { font-size: 12px; letter-spacing: .12em; text-transform: uppercase; color: var(--app-muted-foreground); margin-block-end: 4px; }
.atlas-heading h2 { font-size: 22px; font-weight: 650; letter-spacing: -.025em; }
.atlas-toolbar { display: flex; flex-wrap: wrap; gap: 14px; }
.atlas label { display: flex; flex-direction: column; gap: 6px; font-size: 12px; color: var(--app-muted-foreground); }
.atlas input, .atlas select { min-height: 44px; border: 1px solid var(--app-input); background: var(--app-card); border-radius: 9px; padding: 8px 12px; font-size: 14px; color: var(--app-foreground); min-width: 0; }
.atlas-search { flex: 1; min-width: 200px; }
.atlas-legend { color: var(--app-muted-foreground); font-size: 12px; margin-block: 12px 18px; max-width: 85ch; }
.atlas-workspace { display: grid; grid-template-columns: minmax(0, 1fr) 300px; gap: 18px; align-items: start; }
.atlas-main { min-width: 0; }
.atlas-fields { display: flex; flex-wrap: wrap; gap: 6px; margin-block-end: 12px; }
.atlas-fields button { font-size: 12px; display: flex; align-items: center; gap: 7px; }
.field-dot { display: inline-block; width: 9px; height: 9px; background: var(--field); border-radius: 50%; flex-shrink: 0; }
.atlas-viewport { border: 1px solid var(--app-border); border-radius: 18px; overflow: hidden; background: var(--app-background); }
.atlas-overview { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 16px; padding: 20px; max-height: 680px; overflow: auto; }
.atlas-overview-hint { grid-column: 1 / -1; color: var(--app-muted-foreground); font-size: 13px; }
.field-card { background: var(--app-card); border: 1px solid var(--app-border); border-block-start: 3px solid var(--field); border-radius: 12px; padding: 14px; min-width: 0; }
.atlas .field-title { border: 0; padding: 0; display: flex; justify-content: space-between; align-items: start; gap: 8px; width: 100%; color: var(--field); font-weight: 650; font-size: 18px; background: transparent; }
.field-card > p { font-size: 12px; color: var(--app-muted-foreground); margin-block: 6px; }
.field-progress { height: 3px; background: var(--app-muted); border-radius: 4px; margin-block: 14px; overflow: hidden; }
.field-progress span { display: block; height: 100%; background: var(--field); }
.atlas .field-earned { display: flex; flex-direction: column; gap: 5px; width: 100%; padding: 9px 0; border: 0; border-radius: 4px; background: transparent; }
.field-earned > span:last-child { color: var(--field); font-size: 12px; font-weight: 600; }
.skill-name { display: -webkit-box; -webkit-line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden; overflow-wrap: anywhere; font-weight: 600; }
.field-connections { display: flex; flex-wrap: wrap; gap: 4px; margin-block-start: 10px; border-block-start: 1px solid var(--app-border); padding-block-start: 8px; }
.field-connections > span { width: 100%; font-size: 11px; color: var(--app-muted-foreground); }
.atlas .field-connections button { font-size: 11px; padding: 4px 7px; border: 0; }
.map-tools { display: flex; flex-wrap: wrap; align-items: center; gap: 4px; padding: 8px; border-block-end: 1px solid var(--app-border); }
.map-tools span { font-size: 12px; margin-inline: 6px; }
.map-tools button { min-width: 44px; text-align: center; }
.atlas-map { touch-action: pan-y pinch-zoom; cursor: grab; user-select: none; display: block; width: 100%; overflow: hidden; background-image: radial-gradient(var(--app-border) .7px, transparent .7px); background-size: 18px 18px; }
.map-region { fill: color-mix(in srgb, var(--field) 5%, var(--app-background)); stroke: color-mix(in srgb, var(--field) 40%, var(--app-border)); stroke-width: 1; }
.region-heading { display: flex; flex-direction: column; gap: 4px; color: var(--field); }
.region-heading strong { font-size: 22px; font-weight: 650; }
.region-heading span { font-size: 12px; color: var(--app-muted-foreground); }
.map-edge { fill: none; stroke: var(--app-muted-foreground); stroke-width: 1.5; opacity: .28; }
.map-edge-active { stroke: var(--app-primary); stroke-width: 2.5; opacity: .9; }
.atlas .map-node { box-sizing: border-box; margin: 4px; width: calc(100% - 8px); height: calc(100% - 8px); border-inline-start: 4px solid var(--field); border-radius: 12px; display: flex; flex-direction: column; justify-content: space-between; gap: 6px; padding: 10px 12px; background: var(--app-card); font-size: 14px; line-height: 1.35; }
.node-evidence { display: flex; gap: 6px; color: var(--field); font-size: 13px; font-weight: 650; }
.map-node.is-dimmed .skill-name { color: var(--app-muted-foreground); font-weight: 400; }
.atlas .map-node.is-selected { border-color: var(--app-primary); box-shadow: inset 0 0 0 2px var(--app-primary); }
.atlas-map.dragging, .atlas-map.dragging .map-node { cursor: grabbing; }
.map-help { padding: 10px 14px; color: var(--app-muted-foreground); font-size: 12px; border-block-start: 1px solid var(--app-border); }
.atlas-list { margin-block-start: 16px; border: 1px solid var(--app-border); border-radius: 12px; background: var(--app-card); }
.atlas-list summary { cursor: pointer; padding: 14px; font-weight: 600; }
.atlas-list summary span { display: inline-block; margin-inline-start: 10px; font-size: 12px; font-weight: 400; color: var(--app-muted-foreground); }
.atlas ul { list-style: none; padding: 0; margin: 0; }
.atlas-list ul { max-height: 330px; overflow: auto; padding: 0 8px 8px; }
.atlas .atlas-list li button { width: 100%; display: flex; align-items: center; gap: 12px; border: 0; border-block-start: 1px solid var(--app-border); border-radius: 0; }
.atlas-list strong, .atlas-list small { display: block; }
.atlas-list strong { font-size: 13px; font-weight: 550; }
.atlas-list small { color: var(--app-muted-foreground); font-size: 11px; }
.list-evidence { margin-inline-start: auto; font-size: 12px; flex-shrink: 0; max-width: 42%; color: var(--field); }
.bloom-summary { margin-block-end: 12px; padding: 10px 12px; border-inline-start: 3px solid var(--app-primary); background: var(--app-muted); border-radius: 8px; font-size: 13px; }
.atlas button.is-bloom-match { background: var(--app-muted); outline: 2px solid var(--app-primary); outline-offset: -2px; }
.atlas .field-earned.is-bloom-match { padding-inline: 8px; }
.atlas button.is-bloom-match:focus-visible { outline-offset: 2px; outline-width: 3px; }
.atlas-inspector { background: var(--app-card); border: 1px solid var(--app-border); border-radius: 16px; position: sticky; top: 12px; overflow: hidden; }
.atlas-inspector > header { display: flex; justify-content: space-between; align-items: center; gap: 8px; padding: 10px 14px; border-block-end: 1px solid var(--app-border); font-size: 11px; color: var(--app-muted-foreground); }
.atlas-inspector > header button { font-size: 11px; padding: 5px 7px; }
.inspector-content { padding: 20px; max-height: min(760px, calc(100dvh - 100px)); overflow: auto; overflow-wrap: anywhere; }
.inspector-field { color: var(--field); display: flex; align-items: center; gap: 7px; font-size: 12px; }
.inspector-content h2 { font-size: 24px; line-height: 1.2; letter-spacing: -.03em; font-weight: 650; margin-block: 14px 10px; }
.inspector-status { font-weight: 650; margin-block-end: 10px; }
.inspector-content > p { font-size: 13px; }
.inspector-content dl { background: var(--app-background); border-radius: 10px; padding: 12px; margin-block: 18px; }
.inspector-content dt { color: var(--app-muted-foreground); font-size: 11px; }
.inspector-content dd { font-size: 15px; font-weight: 600; margin-block: 4px 10px; }
.inspector-content dd:last-child { margin-block-end: 0; }
.inspector-actions, .neighborhood-controls { display: flex; flex-direction: column; gap: 8px; margin-block: 14px; }
.atlas .atlas-primary { background: var(--app-primary); color: var(--app-primary-foreground); border-color: var(--app-primary); display: flex; justify-content: space-between; }
.neighborhood-controls button { font-size: 12px; }
.inspector-relations { border-block-start: 1px solid var(--app-border); padding-block-start: 14px; margin-block-start: 16px; }
.inspector-relations h3 { display: flex; justify-content: space-between; font-size: 13px; font-weight: 650; }
.atlas .inspector-relations button { width: 100%; border: 0; padding: 10px 0; }
.inspector-relations strong, .inspector-relations small { display: block; }
.inspector-relations strong { font-size: 13px; font-weight: 550; }
.inspector-relations small, .atlas-muted, .inspector-prompt { color: var(--app-muted-foreground); font-size: 12px; }
.inspector-relations p { font-size: 12px; margin-block-start: 8px; }
@media (max-width: 1000px) { .atlas-workspace { grid-template-columns: minmax(0, 1fr) 270px; gap: 12px; } .atlas-overview { grid-template-columns: 1fr; padding: 14px; } }
@media (max-width: 700px) {
  .atlas-workspace { display: flex; flex-direction: column; }
  .atlas-main, .atlas-inspector { width: 100%; }
  .atlas-heading { align-items: start; }
  .atlas-heading h2 { font-size: 18px; }
  .atlas-heading > button { font-size: 12px; }
  .atlas-toolbar > label { width: 100%; }
  .atlas-fields { flex-wrap: nowrap; overflow-x: auto; padding-block-end: 4px; }
  .atlas-fields button { flex-shrink: 0; max-width: 230px; }
  .atlas-overview { max-height: 560px; }
  .atlas-inspector { position: sticky; top: auto; bottom: 0; z-index: 2; border-radius: 18px 18px 0 0; box-shadow: 0 -4px 22px color-mix(in srgb, var(--app-foreground) 10%, transparent); }
  .inspector-content { max-height: 45vh; padding: 16px; }
  .atlas-inspector.is-collapsed { margin-block-start: 0; }
}
</style>
