import { computed, onBeforeUnmount, onMounted, ref, shallowRef, watch, type Ref } from 'vue'
import { motionDuration, useMotion } from '@/composables/useMotion'
import type { AtlasModel, AtlasNode } from '@/utils/skillAtlas'
import type { AtlasCamera } from '@/utils/skillAtlasView'
import { atlasFieldColor } from './atlasPresentation'
import { continueWebLayout, webDotRadius, webGraphFromAtlas, webNamePlaces, webNearestInDirection, webNearestTo, type WebDirection, type WebNamePlace, type WebPoint } from '@/utils/webGeometry'

// Camera and drawing constants. Positions and dot radii are layout units; the
// camera's zoom is screen pixels per layout unit. Lines, arrowheads and names
// are drawn in screen pixels, so they keep their size at any zoom.
/** The camera's zoom range. */
export const WEB_MIN_ZOOM = 0.2
export const WEB_MAX_ZOOM = 6
/** Past this zoom every shown dot carries its name. */
export const WEB_NAMES_ZOOM = 2
/** Room left around what a fitted camera shows, in pixels, in the full and the compact graph. */
export const WEB_FIT_PADDING = 36
/** Room left in the full graph when every shown dot carries its name: half the widest name, so names at the edge are not cut off. */
export const WEB_NAMED_FIT_PADDING = 84
export const WEB_COMPACT_FIT_PADDING = 14
/** A fitted camera never frames less than this many layout units, so one dot is not blown up. */
export const WEB_MIN_FIT_EXTENT = 160
/** Size of the graph before it is measured (and in tests), in pixels. */
export const WEB_DEFAULT_SIZE = { width: 800, height: 520 }
export const WEB_COMPACT_DEFAULT_SIZE = { width: 260, height: 200 }
/** Smallest dot drawn, and smallest pointer target around a dot, in pixels. */
export const WEB_DOT_MIN_PIXELS = 4
export const WEB_HIT_PIXELS = 14
/** A press that moves this many pixels is a drag, not a click. */
export const WEB_DRAG_THRESHOLD = 4
/** Zoom per pixel of wheel travel: one notch (about 100 pixels) zooms by about a fifth. */
export const WEB_WHEEL_RATE = 0.002
/** Zoom of one press of a zoom button, as a factor. */
export const WEB_ZOOM_STEP = 1.25

/** Estimated size of a name, to keep names apart: pixels per character, widest name and line height, in the full and the compact graph (match WebGraphDot's styles). */
export const WEB_NAME_CHAR_PIXELS = 6.6
export const WEB_COMPACT_NAME_CHAR_PIXELS = 5.5
export const WEB_NAME_MAX_PIXELS = 160
export const WEB_COMPACT_NAME_MAX_PIXELS = 110
export const WEB_NAME_LINE_PIXELS = 16
export const WEB_COMPACT_NAME_LINE_PIXELS = 13

/** Length of an arrowhead, in pixels. */
export const WEB_ARROW_PIXELS = 7

export type WebDotState = 'earned' | 'ready-next' | 'ready'

/** One SVG path per emphasis: lines in the lit set, faded outside it (or outside a search), plain when nothing is lit. */
export interface WebLinePaths { plain: string; lit: string; faded: string }
/** Prerequisite lines, dotted relation lines and the arrowheads at the dependent end of prerequisite lines. */
export interface WebLines { prerequisites: WebLinePaths; relations: WebLinePaths; heads: WebLinePaths }

/** One dot as drawn: screen position and size in pixels, and how it reads now. */
export interface WebDot {
  node: AtlasNode
  id: string
  x: number
  y: number
  /** Diameter of the dot, and of the pointer target around it. */
  size: number
  hit: number
  color: string
  state: WebDotState
  selected: boolean
  /** The one dot in the Tab order. */
  roving: boolean
  /** Outside the lit neighbourhood, or not matching the search or field focus. */
  faded: boolean
  /** Its name shows beside the dot, */
  labelled: boolean
  /** on this side, chosen so names do not overlap where there is room. */
  namePlace: WebNamePlace
}

