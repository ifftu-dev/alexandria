import { computed, onBeforeUnmount, shallowRef, watch, type Ref } from 'vue'
import type { SkillGraphView } from '@/composables/useSkillGraphView'
import type { AtlasField, AtlasModel, AtlasNode } from '@/utils/skillAtlas'
import type { AtlasCamera, AtlasViewState } from '@/utils/skillAtlasView'
import { webLayoutEntry, webNarrowedIds, type WebPoint } from '@/utils/webGeometry'
import type { NodeBox } from './simpleGeometry'
import { useWebGraph, WEB_MAX_ZOOM, WEB_MIN_ZOOM, WEB_ZOOM_STEP, type WebDot } from './useWebGraph'

/** What the explorer's Web view reads from the explorer, through getters so it sees changes. */
export interface ExplorerWebInput {
  model: AtlasModel
  view: AtlasViewState
  viewMode: SkillGraphView
  reducedMotion: boolean
  /** The owner's shared dragged positions; without them the explorer keeps its own. */
  webPositions: ReadonlyMap<string, WebPoint> | null
  /** The inspector's prerequisite or dependent focus; empty when off. */
  neighborhoodIds: ReadonlySet<string>
  /** Skills matching the search (every skill without a query), and whether there is a query. */
  matchIds: ReadonlySet<string>
  searching: boolean
}

export interface ExplorerWebOptions {
  input: () => ExplorerWebInput
  /** The explorer's view update; the adapter publishes the Web camera and its own changes through it. */
  update: (patch: Partial<AtlasViewState>) => void
  dragged: (positions: ReadonlyMap<string, WebPoint>) => void
  reset: () => void
  /** Runs a task when the main thread is idle; the explorer cancels what is left when it goes. */
  whenIdle: (run: () => void) => void
  map: Readonly<Ref<HTMLElement | undefined>>
}

/**
 * The explorer's Web view: the shared Web graph geometry and interaction
 * placed on the explorer's own node elements, the Web tools' actions, and
 * the camera that the graph moves live and the view hears of when a gesture
 * ends. Every handler is a no-op outside Web and says whether it handled the
 * event, so the explorer delegates to it once per handler. Everything the Web
 * graph reads is empty outside Web, so the other views never pay for it.
 */
