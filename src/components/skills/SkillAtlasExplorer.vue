<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, shallowRef, useId, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { atlasLayout, MAP_HEIGHT, NODE_HEIGHT, NODE_WIDTH, REGION_HEADING_BOX, REGION_WIDTH, regionCamera } from './atlasLayout'
import { atlasFieldColor, createAtlasPresenter } from './atlasPresentation'
import { SIMPLE_DISC_SIZE, simpleLayout } from './simpleLayout'
import { edgePaths as sortEdgePaths, simpleArrowHead, simpleDenseIds, simpleDimmed, simpleEdgeFaded, simpleEdgeShapes as buildSimpleEdgeShapes, simpleEmphasis as emphasisAround, simpleLabelQuiet, simpleNodeBoxes, type EdgeShape, type NodeBox } from './simpleGeometry'
import AtlasNodeLayer from './AtlasNodeLayer.vue'
import { animateMotion, motionDuration, useMotion } from '@/composables/useMotion'
import type { SkillGraphView } from '@/composables/useSkillGraphView'
import { BLOOM_ORDER } from '@/utils/bloom'
import { createAtlasViewState, type AtlasCamera, type AtlasViewState } from '@/utils/skillAtlasView'
import { isBloomRank, type AtlasField, type AtlasModel, type AtlasNode } from '@/utils/skillAtlas'
import type { AtlasFeedback } from '@/composables/useSkillAtlas'

const props = withDefaults(defineProps<{ model: AtlasModel; view: AtlasViewState; viewMode?: SkillGraphView; feedback?: AtlasFeedback | null; reducedMotion?: boolean }>(), { viewMode: 'simple' })
const emit = defineEmits<{ 'update:view': [view: AtlasViewState]; 'update:viewMode': [mode: SkillGraphView]; 'open-skill': [id: string]; 'learn-skill': [id: string]; 'feedback-consumed': [revision: number] }>()
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
  viewAnnouncement.value = ''
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
/**
 * A zero-height strip across the viewport. Watching it rather than the
 * viewport keeps the Simple view's width-driven height from re-notifying
 * within one frame (WebKit reports that as a ResizeObserver loop).
 */