/** What a Web graph draws and how; the component's props, read through a getter so the composable sees changes. */
export interface WebGraphInput {
  model: AtlasModel
  positions: ReadonlyMap<string, WebPoint>
  selectedId: string | null
  /** An explicit neighbourhood (the inspector's prerequisite or dependent focus) lit in place of the selection's. */
  focusIds: ReadonlySet<string> | null
  /** When set, dots outside it fade (search, field focus). */
  matchIds: ReadonlySet<string> | null
  /** When set, only these dots are drawn (narrowing, the sidebar subset). */
  shownIds: ReadonlySet<string> | null
  /** When set, a fitted camera frames these dots (field focus) rather than every shown one. */
  fitIds: ReadonlySet<string> | null
  narrowed: boolean
  names: boolean
  arrows: boolean
  related: boolean
  camera: AtlasCamera | null
  compact: boolean
  reducedMotion: boolean
}

export interface WebGraphEvents {
  select: (id: string | null) => void
  active: (id: string | null) => void
  camera: (camera: AtlasCamera) => void
  drag: (positions: ReadonlyMap<string, WebPoint>) => void
}

export function clampZoom(zoom: number): number {
  return Math.min(WEB_MAX_ZOOM, Math.max(WEB_MIN_ZOOM, zoom))
}

/** The camera that frames these points in a box of this many pixels. */
export function webFitCamera(points: Iterable<WebPoint>, width: number, height: number, padding: number): AtlasCamera {
  let minX = Infinity, maxX = -Infinity, minY = Infinity, maxY = -Infinity
  for (const point of points) {
    minX = Math.min(minX, point.x); maxX = Math.max(maxX, point.x)
    minY = Math.min(minY, point.y); maxY = Math.max(maxY, point.y)
  }
  if (minX > maxX) return { x: 0, y: 0, zoom: 1 }
  const spanX = Math.max(maxX - minX, WEB_MIN_FIT_EXTENT), spanY = Math.max(maxY - minY, WEB_MIN_FIT_EXTENT)
  const zoom = clampZoom(Math.min(Math.max(1, width - 2 * padding) / spanX, Math.max(1, height - 2 * padding) / spanY))
  return { x: (minX + maxX) / 2, y: (minY + maxY) / 2, zoom }
}

/**
 * The Web graph's geometry, emphasis and interaction, shared by the Web graph
 * component and by the explorer's own node layer, so the two cannot drift
 * apart. `root` is the element that holds the dots (each carries
 * `data-skill`) and takes the pointer, wheel, key and focus handlers.
 */
