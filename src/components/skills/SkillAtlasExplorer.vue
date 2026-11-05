<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, shallowRef, useId, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { atlasLayout, MAP_HEIGHT, NODE_HEIGHT, NODE_WIDTH, REGION_HEADING_BOX, REGION_WIDTH, regionCamera } from './atlasLayout'
import { atlasFieldColor, createAtlasPresenter } from './atlasPresentation'
import AtlasNodeLayer from './AtlasNodeLayer.vue'
import { motionDuration, useMotion } from '@/composables/useMotion'
import { BLOOM_ORDER } from '@/utils/bloom'
import { createAtlasViewState, type AtlasCamera, type AtlasViewState } from '@/utils/skillAtlasView'
import { isBloomRank, type AtlasField, type AtlasModel, type AtlasNode } from '@/utils/skillAtlas'
import type { AtlasFeedback } from '@/composables/useSkillAtlas'

const props = defineProps<{ model: AtlasModel; view: AtlasViewState; feedback?: AtlasFeedback | null; reducedMotion?: boolean }>()
const emit = defineEmits<{ 'update:view': [view: AtlasViewState]; 'open-skill': [id: string]; 'learn-skill': [id: string]; 'feedback-consumed': [revision: number] }>()
const { t, locale } = useI18n()
const present = createAtlasPresenter(t)
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
  // Same token as the atlas-evidence animation, so the highlight is never cut short.
  feedbackTimer = setTimeout(finishFeedback, motionDuration('reveal'))
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
const nodesById = computed(() => props.model.nodeById)
// One computed per view field: a pan replaces the view object, and only what
// reads the camera should recompute (a computed whose value is unchanged
// does not wake its dependents).
const selectedId = computed(() => props.view.selectedSkillId)
const neighborhood = computed(() => props.view.neighborhood)
const neighborhoodDepth = computed(() => props.view.neighborhoodDepth)
const rawQuery = computed(() => props.view.query)
const bloomEmphasis = computed(() => props.view.bloomEmphasis)
const focusedFieldId = computed(() => props.view.focusedFieldId)
const selected = computed(() => nodesById.value.get(selectedId.value ?? ''))
const fieldFocused = computed(() => focusedFieldId.value !== null || props.view.unassignedFocused)
const overview = computed(() => !fieldFocused.value && props.view.camera === null)
const viewport = ref<HTMLElement>()
const width = ref(800)
const height = MAP_HEIGHT
/** How far one arrow press or pan button moves the map, in map units. */
const PAN_STEP = 180
let resizeObserver: ResizeObserver | undefined
onMounted(() => {
  if (viewport.value && typeof ResizeObserver !== 'undefined') {
    resizeObserver = new ResizeObserver(entries => { width.value = Math.max(280, entries[0]?.contentRect.width ?? 800) })
    resizeObserver.observe(viewport.value)
  }
})
onBeforeUnmount(() => resizeObserver?.disconnect())
const focusedRegion = computed(() => layout.value.regions.find(region => region.field.id === focusedFieldId.value))
const camera = computed<AtlasCamera>(() => props.view.camera ?? regionCamera(focusedRegion.value ?? { x: 0, y: 0 }, width.value))
interface MapRect { left: number; top: number; right: number; bottom: number }
/** The map rectangle, in map units, that a box of this many pixels shows around a camera. */
function rectAround(at: AtlasCamera, pixelWidth: number, pixelHeight: number): MapRect {
  const w = pixelWidth / at.zoom / 2, h = pixelHeight / at.zoom / 2
  return { left: at.x - w, top: at.y - h, right: at.x + w, bottom: at.y + h }
}
function overlaps(rect: MapRect, left: number, top: number, right: number, bottom: number): boolean {
  return right >= rect.left && left <= rect.right && bottom >= rect.top && top <= rect.bottom
}
/** The part of the map on screen. Only nodes on it take focus or reach assistive technology. */
const visibleRect = computed(() => rectAround(camera.value, width.value, height))
function onScreen(node: AtlasNode): boolean {
  const point = layout.value.positions.get(node.skill.id)
  return point !== undefined && overlaps(visibleRect.value, point.x, point.y, point.x + NODE_WIDTH, point.y + NODE_HEIGHT)
}
// The map is one world-sized canvas that moves under the screen with a
// compositor transform, so panning restyles and repaints nothing. Nodes are
// drawn for the screen plus a margin. The drawn area moves when the screen
// comes within half a margin of its edge, so the nodes it adds are still off
// screen: they join the canvas one at a time when the main thread is idle,
// and a pan never waits for them.
/** How far past each edge of the screen the map is drawn, in pixels. */
const DRAWN_MARGIN = 360
/** How many off-screen nodes join the canvas at a time. */
const NODES_PER_FRAME = 1
function drawnAround(at: AtlasCamera): MapRect & { zoom: number; width: number } {
  return { ...rectAround(at, width.value + 2 * DRAWN_MARGIN, height + 2 * DRAWN_MARGIN), zoom: at.zoom, width: width.value }
}
const drawn = ref(drawnAround(camera.value))
function keepDrawn(at: AtlasCamera): void {
  const area = drawn.value, slack = DRAWN_MARGIN / 2, near = rectAround(at, width.value + 2 * slack, height + 2 * slack)
  if (area.zoom === at.zoom && area.width === width.value && near.left >= area.left && near.right <= area.right &&
    near.top >= area.top && near.bottom <= area.bottom) return
  drawn.value = drawnAround(at)
}
watch([camera, width], () => keepDrawn(camera.value))
// Canvas upkeep runs in a task of its own when the main thread is idle, never
// inside a pointer event or a frame, so the frame that shows it does only the
// rendering. (Safari has no idle callbacks, so there it is the next task.)
const whenIdle = (run: () => void): void => {
  if (typeof requestIdleCallback === 'function') requestIdleCallback(run, { timeout: 100 })
  else window.setTimeout(run, 0)
}
// During a drag the drawn area follows when idle: it moves half a margin
// early, so nothing on screen waits for it.
let followScheduled = false
function followDrag(): void {
  if (followScheduled) return
  followScheduled = true
  whenIdle(() => { followScheduled = false; if (drag?.target) keepDrawn(drag.target) })
}
function placed(node: AtlasNode, rect: MapRect): boolean {
  const point = layout.value.positions.get(node.skill.id)
  return point !== undefined && overlaps(rect, point.x, point.y, point.x + NODE_WIDTH, point.y + NODE_HEIGHT)
}
const drawnNodes = computed(() => props.model.nodes.filter(node => placed(node, drawn.value)))
/**
 * The drawn nodes on the canvas: every one on screen, and the rest one at a
 * time. Ordered by arrival, so a node that joins goes last and the memoised
 * nodes before it keep their places (Vue matches a memoised list by position).
 */