export function useExplorerWeb({ input, update, dragged, reset: resetOwner, whenIdle, map }: ExplorerWebOptions) {
  const active = computed(() => input().viewMode === 'web')
  const entry = computed(() => webLayoutEntry(input().model))
  const view = computed(() => input().view)
  const selectedId = computed(() => view.value.selectedSkillId)

  /** The explorer's own drags, used when the owner passes no positions. */
  const ownDragged = shallowRef<{ model: AtlasModel; positions: ReadonlyMap<string, WebPoint> } | null>(null)
  const positions = computed(() => input().webPositions ??
    (ownDragged.value?.model === input().model ? ownDragged.value.positions : entry.value.layout))
  /** Narrowed to the selection: only skills within the chosen steps show. */
  const narrowed = computed(() => active.value && view.value.webNarrowed && selectedId.value !== null && input().model.nodeById.has(selectedId.value))
  const shownIds = computed(() => narrowed.value
    ? webNarrowedIds(entry.value.graph, selectedId.value!, view.value.neighborhoodDepth, view.value.webRelated) : null)
  const fieldFocused = computed(() => view.value.focusedFieldId !== null || view.value.unassignedFocused)
  /** Skills in the focused field. */
  const fieldIds = computed(() => fieldFocused.value
    ? new Set(input().model.nodes.filter(node => node.fieldId === view.value.focusedFieldId).map(node => node.skill.id)) : null)
  /** Search and field focus fade every skill outside them. */
  const matchIds = computed(() => {
    if (!active.value || (!input().searching && !fieldFocused.value)) return null
    return new Set([...input().matchIds].filter(skillId => !fieldIds.value || fieldIds.value.has(skillId)))
  })

  const EMPTY_POSITIONS: ReadonlyMap<string, WebPoint> = new Map()
  const graph = useWebGraph(() => ({
    model: input().model,
    graph: entry.value.graph,
    positions: active.value ? positions.value : EMPTY_POSITIONS,
    selectedId: active.value ? selectedId.value : null,
    // The inspector's focus lights its own neighbourhood in place of the selection's.
    focusIds: active.value && input().neighborhoodIds.size > 0 ? input().neighborhoodIds : null,
    matchIds: matchIds.value,
    shownIds: shownIds.value,
    fitIds: active.value ? fieldIds.value : null,
    narrowed: narrowed.value,
    names: view.value.webNames, arrows: view.value.webArrows, related: view.value.webRelated,
    camera: view.value.webCamera, compact: false, reducedMotion: input().reducedMotion,
  }), {
    select: skillId => { if (skillId) select(skillId); else clearSelection() },
    camera: cameraMoved,
    drag: next => {
      if (!input().webPositions) ownDragged.value = { model: input().model, positions: next }
      dragged(next)
    },
  }, computed(() => active.value ? map.value : undefined))

  const dotById = computed(() => new Map(graph.dots.value.map(dot => [dot.id, dot])))
  function dot(node: AtlasNode): WebDot | undefined { return active.value ? dotById.value.get(node.skill.id) : undefined }
  /** The nodes drawn in Web, in model order; null outside Web. */
  const nodes = computed(() => active.value ? graph.dots.value.map(dot => dot.node) : null)
  /** Where each node's element sits on the canvas, in screen pixels; null outside Web. */
  const nodeBoxes = computed(() => {
    if (!active.value) return null
    const boxes = new Map<string, NodeBox>()
    for (const dot of graph.dots.value) boxes.set(dot.id, { x: dot.x - dot.hit / 2, y: dot.y - dot.hit / 2, width: dot.hit, height: dot.hit })
    return boxes
  })
  /** Web draws in screen pixels: one canvas unit per pixel of the map. Null outside Web. */
  const canvasBox = computed(() => {
    if (!active.value) return null
    const { width, height } = graph.size.value
    return { viewBox: `0 0 ${width} ${height}`, width, height }
  })
  // The lines and zoom tools follow every pan and zoom. Read through getters
  // by layers of their own, so a camera move re-renders them, not the explorer.
  const lineLayer = { get items() { return active.value ? [graph.lines.value] : [] } }
  const zoomLayer = { get items() {
    if (!active.value) return []
    const zoom = graph.camera.value.zoom
    return [{ percent: Math.round(zoom * 100), canZoomIn: zoom < WEB_MAX_ZOOM, canZoomOut: zoom > WEB_MIN_ZOOM }]
  } }
  /** The Web camera on the map element, for checks that follow it, set directly so a pan renders nothing else. */
  watch(() => active.value ? graph.camera.value : null, at => {
    if (at && map.value) map.value.dataset.camera = `${at.x} ${at.y} ${at.zoom}`
  }, { flush: 'post' })

  // The graph draws a moved camera at once; the view hears of it when the
  // gesture ends (or when idle, for the wheel and the zoom buttons), so a pan
  // re-renders the graph and not the explorer and its owner on every frame.
  let pendingCamera: AtlasCamera | null = null
  /** Pointers pressed on the map. */
  const pointers = new Set<number>()
  let cameraScheduled = false
  function cameraMoved(at: AtlasCamera): void {
    pendingCamera = at
    if (pointers.size > 0 || cameraScheduled) return
    cameraScheduled = true
    whenIdle(() => { cameraScheduled = false; if (pointers.size === 0) publishCamera() })
  }
  function publishCamera(): void {
    const at = pendingCamera
    pendingCamera = null
    if (at) update({ webCamera: at })
  }
  watch(() => input().viewMode, publishCamera, { flush: 'sync' })
  onBeforeUnmount(publishCamera)
  /**
   * A view update with the camera a gesture has moved but not yet published.
   * A camera set on purpose (Fit, Reset, a field, narrowing) replaces it.
   */
  function withPendingCamera(patch: Partial<AtlasViewState>): Partial<AtlasViewState> {
    const pending = pendingCamera
    pendingCamera = null
    return pending && !('webCamera' in patch) ? { webCamera: pending, ...patch } : patch
  }

  // Actions.
  /** Selects a skill. Narrowed, the graph re-centres on it and fits; a choice made off the map is brought to the middle at the same zoom. */
  function select(skillId: string, fromElsewhere = false): boolean {
    if (!active.value) return false
    const at = positions.value.get(skillId)
    update({ selectedSkillId: skillId, inspectorCollapsed: false,
      ...(view.value.webNarrowed ? { webCamera: null } : fromElsewhere && at ? { webCamera: { x: at.x, y: at.y, zoom: graph.camera.value.zoom } } : {}) })
    return true
  }
  function clearSelection(): void {
    update({ selectedSkillId: null, webNarrowed: false, ...(view.value.webNarrowed ? { webCamera: null } : {}) })
  }
  /** Fades the other fields and fits the camera to this one. */
  function focusField(field: AtlasField): boolean {
    if (!active.value) return false
    update({ focusedFieldId: field.id, unassignedFocused: field.id === null, webCamera: null })
    return true
  }
  function allFields(): void { update({ focusedFieldId: null, unassignedFocused: false, webCamera: null }) }
  function narrow(around: boolean): void { update({ webNarrowed: around, webCamera: null }) }
  /** A changed depth re-fits a narrowed graph. */
  function depthPatch(): Partial<AtlasViewState> { return narrowed.value ? { webCamera: null } : {} }
  function fit(): void { update({ webCamera: null }) }
  /** After the explorer's Reset: every dragged dot goes back, on every surface. */
  function reset(): void {
    if (!active.value) return
    ownDragged.value = null
    resetOwner()
  }

  // Map handlers. Each returns whether Web handled the event.
  function pointerDown(event: PointerEvent): boolean {
    if (!active.value) return false
    if (event.pointerType !== 'mouse' || event.button === 0) pointers.add(event.pointerId)
    graph.pointerDown(event)
    return true
  }
  function pointerMove(event: PointerEvent): boolean {
    if (!active.value) return false
    graph.pointerMove(event)
    return true
  }
  function pointerEnd(event: PointerEvent): boolean {
    if (!active.value) return false
    graph.pointerUp(event)
    if (pointers.delete(event.pointerId) && pointers.size === 0) publishCamera()
    return true
  }
  function click(event: MouseEvent): boolean {
    if (!active.value) return false
    graph.clickCapture(event)
    return true
  }
  /** In Web the arrow keys rove between dots and never pan. */
  function keyDown(event: KeyboardEvent): boolean {
    if (!active.value) return false
    graph.keyDown(event)
    return true
  }
  function keyUp(event: KeyboardEvent): void { if (active.value) graph.keyUp(event) }
  function focusIn(event: FocusEvent): void { if (active.value) graph.focusIn(event) }
  function focusOut(event: FocusEvent): void { if (active.value) graph.focusOut(event) }
  function wheel(event: WheelEvent): void { if (active.value) graph.wheel(event) }
  function hover(skillId: string | null): void { if (active.value) graph.hover(skillId) }
  function zoom(direction: 1 | -1): void { graph.zoomBy(direction > 0 ? WEB_ZOOM_STEP : 1 / WEB_ZOOM_STEP) }
  function pan(dx: number, dy: number): void { graph.panBy(dx, dy) }

  return {
    active, narrowed, nodes, nodeBoxes, canvasBox, dot, lineLayer, zoomLayer, withPendingCamera,
    select, focusField, allFields, narrow, depthPatch, fit, reset, zoom, pan,
    pointerDown, pointerMove, pointerEnd, click, keyDown, keyUp, focusIn, focusOut, wheel, hover,
  }
}