export function useWebGraph(input: () => WebGraphInput, events: WebGraphEvents, root: Readonly<Ref<HTMLElement | undefined>>) {
  const { reduced: systemReduced } = useMotion()
  const motionReduced = computed(() => input().reducedMotion || systemReduced.value)
  const model = computed(() => input().model)
  const graph = computed(() => webGraphFromAtlas(model.value))

  // Size: measured from the root, with a default until then. The root may
  // come and go (the explorer draws its map only in some views), so it is
  // watched rather than read once at mount.
  const measured = ref<{ width: number; height: number } | null>(null)
  const size = computed(() => measured.value ?? (input().compact ? WEB_COMPACT_DEFAULT_SIZE : WEB_DEFAULT_SIZE))
  let observer: ResizeObserver | undefined
  onMounted(() => {
    watch(root, element => {
      observer?.disconnect()
      observer = undefined
      if (!element || typeof ResizeObserver === 'undefined') return
      observer = new ResizeObserver(entries => {
        const box = entries[0]?.contentRect
        if (box && box.width > 0 && box.height > 0) measured.value = { width: box.width, height: box.height }
      })
      observer.observe(element)
    }, { immediate: true, flush: 'post' })
  })
  onBeforeUnmount(() => observer?.disconnect())

  /** Positions during a live drag, until the owner's catch up through the drag event and the run stops. */
  const working = shallowRef<ReadonlyMap<string, WebPoint> | null>(null)
  const positions = computed(() => working.value ?? input().positions)
  /** Every dot drawn, in model order. */
  const shownNodes = computed(() => {
    const shown = input().shownIds
    return model.value.nodes.filter(node => positions.value.has(node.skill.id) && (!shown || shown.has(node.skill.id)))
  })
  const shownSet = computed(() => new Set(shownNodes.value.map(node => node.skill.id)))

  // Camera: the owner's, or the camera that fits what is shown.
  const fitCamera = computed(() => {
    const fit = input().fitIds
    const ids = fit && [...fit].some(id => shownSet.value.has(id)) ? [...fit].filter(id => shownSet.value.has(id)) : [...shownSet.value]
    const points = ids.flatMap(id => { const point = positions.value.get(id); return point ? [point] : [] })
    const { compact, names, narrowed } = input()
    return webFitCamera(points, size.value.width, size.value.height, compact ? WEB_COMPACT_FIT_PADDING : names || narrowed ? WEB_NAMED_FIT_PADDING : WEB_FIT_PADDING)
  })
  /** A camera this graph moved to and has emitted, until the owner's camera changes. */
  const localCamera = shallowRef<AtlasCamera | null>(null)
  watch(() => input().camera, () => { localCamera.value = null })
  const camera = computed<AtlasCamera>(() => localCamera.value ?? input().camera ?? fitCamera.value)
  function setCamera(next: AtlasCamera): void {
    localCamera.value = next
    events.camera(next)
  }
  function toScreen(point: WebPoint): WebPoint {
    const at = camera.value
    return { x: (point.x - at.x) * at.zoom + size.value.width / 2, y: (point.y - at.y) * at.zoom + size.value.height / 2 }
  }

  // Keyboard roving: one dot is in the Tab order.
  const activeId = ref<string | null>(null)
  const rovingId = computed(() => {
    const selected = input().selectedId
    if (activeId.value && shownSet.value.has(activeId.value)) return activeId.value
    if (selected && shownSet.value.has(selected)) return selected
    return webNearestTo({ x: camera.value.x, y: camera.value.y }, positions.value, shownSet.value)
  })
  watch(() => input().selectedId, () => { activeId.value = null })

  // Emphasis. Hover, and focus that arrived by keyboard, light a dot's
  // one-step neighbourhood while they last; selection keeps it.
  const hoverId = ref<string | null>(null)
  const focusedId = ref<string | null>(null)
  /** Focus counts only when it came from the keyboard: a click focuses its dot, and selection already lights it. */
  const keyboardFocus = ref(false)
  let pressing = false
  const transientId = computed(() => hoverId.value ?? (keyboardFocus.value ? focusedId.value : null))
  watch(transientId, id => events.active(id))
  /** A shown skill and the shown skills one step away: prerequisites, dependents and, with Related on, relations. */
  function neighbourhood(id: string): Set<string> {
    const ids = new Set<string>()
    const node = model.value.nodeById.get(id)
    if (!node || !shownSet.value.has(id)) return ids
    ids.add(id)
    const steps = [...node.prerequisites, ...node.dependents, ...(input().related ? node.relations : [])]
    for (const step of steps) if (shownSet.value.has(step.skillId)) ids.add(step.skillId)
    return ids
  }
  /** Dots whose names show at any zoom: the selected and the hovered or focused dot, and their neighbours. */
  const namedIds = computed(() => {
    const selected = input().selectedId
    return new Set([...(selected ? neighbourhood(selected) : []), ...(transientId.value ? neighbourhood(transientId.value) : [])])
  })
  /** The lit set; empty when nothing is lit, so nothing fades. */
  const litIds = computed(() => {
    const focus = input().focusIds, selected = input().selectedId
    const kept = focus && focus.size > 0 ? [...focus].filter(id => shownSet.value.has(id)) : selected ? [...neighbourhood(selected)] : []
    return new Set([...kept, ...(transientId.value ? neighbourhood(transientId.value) : [])])
  })
  function fadedByMatch(id: string): boolean {
    const matches = input().matchIds
    return matches !== null && !matches.has(id) && id !== input().selectedId
  }
  function faded(id: string): boolean {
    return (litIds.value.size > 0 && !litIds.value.has(id)) || fadedByMatch(id)
  }

  function radiusOf(node: AtlasNode): number {
    // An earned skill whose level is unavailable takes the base size, filled.
    return webDotRadius(node.status === 'earned' ? node.earnedBloom?.rank : null)
  }
  const dots = computed<WebDot[]>(() => {
    const at = camera.value, { selectedId: selected, names, narrowed } = input()
    const everyName = names || narrowed || at.zoom >= WEB_NAMES_ZOOM
    const drawn = shownNodes.value.map((node): WebDot => {
      const id = node.skill.id
      const screen = toScreen(positions.value.get(id)!)
      const dotSize = Math.max(WEB_DOT_MIN_PIXELS, radiusOf(node) * 2 * at.zoom)
      return {
        node, id, x: screen.x, y: screen.y, size: dotSize, hit: Math.max(WEB_HIT_PIXELS, dotSize),
        color: atlasFieldColor(model.value.fieldById.get(node.fieldId) ?? { id: node.fieldId, slot: null }),
        state: node.status === 'earned' ? 'earned' : node.readyNext ? 'ready-next' : 'ready',
        selected: id === selected, roving: id === rovingId.value, faded: faded(id),
        labelled: everyName || namedIds.value.has(id), namePlace: 'below',
      }
    })
    placeNames(drawn)
    return drawn
  })
  /**
   * Chooses where each shown name on screen sits, so names in a cluster do
   * not overlap where there is room: the selected and the hovered or focused
   * dot first, then their neighbours, then larger dots.
   */
  function placeNames(drawn: WebDot[]): void {
    const { compact } = input(), transient = transientId.value, { width, height } = size.value
    const charPixels = compact ? WEB_COMPACT_NAME_CHAR_PIXELS : WEB_NAME_CHAR_PIXELS
    const maxPixels = compact ? WEB_COMPACT_NAME_MAX_PIXELS : WEB_NAME_MAX_PIXELS
    const linePixels = compact ? WEB_COMPACT_NAME_LINE_PIXELS : WEB_NAME_LINE_PIXELS
    const rank = (dot: WebDot) => dot.selected ? 0 : dot.id === transient ? 1 : namedIds.value.has(dot.id) ? 2 : 3
    const near = (dot: WebDot) => dot.x > -maxPixels && dot.x < width + maxPixels && dot.y > -linePixels * 2 && dot.y < height + linePixels * 2
    const named = drawn.filter(dot => dot.labelled && near(dot))
    if (!named.length) return
    named.sort((a, b) => rank(a) - rank(b) || b.size - a.size || (a.id < b.id ? -1 : a.id > b.id ? 1 : 0))
    const places = webNamePlaces(named.map(dot => ({ id: dot.id, x: dot.x, y: dot.y, radius: dot.size / 2, height: linePixels,
      width: Math.min(maxPixels, dot.node.skill.name.length * charPixels + 4) })),
    drawn.filter(near).map(dot => ({ x: dot.x, y: dot.y, radius: dot.size / 2 })))
    for (const dot of named) dot.namePlace = places.get(dot.id) ?? 'below'
  }

  // Lines run from dot edge to dot edge in screen pixels, so their width and
  // arrowheads stay the same at any zoom.
  const lines = computed<WebLines>(() => {
    const byId = new Map(dots.value.map(dot => [dot.id, dot]))
    const lit = litIds.value, arrows = input().arrows
    const out = { prerequisites: { plain: [] as string[], lit: [] as string[], faded: [] as string[] },
      relations: { plain: [] as string[], lit: [] as string[], faded: [] as string[] },
      heads: { plain: [] as string[], lit: [] as string[], faded: [] as string[] } }
    const n = (value: number) => value.toFixed(1)
    const emphasis = (a: string, b: string): keyof WebLinePaths =>
      lit.size > 0 && lit.has(a) && lit.has(b) ? 'lit' : lit.size > 0 || fadedByMatch(a) || fadedByMatch(b) ? 'faded' : 'plain'
    const segment = (fromId: string, toId: string) => {
      const from = byId.get(fromId), to = byId.get(toId)
      if (!from || !to) return null
      const dx = to.x - from.x, dy = to.y - from.y, length = Math.hypot(dx, dy)
      if (length <= from.size / 2 + to.size / 2) return null
      const ux = dx / length, uy = dy / length
      return { x1: from.x + ux * from.size / 2, y1: from.y + uy * from.size / 2, x2: to.x - ux * to.size / 2, y2: to.y - uy * to.size / 2, ux, uy }
    }
    for (const link of graph.value.prerequisites) {
      // Drawn from the prerequisite to the skill that builds on it.
      const line = segment(link.prerequisite, link.skill)
      if (!line) continue
      const kind = emphasis(link.prerequisite, link.skill)
      out.prerequisites[kind].push(`M ${n(line.x1)} ${n(line.y1)} L ${n(line.x2)} ${n(line.y2)}`)
      if (kind !== 'lit' && !arrows) continue
      const back = WEB_ARROW_PIXELS, wing = WEB_ARROW_PIXELS / 2
      const bx = line.x2 - line.ux * back, by = line.y2 - line.uy * back
      out.heads[kind].push(`M ${n(line.x2)} ${n(line.y2)} L ${n(bx - line.uy * wing)} ${n(by + line.ux * wing)} L ${n(bx + line.uy * wing)} ${n(by - line.ux * wing)} Z`)
    }
    if (input().related) {
      for (const [a, b] of graph.value.relations) {
        const line = segment(a, b)
        if (line) out.relations[emphasis(a, b)].push(`M ${n(line.x1)} ${n(line.y1)} L ${n(line.x2)} ${n(line.y2)}`)
      }
    }
    const join = (paths: { plain: string[]; lit: string[]; faded: string[] }): WebLinePaths =>
      ({ plain: paths.plain.join(' '), lit: paths.lit.join(' '), faded: paths.faded.join(' ') })
    return { prerequisites: join(out.prerequisites), relations: join(out.relations), heads: join(out.heads) }
  })

  // Dragging. With full motion a live run of the layout's forces lets the
  // neighbours follow and stops within the emphasis duration of release;
  // with reduced or off motion only the held dot moves. At rest nothing runs.
  let held: { id: string; point: WebPoint } | null = null
  let frame: number | null = null
  let stopTimer: ReturnType<typeof setTimeout> | undefined
  const nextFrame = (run: () => void): number => typeof requestAnimationFrame === 'function' ? requestAnimationFrame(run) : Number(setTimeout(run, 16))
  const cancelFrame = (handle: number): void => { if (typeof cancelAnimationFrame === 'function') cancelAnimationFrame(handle); else clearTimeout(handle) }
  function liveFrame(): void {
    frame = null
    if (!held) return
    const base = new Map(positions.value)
    base.set(held.id, held.point)
    const next = continueWebLayout(graph.value, base, { pinned: new Set([held.id]) })
    working.value = next
    events.drag(next)
    frame = nextFrame(liveFrame)
  }
  function moveHeld(id: string, point: WebPoint): void {
    held = { id, point }
    if (motionReduced.value) {
      const next = new Map(positions.value)
      next.set(id, point)
      working.value = next
      events.drag(next)
      return
    }
    if (stopTimer !== undefined) { clearTimeout(stopTimer); stopTimer = undefined }
    if (frame === null) frame = nextFrame(liveFrame)
  }
  function stopDrag(): void {
    if (frame !== null) cancelFrame(frame)
    if (stopTimer !== undefined) clearTimeout(stopTimer)
    frame = null
    stopTimer = undefined
    held = null
    working.value = null
  }
  function releaseHeld(): void {
    if (!held) return
    if (motionReduced.value || frame === null) stopDrag()
    else stopTimer = setTimeout(stopDrag, motionDuration('emphasis'))
  }
  onBeforeUnmount(stopDrag)

  // Pointer gestures: press a dot to drag it, the background to pan.
  // Two touches pinch: they zoom about their midpoint and pan with it.
  type Gesture =
    | { kind: 'press'; pointerId: number; id: string | null; start: WebPoint }
    | { kind: 'dot'; pointerId: number; id: string; offset: WebPoint }
    | { kind: 'pan'; pointerId: number; start: WebPoint; camera: AtlasCamera }
    | { kind: 'pinch'; pointerId: number; distance: number; anchor: WebPoint; camera: AtlasCamera }
  let gesture: Gesture | null = null
  const pointers = new Map<number, WebPoint>()
  function pinchSpan(): { distance: number; middle: WebPoint } | null {
    const [a, b] = [...pointers.values()]
    if (!a || !b) return null
    return { distance: Math.max(1, Math.hypot(b.x - a.x, b.y - a.y)), middle: { x: (a.x + b.x) / 2, y: (a.y + b.y) / 2 } }
  }
  /** The camera that shows `world` at the screen point `screen` at this zoom. */
  function cameraHolding(world: WebPoint, screen: WebPoint, zoom: number): AtlasCamera {
    return { x: world.x - (screen.x - size.value.width / 2) / zoom, y: world.y - (screen.y - size.value.height / 2) / zoom, zoom }
  }
  /** Zooms by a factor, keeping the layout point under a screen point (the centre by default) where it is. */
  function zoomBy(factor: number, about: WebPoint = { x: size.value.width / 2, y: size.value.height / 2 }): void {
    const zoom = clampZoom(camera.value.zoom * factor)
    if (zoom === camera.value.zoom) return
    setCamera(cameraHolding(toWorld(about), about, zoom))
  }
  /** Pans by screen pixels, as the pan controls do. */
  function panBy(dx: number, dy: number): void {
    const at = camera.value
    setCamera({ ...at, x: at.x + dx / at.zoom, y: at.y + dy / at.zoom })
  }
  /** The compact graph zooms with any wheel; the full graph, like the Detailed map, leaves a plain wheel to the page and zooms on a pinch (ctrl or meta + wheel). */
  function wheel(event: WheelEvent): void {
    if (!input().compact && !event.ctrlKey && !event.metaKey) return
    event.preventDefault()
    const pixels = event.deltaY * (event.deltaMode === 1 ? 16 : event.deltaMode === 2 ? size.value.height : 1)
    zoomBy(Math.exp(-pixels * WEB_WHEEL_RATE), local(event))
  }
  let suppressClick = false
  function local(event: MouseEvent): WebPoint {
    const box = root.value?.getBoundingClientRect()
    return { x: event.clientX - (box?.left ?? 0), y: event.clientY - (box?.top ?? 0) }
  }
  function toWorld(point: WebPoint): WebPoint {
    const at = camera.value
    return { x: (point.x - size.value.width / 2) / at.zoom + at.x, y: (point.y - size.value.height / 2) / at.zoom + at.y }
  }
  function pointerDown(event: PointerEvent): void {
    if (event.pointerType === 'mouse' && event.button !== 0) return
    pressStart()
    pointers.set(event.pointerId, local(event))
    const span = pinchSpan()
    if (pointers.size === 2 && span) {
      if (gesture?.kind === 'dot') releaseHeld()
      suppressClick = true
      gesture = { kind: 'pinch', pointerId: event.pointerId, distance: span.distance, anchor: toWorld(span.middle), camera: camera.value }
      return
    }
    if (pointers.size > 2) return
    suppressClick = false
    gesture = { kind: 'press', pointerId: event.pointerId, id: skillOf(event.target), start: local(event) }
  }
  function pointerMove(event: PointerEvent): void {
    if (!pointers.has(event.pointerId)) return
    const at = local(event)
    pointers.set(event.pointerId, at)
    if (gesture?.kind === 'pinch') {
      const span = pinchSpan()
      if (span) setCamera(cameraHolding(gesture.anchor, span.middle, clampZoom(gesture.camera.zoom * span.distance / gesture.distance)))
      return
    }
    if (!gesture || gesture.pointerId !== event.pointerId) return
    if (gesture.kind === 'press') {
      if (Math.hypot(at.x - gesture.start.x, at.y - gesture.start.y) < WEB_DRAG_THRESHOLD) return
      suppressClick = true
      root.value?.setPointerCapture?.(event.pointerId)
      // Whatever the camera shows when a gesture starts, it holds still while dots move.
      if (!localCamera.value && !input().camera) setCamera(camera.value)
      const point = gesture.id ? positions.value.get(gesture.id) : undefined
      if (gesture.id && point) {
        const grabbed = toWorld(gesture.start)
        gesture = { kind: 'dot', pointerId: gesture.pointerId, id: gesture.id, offset: { x: grabbed.x - point.x, y: grabbed.y - point.y } }
      } else gesture = { kind: 'pan', pointerId: gesture.pointerId, start: gesture.start, camera: camera.value }
    }
    if (gesture.kind === 'dot') {
      const world = toWorld(at)
      moveHeld(gesture.id, { x: world.x - gesture.offset.x, y: world.y - gesture.offset.y })
    } else if (gesture.kind === 'pan') {
      const from = gesture.camera
      setCamera({ ...from, x: from.x - (at.x - gesture.start.x) / from.zoom, y: from.y - (at.y - gesture.start.y) / from.zoom })
    }
  }
  function pointerUp(event: PointerEvent): void {
    pressEnd()
    pointers.delete(event.pointerId)
    if (gesture?.kind === 'pinch') { if (pointers.size < 2) gesture = null; return }
    if (!gesture || gesture.pointerId !== event.pointerId) return
    if (gesture.kind === 'dot') releaseHeld()
    gesture = null
    if (root.value?.hasPointerCapture?.(event.pointerId)) root.value.releasePointerCapture(event.pointerId)
  }
  /** A click that ends a drag or a pan selects nothing. Keyboard clicks (detail 0) always pass. */
  function clickCapture(event: MouseEvent): void {
    if (!suppressClick || event.detail === 0) return
    suppressClick = false
    event.stopPropagation()
    event.preventDefault()
  }

  function dotClick(id: string): void { events.select(id) }

  function skillOf(target: EventTarget | null): string | null {
    const element = target instanceof Element ? target.closest<HTMLElement>('[data-skill]') : null
    return element && root.value?.contains(element) ? element.dataset.skill ?? null : null
  }
  /** The control of a dot: the element that carries `data-skill`, or the button inside it (the explorer's node wrapper). */
  function dotElement(id: string): HTMLElement | undefined {
    const element = [...root.value?.querySelectorAll<HTMLElement>('[data-skill]') ?? []].find(item => item.dataset.skill === id)
    return element instanceof HTMLButtonElement ? element : element?.querySelector<HTMLElement>('button') ?? element
  }
  /** Puts focus on the roving dot, such as when an owner moves focus into the graph. */
  function focus(): void {
    if (rovingId.value) dotElement(rovingId.value)?.focus()
  }
  function focusIn(event: FocusEvent): void {
    const id = skillOf(event.target)
    if (!id) return
    focusedId.value = id
    activeId.value = id
    keyboardFocus.value = !pressing
  }
  function focusOut(event: FocusEvent): void {
    if (event.relatedTarget instanceof Node && root.value?.contains(event.relatedTarget)) return
    focusedId.value = null
    keyboardFocus.value = false
  }
  const ARROWS: Record<string, WebDirection> = { ArrowUp: 'up', ArrowDown: 'down', ArrowLeft: 'left', ArrowRight: 'right' }
  function keyDown(event: KeyboardEvent): void {
    const direction = ARROWS[event.key]
    if (direction) {
      // The arrow keys rove; they never scroll the page or pan the graph.
      event.preventDefault()
      keyboardFocus.value = true
      const from = rovingId.value
      const next = from ? webNearestInDirection(from, direction, positions.value, shownSet.value) : null
      if (next) { activeId.value = next; dotElement(next)?.focus() }
      return
    }
    const id = skillOf(event.target)
    if ((event.key === 'Enter' || event.key === ' ') && id) {
      // Handled here, so the button's own activation does not select twice.
      event.preventDefault()
      keyboardFocus.value = true
      events.select(id)
    } else if (event.key === 'Escape' && input().selectedId !== null) {
      event.stopPropagation()
      events.select(null)
    }
  }
  function keyUp(event: KeyboardEvent): void {
    if (event.key === ' ' && skillOf(event.target)) event.preventDefault()
  }
  function pressStart(): void { pressing = true }
  function pressEnd(): void { pressing = false }
  function hover(id: string | null): void { hoverId.value = id }

  return {
    size, camera, dots, lines, graph, motionReduced, rovingId, namedIds, litIds,
    zoomBy, panBy, wheel, dotClick, pointerDown, pointerMove, pointerUp, clickCapture, focus, focusIn, focusOut, keyDown, keyUp, hover,
  }
}