const canvasIds = shallowRef<readonly string[]>(drawnNodes.value.map(node => node.skill.id))
const canvasNodes = computed(() => canvasIds.value.flatMap(id => { const node = nodesById.value.get(id); return node ? [node] : [] }))
// Read through a getter by the node layer, so adding a node re-renders that
// layer and not the whole explorer.
const nodeLayer = { get items(): readonly AtlasNode[] { return canvasNodes.value } }
let fillScheduled = false
/** Drops nodes that left the drawn area and adds those on screen; with `more`, also a few of the rest. */
function fillCanvas(more: boolean): void {
  const screen = rectAround(drag?.target ?? camera.value, width.value, height)
  const wanted = new Set(drawnNodes.value.map(node => node.skill.id))
  const next = canvasIds.value.filter(id => wanted.has(id)), kept = new Set(next), waiting: string[] = []
  for (const node of drawnNodes.value) {
    if (kept.has(node.skill.id)) continue
    if (placed(node, screen)) next.push(node.skill.id)
    else waiting.push(node.skill.id)
  }
  const added = more ? NODES_PER_FRAME : 0
  if (added > 0) {
    // Nearest the screen first: those are the next to scroll into view.
    const centreX = (screen.left + screen.right) / 2, centreY = (screen.top + screen.bottom) / 2
    const distance = (id: string) => { const point = layout.value.positions.get(id)!; return Math.hypot(point.x + NODE_WIDTH / 2 - centreX, point.y + NODE_HEIGHT / 2 - centreY) }
    waiting.sort((a, b) => distance(a) - distance(b))
    next.push(...waiting.slice(0, added))
  }
  if (next.length !== canvasIds.value.length || next.some((id, index) => id !== canvasIds.value[index])) canvasIds.value = next
  if (waiting.length > added && !fillScheduled) {
    fillScheduled = true
    whenIdle(() => { fillScheduled = false; fillCanvas(true) })
  }
}
watch(drawnNodes, () => fillCanvas(false))
/** The canvas reaches past the world by the most a drawn area can, at the lowest zoom (1). */
const canvasPad = computed(() => ({ x: width.value / 2 + DRAWN_MARGIN, y: height / 2 + DRAWN_MARGIN }))
const canvasBox = computed(() => {
  const pad = canvasPad.value, w = worldBounds.value.x + 2 * pad.x, h = worldBounds.value.y + 2 * pad.y
  return { viewBox: `${-pad.x} ${-pad.y} ${w} ${h}`, width: w * camera.value.zoom, height: h * camera.value.zoom }
})
const canvas = ref<SVGSVGElement>()
/** Moves the canvas so the camera's view fills the screen. Set directly, so no render is involved. */
function placeCanvas(at: AtlasCamera): void {
  const screen = rectAround(at, width.value, height), pad = canvasPad.value
  if (canvas.value) canvas.value.style.transform = `translate3d(${-(screen.left + pad.x) * at.zoom}px, ${-(screen.top + pad.y) * at.zoom}px, 0)`
}
watch([camera, width, canvas], () => { if (!drag?.moved) placeCanvas(camera.value) }, { flush: 'post' })
const cameraLabel = computed(() => `${camera.value.x} ${camera.value.y} ${camera.value.zoom}`)
const neighborhoodIds = computed(() => {
  const ids = new Set<string>()
  if (!selected.value || neighborhood.value === 'none') return ids
  ids.add(selected.value.skill.id)
  let frontier = [selected.value]
  for (let step = 0; step < neighborhoodDepth.value; step++) {
    const next: AtlasNode[] = []
    for (const node of frontier) for (const relation of node[neighborhood.value]) {
      if (ids.has(relation.skillId)) continue
      ids.add(relation.skillId)
      const related = nodesById.value.get(relation.skillId)
      if (related) next.push(related)
    }
    frontier = next
  }
  return ids
})
// The map dims everything outside an active focus, so the map always says which one.
const focusSummary = computed(() => {
  if (!selected.value || neighborhood.value === 'none') return ''
  return t(neighborhood.value === 'prerequisites' ? 'skills.atlas.focusShowingPrerequisites' : 'skills.atlas.focusShowingDependents', {
    skill: selected.value.skill.name,
    depth: t(neighborhoodDepth.value === 2 ? 'skills.atlas.twoSteps' : 'skills.atlas.oneStep'),
  })
})
// A string computed only notifies when the query text changes, so other view
// updates (selection, camera) keep the same match list and its window.
const query = computed(() => rawQuery.value.trim().toLocaleLowerCase())
const searchText = computed(() => new Map(props.model.nodes.map(node => [node.skill.id,
  `${node.skill.name} ${node.skill.description ?? ''} ${node.skill.subject_name ?? ''} ${fieldName(node.fieldId)}`.toLocaleLowerCase()])))