const widthProbe = ref<HTMLElement>()
const width = ref(800)
const height = MAP_HEIGHT
/** How far one arrow press or pan button moves the map, in map units. */
const PAN_STEP = 180
let resizeObserver: ResizeObserver | undefined
onMounted(() => {
  if (widthProbe.value && typeof ResizeObserver !== 'undefined') {
    resizeObserver = new ResizeObserver(entries => { width.value = Math.max(280, entries[0]?.contentRect.width ?? 800) })
    resizeObserver.observe(widthProbe.value)
  }
})
onBeforeUnmount(() => resizeObserver?.disconnect())
// Simple: a layered flow of discs that fits the viewport, with no camera.
// Detailed-only view fields (camera, query, Bloom emphasis, field focus) are
// left untouched in Simple, so they are intact on the way back.
const simple = computed(() => props.viewMode === 'simple')
const simpleGeometry = computed(() => simpleLayout(props.model, width.value))
/** Where each node's element sits on the canvas, in the current view. */
const nodeBoxes = computed(() => {
  if (simple.value) return simpleNodeBoxes(simpleGeometry.value)
  const boxes = new Map<string, NodeBox>()
  for (const [skillId, point] of layout.value.positions) boxes.set(skillId, { x: point.x, y: point.y, width: NODE_WIDTH, height: NODE_HEIGHT })
  return boxes
})
/** Nodes in layers too dense for every name: their plain Ready names wait for hover or focus. */
const denseIds = computed(() => simpleDenseIds(simpleGeometry.value, width.value))
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
// Whatever is still scheduled is cancelled when the explorer goes.
const scheduled = new Set<() => void>()
function whenIdle(run: () => void): void {
  let cancel: () => void
  const task = () => { scheduled.delete(cancel); run() }
  if (typeof requestIdleCallback === 'function') {
    const handle = requestIdleCallback(task, { timeout: 100 })
    cancel = () => cancelIdleCallback(handle)
  } else {
    const handle = setTimeout(task, 0)
    cancel = () => clearTimeout(handle)
  }
  scheduled.add(cancel)
}
onBeforeUnmount(() => {
  for (const cancel of scheduled) cancel()
  scheduled.clear()
})
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
const nodeLayer = { get items(): readonly AtlasNode[] { return simple.value ? props.model.nodes : canvasNodes.value } }
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
  if (simple.value) {
    const h = simpleGeometry.value.height
    return { viewBox: `0 0 ${width.value} ${h}`, width: width.value, height: h }
  }
  const pad = canvasPad.value, w = worldBounds.value.x + 2 * pad.x, h = worldBounds.value.y + 2 * pad.y
  return { viewBox: `${-pad.x} ${-pad.y} ${w} ${h}`, width: w * camera.value.zoom, height: h * camera.value.zoom }
})
const canvas = ref<SVGSVGElement>()
/** Moves the canvas so the camera's view fills the screen. Set directly, so no render is involved. */
function placeCanvas(at: AtlasCamera): void {
  if (simple.value) { if (canvas.value) canvas.value.style.transform = ''; return }
  const screen = rectAround(at, width.value, height), pad = canvasPad.value
  if (canvas.value) canvas.value.style.transform = `translate3d(${-(screen.left + pad.x) * at.zoom}px, ${-(screen.top + pad.y) * at.zoom}px, 0)`
}
watch([camera, width, canvas, simple], () => { if (!drag?.moved) placeCanvas(camera.value) }, { flush: 'post' })
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
/** Simple emphasis: what a selection or inspector focus traces. */
const simpleEmphasis = computed(() => emphasisAround(selected.value, neighborhoodIds.value, nodesById.value))
function labelQuiet(node: AtlasNode): boolean {
  return simple.value && simpleLabelQuiet(node, denseIds.value, simpleEmphasis.value)
}
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
// Simple offers no search, so its list holds every skill; the query waits for Detailed.
const query = computed(() => simple.value ? '' : rawQuery.value.trim().toLocaleLowerCase())
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
function bloomMatch(node: AtlasNode): boolean { return !simple.value && bloomEmphasis.value !== null && node.earnedBloom?.rank === bloomEmphasis.value }
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
/** Scrolls a Simple node into view when it is off screen; Reduced and Off jump rather than glide. */
function reveal(skillId: string): void {
  void nextTick(() => {
    const element = nodeElements().find(item => item.dataset.skill === skillId)?.querySelector('.map-node')
    element?.scrollIntoView({ block: 'nearest', inline: 'nearest', behavior: motionReduced.value ? 'auto' : 'smooth' })
  })
}
/** Selects a skill. `fromElsewhere` marks a choice made off the map (the list, a relation), which Simple brings into view. */
function select(skillId: string, fromElsewhere = false): void {
  const node = nodesById.value.get(skillId)
  const point = layout.value.positions.get(skillId)
  if (!node || !point) return
  if (simple.value) {
    update({ selectedSkillId: skillId, inspectorCollapsed: false })
    if (fromElsewhere) reveal(skillId)
    return
  }
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
  if (simple.value) return simpleDimmed(simpleEmphasis.value, node.skill.id)
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
  if (simple.value) return
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
  if (simple.value || event.target !== event.currentTarget) return
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
/**
 * Set when the learner switches here, until the view arrives. A change made
 * elsewhere (the other surface, or the remembered view loading after mount)
 * applies at once without travel or an announcement.
 */
let switchRequested = false
function chooseView(mode: SkillGraphView): void {
  if (mode === props.viewMode) return
  switchRequested = true
  emit('update:viewMode', mode)
}
// The learner's own switch is announced. In full motion the nodes travel from
// their old places and sizes to their new ones while the old edges fade out;
// the new edges, hidden meanwhile, then fade back in.
const viewAnnouncement = ref('')
const switching = ref(false)
let switchTimer: ReturnType<typeof setTimeout> | undefined
/** A still copy of the edges as drawn before the switch, fading out while the nodes travel. */
let leavingEdges: SVGSVGElement | undefined
function finishSwitch(): void {
  if (switchTimer !== undefined) clearTimeout(switchTimer)
  switchTimer = undefined
  switching.value = false
  leavingEdges?.remove()
  leavingEdges = undefined
}
onBeforeUnmount(finishSwitch)
function nodeElements(): SVGForeignObjectElement[] {
  return [...canvas.value?.querySelectorAll<SVGForeignObjectElement>('foreignObject[data-skill]') ?? []]
}
/**
 * Copies the canvas as it stands, edges only, over the map and fades it out.
 * The copy keeps the old view box, size and placement, so the old curves stay
 * where they were while the canvas beneath re-renders for the new view.
 */
function holdLeavingEdges(): void {
  const source = canvas.value, edges = source?.querySelector('.map-edges'), map = source?.parentElement
  if (!source || !edges || !map) return
  const copy = source.cloneNode(false) as SVGSVGElement
  copy.classList.add('atlas-canvas-leaving')
  copy.setAttribute('aria-hidden', 'true')
  copy.appendChild(edges.cloneNode(true))
  map.appendChild(copy)
  leavingEdges = copy
  animateMotion(copy, [{ opacity: 1 }, { opacity: 0 }], 'slow')
}
/** Moves and resizes each node that was on the canvas before the switch from its old box to its new one. */
function travel(before: Map<string, DOMRect>): void {
  for (const element of nodeElements()) {
    const from = before.get(element.dataset.skill ?? ''), to = element.getBoundingClientRect()
    if (!from || !to.width || !to.height) continue
    // Canvas units per screen pixel differ by view (Detailed zooms).
    const scale = to.width / (Number(element.getAttribute('width')) || to.width)
    const dx = (from.left + from.width / 2 - to.left - to.width / 2) / scale
    const dy = (from.top + from.height / 2 - to.top - to.height / 2) / scale
    // Scaled about the box's centre (see .map-node-box), so the centre travels by dx, dy.
    const sx = from.width / to.width, sy = from.height / to.height
    if (dx || dy || sx !== 1 || sy !== 1) animateMotion(element, [{ transform: `translate(${dx}px, ${dy}px) scale(${sx}, ${sy})` }, { transform: 'none' }], 'slow')
  }
}
watch(() => props.viewMode, mode => {
  finishSwitch()
  const requested = switchRequested
  switchRequested = false
  if (!requested) return
  viewAnnouncement.value = t('skills.atlas.viewChanged', { view: t(mode === 'simple' ? 'skills.atlas.viewSimple' : 'skills.atlas.viewDetailed') })
  if (motionReduced.value) return
  // Read before the re-render (a pre-flush watcher), so the old places are known.
  const before = new Map(nodeElements().map(element => [element.dataset.skill ?? '', element.getBoundingClientRect()]))
  holdLeavingEdges()
  switching.value = true
  switchTimer = setTimeout(finishSwitch, motionDuration('slow'))
  void nextTick(() => travel(before))
})
// Edge geometry depends only on the layout, so it is built once per model.
const edgeShapes = computed(() => props.model.edges.flatMap((edge): EdgeShape[] => {
  const from = layout.value.positions.get(edge.prerequisite_id)
  const to = layout.value.positions.get(edge.skill_id)
  if (!from || !to) return [] // Unloaded endpoints remain explicit in the inspector.
  const x1 = from.x + NODE_WIDTH, y1 = from.y + NODE_HEIGHT / 2
  const x2 = to.x, y2 = to.y + NODE_HEIGHT / 2
  const bend = Math.max(42, Math.min(160, Math.abs(x2 - x1) / 2))
  // A cubic Bezier stays inside the box of its control points.
  return [{ edge, path: `M ${x1} ${y1} C ${x1 + bend} ${y1}, ${x2 - bend} ${y2}, ${x2} ${y2}`, x2, y2, tx: 1, ty: 0 }]
}))
const simpleEdgeShapes = computed(() => buildSimpleEdgeShapes(props.model.edges, simpleGeometry.value))
/** Every Detailed curve ends heading right, so its arrowhead is a fixed triangle at its end. */
function detailedArrowHead({ x2, y2 }: EdgeShape, length: number): string {
  return `M ${x2} ${y2} l ${-length} ${-length / 2} l 0 ${length} z`
}
// All edges are a few paths (lines and arrowheads per emphasis), drawn whole
// and rebuilt only when the model or the focus changes. A pan or a redraw of
// the nodes never touches them, and they add a few elements, not thousands.
function edgeActive(edge: AtlasModel['edges'][number]): boolean {
  return neighborhoodIds.value.size > 0
    ? neighborhoodIds.value.has(edge.prerequisite_id) && neighborhoodIds.value.has(edge.skill_id)
    : edge.prerequisite_id === selectedId.value || edge.skill_id === selectedId.value
}
// Simple also fades the edges that leave the emphasised neighbourhood.
const edgePaths = computed(() => {
  if (!simple.value) return sortEdgePaths(edgeShapes.value, edge => edgeActive(edge) ? 'active' : 'plain', detailedArrowHead)
  const emphasis = simpleEmphasis.value
  return sortEdgePaths(simpleEdgeShapes.value, edge => edgeActive(edge) ? 'active' : simpleEdgeFaded(emphasis, edge) ? 'faded' : 'plain', simpleArrowHead)
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
      <div class="atlas-heading-actions">
        <div class="atlas-view-switch" role="group" :aria-label="t('skills.atlas.viewGroup')">
          <button type="button" :aria-pressed="viewMode === 'simple'" @click="chooseView('simple')"><svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" aria-hidden="true"><circle cx="3.5" cy="8" r="2.2" /><circle cx="12.5" cy="3.5" r="2.2" /><circle cx="12.5" cy="12.5" r="2.2" /><path d="M5.5 7l5-2.5M5.5 9l5 2.5" /></svg>{{ t('skills.atlas.viewSimple') }}</button>
          <button type="button" :aria-pressed="viewMode === 'detailed'" @click="chooseView('detailed')"><svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" aria-hidden="true"><rect x="1.5" y="2" width="6" height="5" rx="1.2" /><rect x="8.5" y="2" width="6" height="5" rx="1.2" /><rect x="1.5" y="9" width="6" height="5" rx="1.2" /><rect x="8.5" y="9" width="6" height="5" rx="1.2" /></svg>{{ t('skills.atlas.viewDetailed') }}</button>
        </div>
        <button type="button" @click="reset">{{ t('skills.atlas.reset') }}</button>
      </div>
    </header>
    <div v-if="!simple" class="atlas-toolbar">
      <label class="atlas-search"><span>{{ t('skills.atlas.search') }}</span><input type="search" :value="view.query" @input="update({ query: ($event.target as HTMLInputElement).value })"></label>
      <button v-if="view.query" type="button" @click="update({ query: '' })">{{ t('skills.atlas.clearSearch') }}</button>
      <label><span>{{ t('skills.atlas.bloomLegend') }}</span><select :value="view.bloomEmphasis ?? ''" @change="setBloom"><option value="">{{ t('skills.atlas.allBloom') }}</option><option v-for="option in bloomOptions" :key="option.rank" :value="option.rank">{{ option.label }}</option></select></label>
    </div>
    <p v-if="!simple" class="atlas-legend">{{ t('skills.atlas.legend') }}</p>
    <p role="status" class="atlas-feedback" :class="{ 'has-feedback': highlightedIds.length }" aria-live="polite"><template v-if="highlightedIds.length">{{ t('skills.atlas.updated') }}: {{ highlightedIds.slice(0, 3).map(skillId => nodesById.get(skillId)?.skill.name).filter(Boolean).join(', ') }}{{ highlightedIds.length > 3 ? '…' : '' }}</template><span v-else-if="viewAnnouncement" class="sr-only">{{ viewAnnouncement }}</span></p>
    <div class="atlas-workspace">
      <div class="atlas-main">
        <p v-if="!simple && view.bloomEmphasis !== null" class="bloom-summary" role="status">{{ t('skills.atlas.bloomSummary', { level: present.bloomLabel(BLOOM_ORDER[view.bloomEmphasis]), count: bloomMatches.length }) }}</p>
        <!-- Simple keeps the field colours named, without field focus. -->
        <p v-if="simple" class="atlas-fields">
          <span v-for="field in model.fields" :key="field.id === null ? 'unassigned' : `field:${field.id}`" class="field-chip" :style="{ '--field': color(field.id) }"><i class="field-dot" aria-hidden="true" />{{ fieldName(field.id) }} <small>{{ t('skills.atlas.fieldEarned', { ...field }) }}</small></span>
        </p>
        <nav v-else class="atlas-fields" :aria-label="t('skills.atlas.overview')">
          <button type="button" :aria-pressed="overview" @click="update({ focusedFieldId: null, unassignedFocused: false, camera: null, neighborhood: 'none' })">{{ t('skills.atlas.overview') }}</button>
          <button v-for="field in model.fields" :key="field.id === null ? 'unassigned' : `field:${field.id}`" type="button" :style="{ '--field': color(field.id) }" :aria-pressed="fieldFocused && field.id === view.focusedFieldId" @click="focus(field)"><i class="field-dot" aria-hidden="true" />{{ fieldName(field.id) }} <small>{{ t('skills.atlas.fieldEarned', { ...field }) }}</small></button>
        </nav>
        <div ref="viewport" class="atlas-viewport">
          <div ref="widthProbe" class="atlas-width-probe" aria-hidden="true" />
          <div v-if="overview && !simple" class="atlas-overview">
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
            <div v-if="!simple" class="map-tools" :aria-label="t('skills.atlas.map')">
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
            <!-- One canvas and one element per node in both views: a switch re-skins and moves them. -->
            <div class="atlas-map" :class="{ 'is-simple': simple, 'is-switching': switching }" :style="{ height: `${simple ? simpleGeometry.height : height}px`, '--disc-size': `${SIMPLE_DISC_SIZE}px` }" :data-camera="simple ? undefined : cameraLabel" :tabindex="simple ? undefined : 0" role="group" :aria-label="t('skills.atlas.map')" :aria-describedby="simple ? undefined : `${id}-map-help`" @keydown="keyPan" @pointerdown="pointerDown" @pointermove="pointerMove" @pointerup="pointerEnd" @pointercancel="pointerEnd" @lostpointercapture="pointerEnd" @click.capture="mapClick">
              <svg ref="canvas" class="atlas-canvas" :viewBox="canvasBox.viewBox" :width="canvasBox.width" :height="canvasBox.height">
                <template v-if="!simple">
                  <g v-for="region in layout.regions" :key="region.field.id === null ? 'unassigned' : `field:${region.field.id}`" :style="{ '--field': color(region.field.id) }">
                    <rect :x="region.x" :y="region.y" :width="region.width" :height="region.height" rx="24" class="map-region" />
                    <foreignObject :x="region.x + REGION_HEADING_BOX.dx" :y="region.y + REGION_HEADING_BOX.dy" :width="REGION_HEADING_BOX.width" :height="REGION_HEADING_BOX.height"><div class="region-heading"><strong>{{ regionHeadings.get(region)?.name }}</strong><span>{{ regionHeadings.get(region)?.counts }}</span></div></foreignObject>
                  </g>
                </template>
                <g class="map-edges" aria-hidden="true">
                  <template v-if="simple"><path class="map-edge map-edge-faded" :d="edgePaths.faded.path" /><path class="map-edge-head map-edge-faded" :d="edgePaths.faded.heads" /></template>
                  <path class="map-edge" :d="edgePaths.plain.path" /><path class="map-edge-head" :d="edgePaths.plain.heads" />
                  <path class="map-edge map-edge-active" :d="edgePaths.active.path" /><path class="map-edge-head map-edge-active" :d="edgePaths.active.heads" />
                </g>
                <AtlasNodeLayer v-slot="{ items }" :source="nodeLayer">
                  <foreignObject v-for="node in items" :key="node.skill.id" v-memo="[node, nodeBoxes, simple, locale, onScreen(node), node.skill.id === view.selectedSkillId, node.readyNext, bloomMatch(node), dimmed(node), labelQuiet(node), highlightedIds.includes(node.skill.id)]" class="map-node-box" :data-skill="node.skill.id" :x="nodeBoxes.get(node.skill.id)?.x" :y="nodeBoxes.get(node.skill.id)?.y" :width="nodeBoxes.get(node.skill.id)?.width" :height="nodeBoxes.get(node.skill.id)?.height" :style="{ '--field': color(node.fieldId) }">
                    <button class="map-node" :class="{ 'is-selected': node.skill.id === view.selectedSkillId, 'is-earned': node.status === 'earned', 'is-ready-next': node.readyNext, 'is-bloom-match': bloomMatch(node), 'is-dimmed': dimmed(node), 'is-updated': highlightedIds.includes(node.skill.id) }" type="button" :inert="(!simple && !onScreen(node)) || undefined" :aria-label="nodeText(node).name" :aria-pressed="node.skill.id === view.selectedSkillId" @click="select(node.skill.id)">
                      <template v-if="simple">
                        <span class="node-disc" aria-hidden="true">{{ node.status === 'earned' && node.earnedBloom ? node.earnedBloom.rank + 1 : '' }}</span><span class="node-label" :class="{ 'is-label-quiet': labelQuiet(node) }" dir="auto">{{ node.skill.name }}</span>
                      </template>
                      <template v-else>
                        <span class="skill-name" dir="auto">{{ node.skill.name }}</span><span class="node-evidence"><span v-if="node.status === 'earned'" aria-hidden="true">✓</span><span v-else-if="node.readyNext" aria-hidden="true">◎</span><span v-else aria-hidden="true">○</span><span>{{ nodeText(node).evidence }}</span></span>
                      </template>
                    </button>
                  </foreignObject>
                </AtlasNodeLayer>
              </svg>
              <div ref="grabCover" class="atlas-grab-cover" hidden />
            </div>
            <p v-if="simple && !model.nodes.length" class="simple-empty">{{ t('skills.atlas.empty') }}</p>
            <p v-if="simple" class="simple-legend">
              <span><i class="legend-key is-earned" aria-hidden="true" />{{ t('skills.atlas.legendEarned') }}</span>
              <span><i class="legend-key is-ready-next" aria-hidden="true" />{{ t('skills.atlas.legendReadyNext') }}</span>
              <span><i class="legend-key" aria-hidden="true" />{{ t('skills.atlas.legendReady') }}</span>
              <span class="legend-field">{{ t('skills.atlas.legendField') }}</span>
            </p>
            <p v-else :id="`${id}-map-help`" class="map-help">{{ t('skills.atlas.mapHelp') }}</p>
          </template>
        </div>
        <details class="atlas-list" open>
          <summary>{{ t('skills.atlas.list') }} <span>{{ t('skills.atlas.results', { count: matches.length }) }}</span></summary>
          <!-- Memoised: a pan or zoom re-renders the map, never these rows. -->
          <ul ref="list" v-memo="[listedMatches, view.selectedSkillId, view.bloomEmphasis, simple, locale, model]" :aria-label="t('skills.atlas.list')" @scroll.passive="listScrolled"><li v-for="node in listedMatches" :key="node.skill.id"><button :class="{ 'is-bloom-match': bloomMatch(node), 'is-ready-next': node.readyNext }" type="button" :style="{ '--field': color(node.fieldId) }" :aria-label="present.accessibleName(node)" :aria-pressed="node.skill.id === view.selectedSkillId" @click="select(node.skill.id, true)"><i class="field-dot" aria-hidden="true" /><span><strong dir="auto">{{ node.skill.name }}</strong><small>{{ fieldName(node.fieldId) }}</small></span><span class="list-evidence"><span v-if="node.status === 'earned'" aria-hidden="true">✓ </span><span v-else-if="node.readyNext" aria-hidden="true">◎ </span>{{ status(node) }}</span></button></li></ul>
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
              <ul><li v-for="relation in selected[direction]" :key="relation.skillId"><button v-if="relation.loaded" type="button" :aria-label="relationLabel(relation)" @click="select(relation.skillId, true)"><strong dir="auto">{{ relation.name }}</strong><small>{{ fieldName(nodesById.get(relation.skillId)?.fieldId ?? null) }} · {{ present.state(relationNode(relation)) }}</small></button><p v-else>{{ relation.name }} · {{ t('skills.atlas.notLoaded') }}</p></li></ul>
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
.atlas-feedback.has-feedback { padding: 10px 14px; margin-block-end: 12px; border-inline-start: 3px solid var(--app-primary); background: var(--app-muted); border-radius: 8px; }
.atlas .map-node.is-updated { outline: 2px solid var(--app-primary); outline-offset: -2px; }
.atlas:not(.motion-reduced) .map-node.is-updated { animation: atlas-evidence var(--duration-reveal) ease-out 1; }
@keyframes atlas-evidence { from { box-shadow: inset 0 0 0 5px var(--app-primary); } to { box-shadow: inset 0 0 0 0 transparent; } }
@media (prefers-reduced-motion: reduce) { .atlas .map-node.is-updated, .atlas .map-node.is-updated .node-disc { animation: none !important; } }
.atlas-heading { display: flex; align-items: center; justify-content: space-between; gap: 16px; margin-block-end: 20px; }
.atlas-heading-actions { display: flex; flex-wrap: wrap; align-items: center; justify-content: flex-end; gap: 8px; }
/* The view switch: a segmented control whose pressed half is the active view. */
.atlas-view-switch { display: flex; gap: 3px; padding: 3px; border: 1px solid var(--app-border); border-radius: 12px; background: var(--app-card); }
.atlas .atlas-view-switch button { display: flex; align-items: center; justify-content: center; gap: 8px; border: 0; border-radius: 9px; background: transparent; color: var(--app-muted-foreground); font-size: 13px; font-weight: 550; padding: 8px 13px; transition: background-color var(--transition-fast), color var(--transition-fast); }
.atlas .atlas-view-switch button:not([aria-pressed=true]):hover { background: var(--app-muted); color: var(--app-foreground); }
.atlas .atlas-view-switch button[aria-pressed=true] { background: var(--app-primary); color: var(--app-primary-foreground); box-shadow: none; }
.atlas-view-switch svg { width: 16px; height: 16px; flex-shrink: 0; }
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
.atlas-fields small { font-size: 11px; color: var(--app-muted-foreground); }
.field-chip { display: inline-flex; align-items: center; gap: 7px; min-height: 32px; padding: 4px 11px 4px 9px; border: 1px solid var(--app-border); border-radius: 999px; background: var(--app-card); font-size: 12px; }
.field-dot { display: inline-block; width: 9px; height: 9px; background: var(--field); border-radius: 50%; flex-shrink: 0; }
.atlas-viewport { border: 1px solid var(--app-border); border-radius: 18px; overflow: hidden; background: var(--app-background); }
.atlas-width-probe { height: 0; }
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
/* Simple: the same node elements wear a disc skin on a still canvas. */
.atlas-map.is-simple { cursor: default; touch-action: auto; user-select: auto; background-size: 22px 22px; }
/* In both views the edges are hidden at once while nodes travel (their old curves fade out on a copy), then fade back in at their new places. */
.atlas-map .map-edges { transition: opacity var(--transition-base); }
.atlas-map.is-switching .map-edges { opacity: 0; transition: none; }
.atlas-canvas-leaving { pointer-events: none; }
/* A travelling node resizes about its centre. */
.map-node-box { transform-box: fill-box; transform-origin: center; }
.map-edge.map-edge-faded, .map-edge-head.map-edge-faded { opacity: .07; }
.atlas-map.is-simple .map-node-box { pointer-events: none; overflow: visible; }
.atlas .atlas-map.is-simple .map-node { pointer-events: auto; box-sizing: border-box; margin: 0 auto; width: auto; max-width: 100%; height: auto; min-height: 0; display: flex; flex-direction: column; align-items: center; justify-content: flex-start; gap: 6px; padding: 8px 4px 2px; border: 0; border-radius: 10px; background: transparent; box-shadow: none; text-align: center; transition: opacity var(--transition-base); }
.atlas .atlas-map.is-simple .map-node.is-updated { outline: none; animation: none; }
.atlas .atlas-map.is-simple .map-node:focus-visible { outline: 3px solid var(--app-ring); outline-offset: 2px; }
.atlas-map.is-simple .map-node.is-dimmed { opacity: .32; }
.node-disc { position: relative; box-sizing: border-box; display: grid; place-items: center; flex-shrink: 0; width: var(--disc-size); height: var(--disc-size); border-radius: 50%; border: 1.5px solid color-mix(in srgb, var(--app-muted-foreground) 55%, transparent); background: var(--app-card); color: var(--app-muted-foreground); font-size: 15px; font-weight: 650; transition: transform var(--transition-fast), box-shadow var(--transition-fast); }
.map-node.is-earned .node-disc { background: var(--field); border-color: var(--field); color: var(--app-card); }
.map-node.is-ready-next .node-disc { border: 2px solid var(--field); color: var(--field); }
.map-node.is-ready-next .node-disc::after { content: ""; position: absolute; inset: -6px; border-radius: 50%; border: 1.5px dashed color-mix(in srgb, var(--field) 70%, transparent); }
.map-node:hover .node-disc { transform: scale(1.06); }
.map-node.is-selected .node-disc { box-shadow: 0 0 0 3px var(--app-background), 0 0 0 5.5px var(--app-primary); }
.map-node.is-updated .node-disc { outline: 2px solid var(--app-primary); outline-offset: 3px; }
.atlas:not(.motion-reduced) .map-node.is-updated .node-disc { animation: atlas-evidence var(--duration-reveal) ease-out 1; }
.node-label { max-width: 100%; padding: 1px 4px; border-radius: 4px; background: color-mix(in srgb, var(--app-background) 88%, transparent); font-size: 12px; font-weight: 550; line-height: 1.4; display: -webkit-box; -webkit-line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden; overflow-wrap: anywhere; }
.map-node.is-selected .node-label { color: var(--app-primary); font-weight: 650; }
/* Dense layers: plain Ready names wait for hover or focus (the list and accessible name always carry them). */
.node-label.is-label-quiet { opacity: 0; }
.map-node:hover .node-label, .map-node:focus-visible .node-label { opacity: 1; }
.simple-empty { padding: 14px; color: var(--app-muted-foreground); }
.simple-legend { display: flex; flex-wrap: wrap; align-items: center; gap: 6px 16px; padding: 10px 14px; border-block-start: 1px solid var(--app-border); color: var(--app-muted-foreground); font-size: 12px; }
.simple-legend span { display: inline-flex; align-items: center; gap: 6px; }
.legend-key { display: inline-block; box-sizing: border-box; width: 12px; height: 12px; border-radius: 50%; border: 1.5px solid var(--app-muted-foreground); }
.legend-key.is-earned { background: var(--app-muted-foreground); }
.legend-key.is-ready-next { border-style: dashed; }
.simple-legend .legend-field { margin-inline-start: auto; }
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
  .atlas-heading { flex-direction: column; align-items: stretch; }
  .atlas-heading-actions { justify-content: stretch; }
  .atlas-heading-actions > button { font-size: 12px; }
  /* Full width, with thumb-sized halves. */
  .atlas-view-switch { width: 100%; }
  .atlas .atlas-view-switch button { flex: 1; }
  .atlas-toolbar > label { width: 100%; }
  .atlas-fields { flex-wrap: nowrap; overflow-x: auto; padding-block-end: 4px; }
  .atlas-fields button { flex-shrink: 0; max-width: 230px; }
  .atlas-overview { max-height: 560px; }
  .atlas-inspector { position: sticky; top: auto; bottom: 0; z-index: 2; border-radius: 18px 18px 0 0; box-shadow: 0 -4px 22px color-mix(in srgb, var(--app-foreground) 10%, transparent); }
  .inspector-content { max-height: 45vh; padding: 16px; }
  .atlas-inspector.is-collapsed { margin-block-start: 0; }
}
</style>