const matches = computed(() => query.value
  ? props.model.nodes.filter(node => searchText.value.get(node.skill.id)?.includes(query.value))
  : props.model.nodes)
/** The match list renders this many rows at first, and this many more on request or on scrolling near its end. */
const LIST_WINDOW = 50
const listLimit = ref(LIST_WINDOW)
watch(matches, () => { listLimit.value = LIST_WINDOW })
const listedMatches = computed(() => matches.value.slice(0, listLimit.value))
const unlisted = computed(() => matches.value.length - listedMatches.value.length)
const list = ref<HTMLUListElement>()
function showMoreMatches(moveFocus: boolean): void {
  const first = listLimit.value
  if (first >= matches.value.length) return
  listLimit.value += LIST_WINDOW
  if (moveFocus) void nextTick(() => list.value?.querySelectorAll<HTMLButtonElement>(':scope > li > button')[first]?.focus())
}
function listScrolled(event: Event): void {
  const target = event.currentTarget
  if (target instanceof HTMLElement && target.scrollTop + target.clientHeight >= target.scrollHeight - 200) showMoreMatches(false)
}
const bloomMatches = computed(() => matches.value.filter(bloomMatch))
function bloomMatch(node: AtlasNode): boolean { return bloomEmphasis.value !== null && node.earnedBloom?.rank === bloomEmphasis.value }
const matchIds = computed(() => new Set(matches.value.map(node => node.skill.id)))
function fieldName(fieldId: string | null): string {
  return present.fieldName(props.model.fieldById.get(fieldId) ?? { id: fieldId, name: null })
}
// Region headings are translated once per model and language, not on every render.
const regionHeadings = computed(() => {
  void locale.value
  return new Map(layout.value.regions.map(region => [region, { name: fieldName(region.field.id), counts: t('skills.atlas.fieldCounts', { ...region.field }) }]))
})
const status = present.evidence
// Map labels are translated once per node and language: a pan, or a redraw
// of the nodes, only looks them up.
const nodeTexts = computed(() => {
  void locale.value
  void props.model
  return new WeakMap<AtlasNode, { name: string; evidence: string }>()
})
function nodeText(node: AtlasNode): { name: string; evidence: string } {
  let text = nodeTexts.value.get(node)
  if (!text) { text = { name: present.accessibleName(node), evidence: status(node) }; nodeTexts.value.set(node, text) }
  return text
}
const bloomOptions = computed(() => {
  void locale.value
  return BLOOM_ORDER.map((level, rank) => ({ rank, label: present.bloomLabel(level) }))
})
/** An inspector relation's state, from its node when loaded: earned, Ready next or Ready. */
function relationNode(relation: AtlasNode['prerequisites'][number]): Pick<AtlasNode, 'status' | 'readyNext'> {
  return nodesById.value.get(relation.skillId) ?? { status: relation.earned ? 'earned' : 'ready', readyNext: false }
}
function relationLabel(relation: AtlasNode['prerequisites'][number]): string {
  const node = relationNode(relation)
  const key = node.status === 'earned' ? 'skills.atlas.inspectEarned' : node.readyNext ? 'skills.atlas.inspectReadyNext' : 'skills.atlas.inspectReady'
  return t(key, { name: relation.name })
}
function color(fieldId: string | null): string { return atlasFieldColor(props.model.fieldById.get(fieldId) ?? { id: fieldId, slot: null }) }
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
    camera: regionCamera(region, width.value) })
  preserveOverviewFocus()
}
function dimmed(node: AtlasNode): boolean {
  if (node.skill.id === selectedId.value) return false
  return (rawQuery.value !== '' && !matchIds.value.has(node.skill.id)) ||
    (bloomEmphasis.value !== null && node.earnedBloom?.rank !== bloomEmphasis.value) ||
    (neighborhoodIds.value.size > 0 && !neighborhoodIds.value.has(node.skill.id))
}
function zoom(delta: number): void { update({ camera: { ...camera.value, zoom: Math.min(1.8, Math.max(1, camera.value.zoom + delta)) } }) }
const worldBounds = computed(() => ({
  x: Math.max(REGION_WIDTH, ...layout.value.regions.map(region => region.x + region.width)),
  y: Math.max(MAP_HEIGHT, ...layout.value.regions.map(region => region.y + region.height)),
}))
function movedCamera(origin: AtlasCamera, x: number, y: number): AtlasCamera {
  return { ...origin,
    x: Math.max(0, Math.min(worldBounds.value.x, origin.x + x / origin.zoom)),
    y: Math.max(0, Math.min(worldBounds.value.y, origin.y + y / origin.zoom)) }
}
function pan(x: number, y: number): void { update({ camera: movedCamera(camera.value, x, y) }) }
// A drag moves only the canvas, so the shared view (and everything derived
// from it, such as the match list) stays put until the drag ends.
let drag: { id: number; x: number; y: number; camera: AtlasCamera; moved: boolean; touch: boolean; target: AtlasCamera | null } | null = null
let suppressMapClick = false
/** Shows the grabbing cursor over the map without restyling what is drawn on it. */
const grabCover = ref<HTMLElement>()
function stopDrag(): void {
  const wasMoving = drag?.moved === true
  drag = null
  if (grabCover.value) grabCover.value.hidden = true
  if (wasMoving) void nextTick(() => placeCanvas(camera.value))
}
function pointerDown(event: PointerEvent): void {
  // Native vertical scrolling and browser pinch zoom stay available on touch.
  if (event.isPrimary === false) { stopDrag(); return }
  if (event.button !== 0) return
  suppressMapClick = false
  drag = { id: event.pointerId, x: event.clientX, y: event.clientY, camera: { ...camera.value }, moved: false, touch: event.pointerType === 'touch', target: null }
}
function pointerMove(event: PointerEvent): void {
  if (!drag || event.pointerId !== drag.id) return
  const dx = event.clientX - drag.x, dy = event.clientY - drag.y
  if (!drag.moved && Math.hypot(dx, dy) < 6) return
  if (drag.touch && !drag.moved && Math.abs(dy) > Math.abs(dx)) { drag = null; return }
  if (!drag.moved) {
    drag.moved = true
    suppressMapClick = true
    if (grabCover.value) grabCover.value.hidden = false
    ;(event.currentTarget as HTMLElement).setPointerCapture?.(event.pointerId)
  }
  drag.target = movedCamera(drag.camera, -dx, drag.touch ? 0 : -dy)
  placeCanvas(drag.target)
  followDrag()
}
function pointerEnd(event: PointerEvent): void {
  if (drag?.id !== event.pointerId) return
  const moved = drag.target
  stopDrag()
  const map = event.currentTarget as HTMLElement
  if (map.hasPointerCapture?.(event.pointerId)) map.releasePointerCapture(event.pointerId)
  if (moved) update({ camera: moved })
}
function mapClick(event: MouseEvent): void {
  if (suppressMapClick && event.detail !== 0) { event.stopPropagation(); event.preventDefault() }
}
function keyPan(event: KeyboardEvent): void {
  if (event.target !== event.currentTarget) return
  const moves: Record<string, [number, number]> = { ArrowLeft: [-PAN_STEP, 0], ArrowRight: [PAN_STEP, 0], ArrowUp: [0, -PAN_STEP], ArrowDown: [0, PAN_STEP] }
  const move = moves[event.key]
  if (move) { event.preventDefault(); pan(...move) }
}
function escapeFocus(event: KeyboardEvent): void {
  if (event.key !== 'Escape' || neighborhood.value === 'none') return
  event.stopPropagation()
  update({ neighborhood: 'none' })
}
function reset(): void { update(createAtlasViewState()) }
// Edge geometry depends only on the layout, so it is built once per model.
const edgeShapes = computed(() => props.model.edges.flatMap(edge => {
  const from = layout.value.positions.get(edge.prerequisite_id)
  const to = layout.value.positions.get(edge.skill_id)
  if (!from || !to) return [] // Unloaded endpoints remain explicit in the inspector.
  const x1 = from.x + NODE_WIDTH, y1 = from.y + NODE_HEIGHT / 2
  const x2 = to.x, y2 = to.y + NODE_HEIGHT / 2
  const bend = Math.max(42, Math.min(160, Math.abs(x2 - x1) / 2))
  // A cubic Bézier stays inside the box of its control points.
  // Every curve ends heading right, so its arrowhead is a fixed triangle at its end.
  return [{ edge, path: `M ${x1} ${y1} C ${x1 + bend} ${y1}, ${x2 - bend} ${y2}, ${x2} ${y2}`, x2, y2 }]
}))
// All edges are four paths (lines and arrowheads, plain and highlighted), drawn
// whole and rebuilt only when the model or the focus changes. A pan or a
// redraw of the nodes never touches them, and they add four elements, not thousands.
function edgeActive(edge: AtlasModel['edges'][number]): boolean {
  return neighborhoodIds.value.size > 0
    ? neighborhoodIds.value.has(edge.prerequisite_id) && neighborhoodIds.value.has(edge.skill_id)
    : edge.prerequisite_id === selectedId.value || edge.skill_id === selectedId.value
}
const edgePaths = computed(() => {
  const plain: string[] = [], plainHeads: string[] = [], active: string[] = [], activeHeads: string[] = []
  for (const { edge, path, x2, y2 } of edgeShapes.value) {
    if (edgeActive(edge)) { active.push(path); activeHeads.push(`M ${x2} ${y2} l -14 -7 l 0 14 z`) }
    else { plain.push(path); plainHeads.push(`M ${x2} ${y2} l -10 -5 l 0 10 z`) }
  }
  return { plain: plain.join(' '), plainHeads: plainHeads.join(' '), active: active.join(' '), activeHeads: activeHeads.join(' ') }
})
// Overview summaries are built once per model, in one pass over its edges.
const fieldConnections = computed(() => {
  const connected = new Map<string | null, Set<string | null>>()
  const link = (field: string | null, other: string | null) => {
    const set = connected.get(field) ?? new Set<string | null>()
    set.add(other)
    connected.set(field, set)
  }
  for (const edge of props.model.edges) {
    const from = nodesById.value.get(edge.prerequisite_id), to = nodesById.value.get(edge.skill_id)
    if (!from || !to || from.fieldId === to.fieldId) continue
    link(from.fieldId, to.fieldId)
    link(to.fieldId, from.fieldId)
  }
  return new Map(props.model.fields.map(field => [field.id, props.model.fields.filter(item => connected.get(field.id)?.has(item.id))]))
})
function connections(field: AtlasField): readonly AtlasField[] { return fieldConnections.value.get(field.id) ?? [] }
const earnedByField = computed(() => new Map(props.model.fields.map(field => [field.id,
  field.nodeIds.flatMap(nodeId => { const node = nodesById.value.get(nodeId); return node?.status === 'earned' ? [node] : [] })])))
const earnedPreviews = computed(() => new Map([...earnedByField.value].map(([fieldId, nodes]) => [fieldId,
  [...nodes].sort((a, b) => Number(bloomMatch(b)) - Number(bloomMatch(a))).slice(0, 2)])))
function earnedPreview(field: AtlasField): readonly AtlasNode[] { return earnedPreviews.value.get(field.id) ?? [] }
function setBloom(event: Event): void {
  const value = event.target instanceof HTMLSelectElement ? event.target.value : ''
  const rank = value === '' ? null : Number(value)
  update({ bloomEmphasis: isBloomRank(rank) ? rank : null })
}
</script>

<template>
  <section class="atlas" :class="{ 'motion-reduced': motionReduced }" :aria-label="t('skills.atlas.title')" @keydown="escapeFocus">
    <header class="atlas-heading">
      <div><p class="atlas-eyebrow">{{ t('skills.atlas.title') }}</p><h2>{{ t('skills.atlas.fieldCounts', model.counts) }}</h2></div>
      <button type="button" @click="reset">{{ t('skills.atlas.reset') }}</button>
    </header>
    <div class="atlas-toolbar">
      <label class="atlas-search"><span>{{ t('skills.atlas.search') }}</span><input type="search" :value="view.query" @input="update({ query: ($event.target as HTMLInputElement).value })"></label>
      <button v-if="view.query" type="button" @click="update({ query: '' })">{{ t('skills.atlas.clearSearch') }}</button>
      <label><span>{{ t('skills.atlas.bloomLegend') }}</span><select :value="view.bloomEmphasis ?? ''" @change="setBloom"><option value="">{{ t('skills.atlas.allBloom') }}</option><option v-for="option in bloomOptions" :key="option.rank" :value="option.rank">{{ option.label }}</option></select></label>
    </div>
    <p class="atlas-legend">{{ t('skills.atlas.legend') }}</p>
    <p role="status" class="atlas-feedback" aria-live="polite"><template v-if="highlightedIds.length">{{ t('skills.atlas.updated') }}: {{ highlightedIds.slice(0, 3).map(skillId => nodesById.get(skillId)?.skill.name).filter(Boolean).join(', ') }}{{ highlightedIds.length > 3 ? '…' : '' }}</template></p>
    <div class="atlas-workspace">
      <div class="atlas-main">
        <p v-if="view.bloomEmphasis !== null" class="bloom-summary" role="status">{{ t('skills.atlas.bloomSummary', { level: present.bloomLabel(BLOOM_ORDER[view.bloomEmphasis]), count: bloomMatches.length }) }}</p>
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
              <button v-for="node in earnedPreview(region.field)" :key="node.skill.id" class="field-earned" :class="{ 'is-bloom-match': bloomMatch(node) }" type="button" :aria-label="present.accessibleName(node)" @click="select(node.skill.id)"><span class="skill-name" dir="auto">{{ node.skill.name }}</span><span><span aria-hidden="true">✓ </span>{{ status(node) }}</span></button>
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
              <button type="button" :aria-label="t('skills.atlas.panLeft')" @click="pan(-PAN_STEP, 0)">←</button>
              <button type="button" :aria-label="t('skills.atlas.panUp')" @click="pan(0, -PAN_STEP)">↑</button>
              <button type="button" :aria-label="t('skills.atlas.panDown')" @click="pan(0, PAN_STEP)">↓</button>
              <button type="button" :aria-label="t('skills.atlas.panRight')" @click="pan(PAN_STEP, 0)">→</button>
            </div>
            <div class="map-focus" :class="{ 'is-active': focusSummary }">
              <p role="status">{{ focusSummary }}</p>
              <button v-if="focusSummary" type="button" @click="update({ neighborhood: 'none' })">{{ t('skills.atlas.clearFocus') }}</button>
            </div>
            <div class="atlas-map" :style="{ height: `${height}px` }" :data-camera="cameraLabel" tabindex="0" role="group" :aria-label="t('skills.atlas.map')" :aria-describedby="`${id}-map-help`" @keydown="keyPan" @pointerdown="pointerDown" @pointermove="pointerMove" @pointerup="pointerEnd" @pointercancel="pointerEnd" @lostpointercapture="pointerEnd" @click.capture="mapClick">
              <svg ref="canvas" class="atlas-canvas" :viewBox="canvasBox.viewBox" :width="canvasBox.width" :height="canvasBox.height">
                <g v-for="region in layout.regions" :key="region.field.id === null ? 'unassigned' : `field:${region.field.id}`" :style="{ '--field': color(region.field.id) }">
                  <rect :x="region.x" :y="region.y" :width="region.width" :height="region.height" rx="24" class="map-region" />
                  <foreignObject :x="region.x + REGION_HEADING_BOX.dx" :y="region.y + REGION_HEADING_BOX.dy" :width="REGION_HEADING_BOX.width" :height="REGION_HEADING_BOX.height"><div class="region-heading"><strong>{{ regionHeadings.get(region)?.name }}</strong><span>{{ regionHeadings.get(region)?.counts }}</span></div></foreignObject>
                </g>
                <g class="map-edges" aria-hidden="true">
                  <path class="map-edge" :d="edgePaths.plain" /><path class="map-edge-head" :d="edgePaths.plainHeads" />
                  <path class="map-edge map-edge-active" :d="edgePaths.active" /><path class="map-edge-head map-edge-active" :d="edgePaths.activeHeads" />
                </g>
                <AtlasNodeLayer v-slot="{ items }" :source="nodeLayer">
                  <foreignObject v-for="node in items" :key="node.skill.id" v-memo="[node, layout, locale, onScreen(node), node.skill.id === view.selectedSkillId, node.readyNext, bloomMatch(node), dimmed(node), highlightedIds.includes(node.skill.id)]" :x="layout.positions.get(node.skill.id)?.x" :y="layout.positions.get(node.skill.id)?.y" :width="NODE_WIDTH" :height="NODE_HEIGHT" :style="{ '--field': color(node.fieldId) }">
                    <button class="map-node" :class="{ 'is-selected': node.skill.id === view.selectedSkillId, 'is-ready-next': node.readyNext, 'is-bloom-match': bloomMatch(node), 'is-dimmed': dimmed(node), 'is-updated': highlightedIds.includes(node.skill.id) }" type="button" :inert="!onScreen(node) || undefined" :aria-label="nodeText(node).name" :aria-pressed="node.skill.id === view.selectedSkillId" @click="select(node.skill.id)">
                      <span class="skill-name" dir="auto">{{ node.skill.name }}</span><span class="node-evidence"><span v-if="node.status === 'earned'" aria-hidden="true">✓</span><span v-else-if="node.readyNext" aria-hidden="true">◎</span><span v-else aria-hidden="true">○</span><span>{{ nodeText(node).evidence }}</span></span>
                    </button>
                  </foreignObject>
                </AtlasNodeLayer>
              </svg>
              <div ref="grabCover" class="atlas-grab-cover" hidden />
            </div>
            <p :id="`${id}-map-help`" class="map-help">{{ t('skills.atlas.mapHelp') }}</p>
          </template>
        </div>
        <details class="atlas-list" open>
          <summary>{{ t('skills.atlas.list') }} <span>{{ t('skills.atlas.results', { count: matches.length }) }}</span></summary>
          <!-- Memoised: a pan or zoom re-renders the map, never these rows. -->
          <ul ref="list" v-memo="[listedMatches, view.selectedSkillId, view.bloomEmphasis, locale, model]" :aria-label="t('skills.atlas.list')" @scroll.passive="listScrolled"><li v-for="node in listedMatches" :key="node.skill.id"><button :class="{ 'is-bloom-match': bloomMatch(node), 'is-ready-next': node.readyNext }" type="button" :style="{ '--field': color(node.fieldId) }" :aria-label="present.accessibleName(node)" :aria-pressed="node.skill.id === view.selectedSkillId" @click="select(node.skill.id)"><i class="field-dot" aria-hidden="true" /><span><strong dir="auto">{{ node.skill.name }}</strong><small>{{ fieldName(node.fieldId) }}</small></span><span class="list-evidence"><span v-if="node.status === 'earned'" aria-hidden="true">✓ </span><span v-else-if="node.readyNext" aria-hidden="true">◎ </span>{{ status(node) }}</span></button></li></ul>
          <button v-if="unlisted > 0" class="list-more" type="button" @click="showMoreMatches(true)">{{ t('skills.atlas.showMoreSkills', { count: Math.min(LIST_WINDOW, unlisted) }) }}</button>
          <p v-if="!matches.length">{{ t('skills.atlas.noResults') }}</p>
        </details>
      </div>
      <aside class="atlas-inspector" :class="{ 'is-collapsed': view.inspectorCollapsed }" :aria-label="t('skills.atlas.inspector')">
        <header><span>{{ t('skills.atlas.inspector') }}</span><button type="button" :aria-expanded="!view.inspectorCollapsed" :aria-controls="`${id}-inspector`" @click="update({ inspectorCollapsed: !view.inspectorCollapsed })">{{ t(view.inspectorCollapsed ? 'skills.atlas.expand' : 'skills.atlas.collapse') }}</button></header>
        <div :id="`${id}-inspector`" v-show="!view.inspectorCollapsed" class="inspector-content">
          <template v-if="selected">
            <p class="inspector-field" :style="{ '--field': color(selected.fieldId) }"><i class="field-dot" aria-hidden="true" />{{ fieldName(selected.fieldId) }}</p>
            <h2 dir="auto">{{ selected.skill.name }}</h2>
            <p class="inspector-status"><span v-if="selected.status === 'earned'" aria-hidden="true">✓ </span><span v-else-if="selected.readyNext" aria-hidden="true">◎ </span>{{ present.state(selected) }}</p>
            <p dir="auto">{{ selected.skill.description }}</p>
            <dl><template v-if="selected.status === 'earned'"><dt>{{ t('skills.atlas.currentBloom') }}</dt><dd>{{ status(selected) }}</dd></template><dt>{{ t('skills.atlas.intrinsicBloom') }}</dt><dd>{{ selected.intrinsicBloom ? t(`common.bloom.${selected.intrinsicBloom}`) : t('skills.atlas.unavailable') }}</dd></dl>
            <div class="inspector-actions"><button class="atlas-primary" type="button" :aria-label="t('skills.atlas.open')" @click="emit('open-skill', selected.skill.id)">{{ t('skills.atlas.open') }} <span aria-hidden="true">↗</span></button><button type="button" @click="emit('learn-skill', selected.skill.id)">{{ t('skills.atlas.learn') }}</button></div>
            <p v-if="!selected.prerequisites.length && selected.skill.prerequisite_count === 0" class="atlas-muted">{{ t('skills.atlas.noPrerequisites') }}</p>
            <p v-else-if="selected.prerequisitesSatisfied" class="atlas-muted">{{ t('skills.atlas.satisfied') }}</p>
            <p v-else-if="selected.prerequisites.some(relation => !relation.loaded) || selected.prerequisites.length < selected.skill.prerequisite_count" class="atlas-muted">{{ t('skills.atlas.incomplete') }}</p>
            <div class="neighborhood-controls"><button type="button" :aria-pressed="view.neighborhood === 'prerequisites'" @click="update({ neighborhood: view.neighborhood === 'prerequisites' ? 'none' : 'prerequisites' })">{{ t('skills.atlas.focusPrerequisites') }}</button><button type="button" :aria-pressed="view.neighborhood === 'dependents'" @click="update({ neighborhood: view.neighborhood === 'dependents' ? 'none' : 'dependents' })">{{ t('skills.atlas.focusDependents') }}</button><button v-if="view.neighborhood !== 'none'" type="button" @click="update({ neighborhood: 'none' })">{{ t('skills.atlas.allConnections') }}</button><label v-if="view.neighborhood !== 'none'"><span>{{ t('skills.atlas.depth') }}</span><select :value="view.neighborhoodDepth" @change="update({ neighborhoodDepth: Number(($event.target as HTMLSelectElement).value) === 2 ? 2 : 1 })"><option :value="1">{{ t('skills.atlas.oneStep') }}</option><option :value="2">{{ t('skills.atlas.twoSteps') }}</option></select></label></div>
            <section v-for="direction in (['prerequisites', 'dependents'] as const)" :key="direction" class="inspector-relations">
              <h3>{{ t(`skills.atlas.${direction}`) }} <span>{{ selected[direction].length }}</span></h3>
              <ul><li v-for="relation in selected[direction]" :key="relation.skillId"><button v-if="relation.loaded" type="button" :aria-label="relationLabel(relation)" @click="select(relation.skillId)"><strong dir="auto">{{ relation.name }}</strong><small>{{ fieldName(nodesById.get(relation.skillId)?.fieldId ?? null) }} · {{ present.state(relationNode(relation)) }}</small></button><p v-else>{{ relation.name }} · {{ t('skills.atlas.notLoaded') }}</p></li></ul>
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
.atlas:not(.motion-reduced) .map-node.is-updated { animation: atlas-evidence var(--duration-reveal) ease-out 1; }
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
/* Stays rendered while empty so its live region announces the next focus. */
.map-focus.is-active { display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: 8px; padding: 6px 8px 6px 14px; border-block-end: 1px solid var(--app-border); border-inline-start: 3px solid var(--app-primary); background: var(--app-muted); font-size: 13px; }
.map-focus p { min-width: 0; overflow-wrap: anywhere; }
.map-focus button { font-size: 12px; }
.atlas-map { position: relative; touch-action: pan-y pinch-zoom; cursor: grab; user-select: none; overflow: clip; background-image: radial-gradient(var(--app-border) .7px, transparent .7px); background-size: 18px 18px; }
.map-region { fill: color-mix(in srgb, var(--field) 5%, var(--app-background)); stroke: color-mix(in srgb, var(--field) 40%, var(--app-border)); stroke-width: 1; }
.region-heading { display: flex; flex-direction: column; gap: 4px; color: var(--field); }
.region-heading strong { font-size: 22px; font-weight: 650; }
.region-heading span { font-size: 12px; color: var(--app-muted-foreground); }
.map-edges { pointer-events: none; }
.map-edge { fill: none; stroke: var(--app-muted-foreground); stroke-width: 1.5; opacity: .28; }
.map-edge.map-edge-active { stroke: var(--app-primary); stroke-width: 2.5; opacity: .9; }
.map-edge-head { fill: var(--app-muted-foreground); opacity: .28; }
.map-edge-head.map-edge-active { fill: var(--app-primary); opacity: .9; }
.atlas .map-node { box-sizing: border-box; margin: 4px; width: calc(100% - 8px); height: calc(100% - 8px); border-inline-start: 4px solid var(--field); border-radius: 12px; display: flex; flex-direction: column; justify-content: space-between; gap: 6px; padding: 10px 12px; background: var(--app-card); font-size: 14px; line-height: 1.35; }
.node-evidence { display: flex; gap: 6px; color: var(--field); font-size: 13px; font-weight: 650; }
.map-node.is-dimmed .skill-name { color: var(--app-muted-foreground); font-weight: 400; }
/* Ready next: every prerequisite earned. Stronger than Ready, weaker than selection. */
.atlas .map-node.is-ready-next { border-color: var(--field); border-inline-start-width: 7px; background: color-mix(in srgb, var(--field) 10%, var(--app-card)); }
.atlas .map-node.is-selected { border-color: var(--app-primary); box-shadow: inset 0 0 0 2px var(--app-primary); }
.atlas-canvas { position: absolute; top: 0; left: 0; display: block; overflow: visible; will-change: transform; }
.atlas-grab-cover { position: absolute; inset: 0; cursor: grabbing; }
.atlas-grab-cover[hidden] { display: none; }
.map-help { padding: 10px 14px; color: var(--app-muted-foreground); font-size: 12px; border-block-start: 1px solid var(--app-border); }
.atlas-list { margin-block-start: 16px; border: 1px solid var(--app-border); border-radius: 12px; background: var(--app-card); }
.atlas-list summary { cursor: pointer; padding: 14px; font-weight: 600; }
.atlas-list summary span { display: inline-block; margin-inline-start: 10px; font-size: 12px; font-weight: 400; color: var(--app-muted-foreground); }
.atlas ul { list-style: none; padding: 0; margin: 0; }
.atlas-list ul { max-height: 330px; overflow: auto; padding: 0 8px 8px; }
.atlas .atlas-list li button { width: 100%; display: flex; align-items: center; gap: 12px; border: 0; border-block-start: 1px solid var(--app-border); border-radius: 0; }
.atlas .list-more { display: block; margin: 0 8px 8px; width: calc(100% - 16px); text-align: center; font-size: 13px; }
.atlas-list strong, .atlas-list small { display: block; }
.atlas-list strong { font-size: 13px; font-weight: 550; }
.atlas-list small { color: var(--app-muted-foreground); font-size: 11px; }
.atlas .atlas-list li button.is-ready-next { background: color-mix(in srgb, var(--field) 8%, var(--app-card)); }
.is-ready-next .list-evidence { font-weight: 650; }
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
