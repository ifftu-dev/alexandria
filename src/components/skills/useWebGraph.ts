import { computed, onBeforeUnmount, onMounted, ref, shallowRef, watch, type ComputedRef, type Ref } from 'vue'
import { motionDuration, useMotion } from '@/composables/useMotion'
import { compareIds, type AtlasModel, type AtlasNode } from '@/utils/skillAtlas'
import type { AtlasCamera } from '@/utils/skillAtlasView'
import { atlasFieldColor } from './atlasPresentation'
import { continueWebLayout, webDotRadius, webNamePlaces, webNearestInDirection, webNearestTo, type WebDirection, type WebGraph, type WebNamePlace, type WebPoint } from '@/utils/webGeometry'

// Camera and drawing constants. Positions and dot radii are layout units; the
// camera's zoom is screen pixels per layout unit. Lines, arrowheads and names
// keep their size on screen at any zoom.
//
// The camera never re-renders the dots. The pan is one transform on the
// graph's layers. The zoom is written straight onto each dot's element (its
// box and its skin's diameter), so a zoom costs one pass over the dots and no
// render; a CSS variable on a shared ancestor would restyle every element
// under it, which is several times slower on a large graph. Only the names,
// which are few and placed in screen pixels, and the arrowheads follow the
// camera through a render.
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
/** Smallest dot drawn in the full and the compact graph, and smallest pointer target around a dot, in pixels. */
export const WEB_DOT_MIN_PIXELS = 4
export const WEB_COMPACT_DOT_MIN_PIXELS = 6
export const WEB_HIT_PIXELS = 14
/** A press that moves this many pixels is a drag, not a click. */
export const WEB_DRAG_THRESHOLD = 4
/** Zoom per pixel of wheel travel: one notch (about 100 pixels) zooms by about a fifth. */
export const WEB_WHEEL_RATE = 0.002
/** Zoom of one press of a zoom button, as a factor. */
export const WEB_ZOOM_STEP = 1.25

/**
 * Names, in pixels, in the full and the compact graph (match WebGraphDot's
 * styles): type size, widest name and line height, and an estimate of a
 * character's width where a name cannot be measured.
 */
export const WEB_NAME = { fontPixels: 12, maxPixels: 160, linePixels: 16, charPixels: 6.8 } as const
export const WEB_COMPACT_NAME = { fontPixels: 10, maxPixels: 110, linePixels: 13, charPixels: 5.6 } as const
/** Weight of a name, and of the selected skill's name. */
export const WEB_NAME_WEIGHT = 550
export const WEB_SELECTED_NAME_WEIGHT = 650
/** Room added to a measured name for its outline. */
export const WEB_NAME_SLACK_PIXELS = 2
/** How far the selection ring reaches past a dot's edge (match WebGraphDot's ring). */
export const WEB_SELECTION_RING_PIXELS = 4

/** Length of an arrowhead, in pixels. */
export const WEB_ARROW_PIXELS = 7

export type WebDotState = 'earned' | 'ready-next' | 'ready'

/** One SVG path per emphasis: lines in the lit set, faded outside it (or outside a search), plain when nothing is lit. */
export interface WebLinePaths { plain: string; lit: string; faded: string }
/** Prerequisite lines, dotted relation lines and the arrowheads at the dependent end of prerequisite lines. */
export interface WebLines { prerequisites: WebLinePaths; relations: WebLinePaths; heads: WebLinePaths }

/** One dot: layout position and radius (layout units; `placeDots` places and sizes its element for the zoom), and how it reads now. */
export interface WebDot {
  node: AtlasNode
  id: string
  x: number
  y: number
  radius: number
  color: string
  state: WebDotState
  selected: boolean
  /** The one dot in the Tab order. */
  roving: boolean
  /** Outside the lit neighbourhood, or not matching the search or field focus. */
  faded: boolean
  /** Its name may show (see `WebName` for the names drawn). */
  labelled: boolean
}

/** One name as drawn, in layer pixels (the layers move with the pan): its dot's centre and diameter, and where the name sits. */
export interface WebName {
  id: string
  name: string
  left: number
  top: number
  dot: number
  /** The side of its dot, chosen so no two names overlap, */
  place: WebNamePlace
  /** and how many pixels further out than the usual gap. */
  offset: number
  selected: boolean
  faded: boolean
}

/** What a Web graph draws and how; the component's props, read through a getter so the composable sees changes. */
export interface WebGraphInput {
  model: AtlasModel
  /** The model's Web graph, computed once per model by its owner (`webLayoutEntry`). */
  graph: WebGraph
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
  /** Shown is the selection's surroundings: every shown dot is named and the selection lights nothing at rest. */
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
  /** The hovered or keyboard-focused dot, for owners that follow it. */
  active?: (id: string | null) => void
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
 * `layers` are the elements the pan moves; the dots, lines and names inside
 * them sit at their layout position times the zoom.
 */
export function useWebGraph(source: () => WebGraphInput, events: WebGraphEvents, root: Readonly<Ref<HTMLElement | undefined>>,
  layers: () => readonly (Element | null | undefined)[]) {
  // The owner's getter builds a new object on every call, so it is read once
  // per change of the owner's state, never once per dot or per frame.
  const current = computed(source)
  // Each input is read through a computed of its own, so a change to one (the
  // camera, above all) re-runs only what reads that one: a published pan or
  // zoom must not recompute the dots.
  const fields: { [K in keyof WebGraphInput]?: ComputedRef<WebGraphInput[K]> } = {}
  function field<K extends keyof WebGraphInput>(key: K): WebGraphInput[K] {
    let ref = fields[key] as ComputedRef<WebGraphInput[K]> | undefined
    if (!ref) { ref = computed(() => current.value[key]); (fields as Record<K, ComputedRef<WebGraphInput[K]>>)[key] = ref }
    return ref.value
  }
  const { reduced: systemReduced } = useMotion()
  const motionReduced = computed(() => field('reducedMotion') || systemReduced.value)
  const model = computed(() => field('model'))
  const graph = computed(() => field('graph'))

  // Size: measured from the root, with a default until then. The root may
  // come and go (the explorer draws its map only in some views), so it is
  // watched rather than read once at mount.
  const measured = ref<{ width: number; height: number } | null>(null)
  const size = computed(() => measured.value ?? (field('compact') ? WEB_COMPACT_DEFAULT_SIZE : WEB_DEFAULT_SIZE))
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
  const positions = computed(() => working.value ?? field('positions'))
  /** Every dot drawn, in model order. */
  const shownNodes = computed(() => {
    const shown = field('shownIds')
    return model.value.nodes.filter(node => positions.value.has(node.skill.id) && (!shown || shown.has(node.skill.id)))
  })
  const shownSet = computed(() => new Set(shownNodes.value.map(node => node.skill.id)))

  // Camera: the owner's, or the camera that fits what is shown.
  const fitCamera = computed(() => {
    const fit = field('fitIds')
    const ids = fit && [...fit].some(id => shownSet.value.has(id)) ? [...fit].filter(id => shownSet.value.has(id)) : [...shownSet.value]
    const points = ids.flatMap(id => { const point = positions.value.get(id); return point ? [point] : [] })
    const compact = field('compact'), names = field('names'), narrowed = field('narrowed')
    return webFitCamera(points, size.value.width, size.value.height, compact ? WEB_COMPACT_FIT_PADDING : names || narrowed ? WEB_NAMED_FIT_PADDING : WEB_FIT_PADDING)
  })
  /** A camera this graph moved to and has emitted, until the owner's camera changes. */
  const localCamera = shallowRef<AtlasCamera | null>(null)
  watch(() => field('camera'), () => { localCamera.value = null })
  const camera = computed<AtlasCamera>(() => localCamera.value ?? field('camera') ?? fitCamera.value)
  function setCamera(next: AtlasCamera): void {
    localCamera.value = next
    events.camera(next)
  }
  /** Where layout point 0, 0 sits on screen: the pan the layers are moved by. */
  const pan = computed(() => {
    const at = camera.value
    return { x: size.value.width / 2 - at.x * at.zoom, y: size.value.height / 2 - at.y * at.zoom }
  })
  /** The layers, as one value that changes only when one of them does. */
  const layerList = computed(() => layers())
  /** The pan on each layer, set directly, so a pan renders nothing. */
  watch([layerList, pan], () => {
    const { x, y } = pan.value
    for (const layer of layerList.value) if (layer instanceof HTMLElement || layer instanceof SVGElement) layer.style.transform = `translate(${x}px, ${y}px)`
  }, { flush: 'sync', immediate: true })
  /** A dot's diameter on screen, in pixels: the size its styles give it. */
  function dotPixels(radius: number): number {
    return Math.max(field('compact') ? WEB_COMPACT_DOT_MIN_PIXELS : WEB_DOT_MIN_PIXELS, radius * 2 * camera.value.zoom)
  }

  // Keyboard roving: one dot is in the Tab order.
  const activeId = ref<string | null>(null)
  const rovingId = computed(() => {
    const selected = field('selectedId')
    if (activeId.value && shownSet.value.has(activeId.value)) return activeId.value
    if (selected && shownSet.value.has(selected)) return selected
    // The middle of what a fitted camera shows, so a pan or zoom never moves the Tab stop.
    return webNearestTo({ x: fitCamera.value.x, y: fitCamera.value.y }, positions.value, shownSet.value)
  })
  watch(() => field('selectedId'), () => { activeId.value = null })

  // Emphasis. Hover, and focus that arrived by keyboard, light a dot's
  // one-step neighbourhood while they last; selection keeps it.
  const hoverId = ref<string | null>(null)
  const focusedId = ref<string | null>(null)
  /** Focus counts only when it came from the keyboard: a click focuses its dot, and selection already lights it. */
  const keyboardFocus = ref(false)
  let pressing = false
  const transientId = computed(() => hoverId.value ?? (keyboardFocus.value ? focusedId.value : null))
  watch(transientId, id => events.active?.(id))
  /** A shown skill and the shown skills one step away: prerequisites, dependents and, with Related on, relations. */
  function neighbourhood(id: string): Set<string> {
    const ids = new Set<string>()
    const node = model.value.nodeById.get(id)
    if (!node || !shownSet.value.has(id)) return ids
    ids.add(id)
    const steps = [...node.prerequisites, ...node.dependents, ...(field('related') ? node.relations : [])]
    for (const step of steps) if (shownSet.value.has(step.skillId)) ids.add(step.skillId)
    return ids
  }
  /** Dots whose names show at any zoom: the selected and the hovered or focused dot, and their neighbours. */
  const namedIds = computed(() => {
    const selected = field('selectedId')
    return new Set([...(selected ? neighbourhood(selected) : []), ...(transientId.value ? neighbourhood(transientId.value) : [])])
  })
  /**
   * The lit set; empty when nothing is lit, so nothing fades. Narrowed, the
   * graph already shows only the selection's surroundings, so the selection
   * keeps nothing lit and only hover, keyboard focus or the inspector's focus
   * light a part of it.
   */
  const litIds = computed(() => {
    const focus = field('focusIds'), selected = field('selectedId'), narrowed = field('narrowed')
    const kept = focus && focus.size > 0 ? [...focus].filter(id => shownSet.value.has(id)) : selected && !narrowed ? [...neighbourhood(selected)] : []
    return new Set([...kept, ...(transientId.value ? neighbourhood(transientId.value) : [])])
  })
  function fadedByMatch(id: string): boolean {
    const matches = field('matchIds')
    return matches !== null && !matches.has(id) && id !== field('selectedId')
  }
  function faded(id: string): boolean {
    return (litIds.value.size > 0 && !litIds.value.has(id)) || fadedByMatch(id)
  }

  function radiusOf(node: AtlasNode): number {
    // An earned skill whose level is unavailable takes the base size, filled.
    return webDotRadius(node.status === 'earned' ? node.earnedBloom?.rank : null)
  }
  /** Every shown dot is named: with the Names toggle, narrowed, or zoomed in past the threshold. */
  const everyName = computed(() => field('names') || field('narrowed') || camera.value.zoom >= WEB_NAMES_ZOOM)
  const dots = computed<WebDot[]>(() => {
    const selected = field('selectedId')
    return shownNodes.value.map((node): WebDot => {
      const id = node.skill.id, point = positions.value.get(id)!
      return {
        node, id, x: point.x, y: point.y, radius: radiusOf(node),
        color: atlasFieldColor(model.value.fieldById.get(node.fieldId) ?? { id: node.fieldId, slot: null }),
        state: node.status === 'earned' ? 'earned' : node.readyNext ? 'ready-next' : 'ready',
        selected: id === selected, roving: id === rovingId.value, faded: faded(id),
        labelled: everyName.value || namedIds.value.has(id),
      }
    })
  })

  // Name sizes. Names are measured in the page's font where the browser can
  // (cached per font and name), and estimated where it cannot, as in tests.
  let measureContext: OffscreenCanvasRenderingContext2D | null | undefined
  const nameWidths = new Map<string, number>()
  function nameWidth(name: string, selected: boolean): number {
    const compact = field('compact'), style = compact ? WEB_COMPACT_NAME : WEB_NAME
    if (measureContext === undefined) measureContext = typeof OffscreenCanvas === 'function' ? new OffscreenCanvas(1, 1).getContext('2d') : null
    const family = measureContext && root.value ? getComputedStyle(root.value).fontFamily : ''
    const font = `${selected ? WEB_SELECTED_NAME_WEIGHT : WEB_NAME_WEIGHT} ${style.fontPixels}px ${family || 'sans-serif'}`
    const key = `${font}\u0000${name}`
    let width = nameWidths.get(key)
    if (width === undefined) {
      if (measureContext) { measureContext.font = font; width = measureContext.measureText(name).width + WEB_NAME_SLACK_PIXELS }
      else width = name.length * style.charPixels + WEB_NAME_SLACK_PIXELS
      nameWidths.set(key, width)
    }
    return Math.min(style.maxPixels, width)
  }
  /**
   * Places and sizes every dot's element for the zoom, in layer pixels: the
   * pointer target's box (an SVG `foreignObject`'s geometry attributes, or an
   * HTML element's transform and size), the skin's diameter, and the lines
   * group's scale. Run after each render and on every zoom, never
   * through one, so a zoom renders no dot. The owner's templates bind none of
   * these.
   */
  /** What was last written to each element, so a write that changes nothing (most dots sit at the smallest size when zoomed out) is skipped. */
  let placed = new WeakMap<Element, { x: string; y: string; hit: string; size: string }>()
  let placedIn: HTMLElement | undefined
  function placeDots(): void {
    const element = root.value
    // A root that went and came back (the explorer leaving Web and returning)
    // had its nodes' attributes reset by the other views: write them all again.
    if (element !== placedIn) { placed = new WeakMap(); placedIn = element }
    if (!element) return
    const { zoom } = camera.value
    const nodes = new Map<string, Element>()
    for (const node of element.querySelectorAll('[data-skill]')) if (node instanceof HTMLElement || node instanceof SVGElement) nodes.set(node.dataset.skill ?? '', node)
    for (const dot of dots.value) {
      const node = nodes.get(dot.id)
      if (!node) continue
      const size = dotPixels(dot.radius), hitPixels = Math.max(WEB_HIT_PIXELS, size)
      const next = { x: (dot.x * zoom - hitPixels / 2).toFixed(1), y: (dot.y * zoom - hitPixels / 2).toFixed(1), hit: hitPixels.toFixed(1), size: size.toFixed(1) }
      const last = placed.get(node)
      if (node instanceof SVGElement) {
        if (last?.x !== next.x) node.setAttribute('x', next.x)
        if (last?.y !== next.y) node.setAttribute('y', next.y)
        if (last?.hit !== next.hit) { node.setAttribute('width', next.hit); node.setAttribute('height', next.hit) }
      } else if (node instanceof HTMLElement) {
        if (last?.x !== next.x || last.y !== next.y) node.style.transform = `translate(${next.x}px, ${next.y}px)`
        if (last?.hit !== next.hit) node.style.width = node.style.height = `${next.hit}px`
      }
      if (last?.size !== next.size) {
        // Width and height rather than an inherited variable, so the skin's own children need no restyle.
        const skin = node.querySelector<HTMLElement>('.web-skin')
        if (skin) skin.style.width = skin.style.height = `${next.size}px`
      }
      placed.set(node, next)
    }
    for (const layer of layerList.value) {
      const lines = layer?.querySelector<SVGGElement>('.web-lines')
      if (lines) lines.style.transform = `scale(${zoom})`
    }
  }
  const zoom = computed(() => camera.value.zoom)
  const compactFlag = computed(() => field('compact'))
  watch([root, layerList, dots, zoom, compactFlag], placeDots, { flush: 'post' })

  const NO_NAMES: readonly WebName[] = Object.freeze([])
  /**
   * The names drawn, placed on screen so that no two overlap: the selected
   * and the hovered or focused dot first, then their neighbours, then larger
   * dots. A name with no free side moves further out from its dot; in the
   * compact graph a neighbour's name is left out instead, while the hovered
   * and selected names always show. Names of dots off screen are not drawn.
   */
  const names = computed<readonly WebName[]>(() => {
    const labelled = dots.value.filter(dot => dot.labelled)
    if (!labelled.length) return NO_NAMES
    const compact = field('compact'), transient = transientId.value, { width, height } = size.value, { zoom } = camera.value, at = pan.value
    const style = compact ? WEB_COMPACT_NAME : WEB_NAME
    const screen = (dot: WebDot) => ({ x: dot.x * zoom + at.x, y: dot.y * zoom + at.y })
    const near = (point: WebPoint) => point.x > -style.maxPixels && point.x < width + style.maxPixels && point.y > -style.linePixels * 2 && point.y < height + style.linePixels * 2
    // The selection ring reaches past the selected dot; names keep clear of it.
    const radius = (dot: WebDot) => dotPixels(dot.radius) / 2 + (dot.selected ? WEB_SELECTION_RING_PIXELS : 0)
    const rank = (dot: WebDot) => dot.selected ? 0 : dot.id === transient ? 1 : namedIds.value.has(dot.id) ? 2 : 3
    const named = labelled.filter(dot => near(screen(dot)))
    if (!named.length) return NO_NAMES
    named.sort((a, b) => rank(a) - rank(b) || b.radius - a.radius || compareIds(a.id, b.id))
    const places = webNamePlaces(named.map(dot => ({ ...screen(dot), id: dot.id, radius: radius(dot), height: style.linePixels,
      width: nameWidth(dot.node.skill.name, dot.selected), optional: !everyName.value && !dot.selected && dot.id !== transient })),
    { obstacles: dots.value.flatMap(dot => { const point = screen(dot); return near(point) ? [{ ...point, radius: radius(dot), firm: dot.selected }] : [] }), compact })
    return named.flatMap((dot): WebName[] => {
      const placement = places.get(dot.id)
      if (!placement) return []
      return [{ id: dot.id, name: dot.node.skill.name, left: dot.x * zoom, top: dot.y * zoom, dot: dotPixels(dot.radius), place: placement.place,
        offset: placement.offset + (dot.selected ? WEB_SELECTION_RING_PIXELS : 0), selected: dot.selected, faded: dot.faded }]
    })
  })

  // Lines run between dot centres in layout units, under the dots, inside a
  // group the owner scales by the zoom with non-scaling strokes, so they keep
  // their width on screen and a pan or zoom never rebuilds them. Arrowheads
  // sit at the dependent dot's edge and keep their size on screen, so they
  // follow the zoom; only lit ones are drawn unless Arrows is on.
  const linePaths = computed(() => {
    const byId = new Map(dots.value.map(dot => [dot.id, dot]))
    const lit = litIds.value
    const out = { prerequisites: { plain: [] as string[], lit: [] as string[], faded: [] as string[] },
      relations: { plain: [] as string[], lit: [] as string[], faded: [] as string[] } }
    const n = (value: number) => value.toFixed(1)
    const emphasis = (a: string, b: string): keyof WebLinePaths =>
      lit.size > 0 && lit.has(a) && lit.has(b) ? 'lit' : lit.size > 0 || fadedByMatch(a) || fadedByMatch(b) ? 'faded' : 'plain'
    const heads: { from: WebDot; to: WebDot; kind: keyof WebLinePaths }[] = []
    for (const link of graph.value.prerequisites) {
      // Drawn from the prerequisite to the skill that builds on it.
      const from = byId.get(link.prerequisite), to = byId.get(link.skill)
      if (!from || !to || (from.x === to.x && from.y === to.y)) continue
      const kind = emphasis(link.prerequisite, link.skill)
      out.prerequisites[kind].push(`M ${n(from.x)} ${n(from.y)} L ${n(to.x)} ${n(to.y)}`)
      heads.push({ from, to, kind })
    }
    if (field('related')) {
      for (const [a, b] of graph.value.relations) {
        const from = byId.get(a), to = byId.get(b)
        if (from && to) out.relations[emphasis(a, b)].push(`M ${n(from.x)} ${n(from.y)} L ${n(to.x)} ${n(to.y)}`)
      }
    }
    const join = (paths: { plain: string[]; lit: string[]; faded: string[] }): WebLinePaths =>
      ({ plain: paths.plain.join(' '), lit: paths.lit.join(' '), faded: paths.faded.join(' ') })
    return { prerequisites: join(out.prerequisites), relations: join(out.relations), heads }
  })
  const headPaths = computed<WebLinePaths>(() => {
    const { zoom } = camera.value, arrows = field('arrows')
    const out = { plain: [] as string[], lit: [] as string[], faded: [] as string[] }
    const n = (value: number) => value.toFixed(2)
    const back = WEB_ARROW_PIXELS / zoom, wing = WEB_ARROW_PIXELS / 2 / zoom
    for (const { from, to, kind } of linePaths.value.heads) {
      if (kind !== 'lit' && !arrows) continue
      const dx = to.x - from.x, dy = to.y - from.y, length = Math.hypot(dx, dy)
      const edge = dotPixels(to.radius) / 2 / zoom
      if (length <= edge + dotPixels(from.radius) / 2 / zoom) continue
      const ux = dx / length, uy = dy / length, tipX = to.x - ux * edge, tipY = to.y - uy * edge
      const bx = tipX - ux * back, by = tipY - uy * back
      out[kind].push(`M ${n(tipX)} ${n(tipY)} L ${n(bx - uy * wing)} ${n(by + ux * wing)} L ${n(bx + uy * wing)} ${n(by - ux * wing)} Z`)
    }
    return { plain: out.plain.join(' '), lit: out.lit.join(' '), faded: out.faded.join(' ') }
  })
  const lines = computed<WebLines>(() => ({ prerequisites: linePaths.value.prerequisites, relations: linePaths.value.relations, heads: headPaths.value }))

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
    if (!field('compact') && !event.ctrlKey && !event.metaKey) return
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
      if (!localCamera.value && !field('camera')) setCamera(camera.value)
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
    } else if (event.key === 'Escape' && field('selectedId') !== null) {
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
    size, camera, pan, dots, names, lines, graph, motionReduced, rovingId, namedIds, litIds, dotPixels,
    zoomBy, panBy, wheel, dotClick, pointerDown, pointerMove, pointerUp, clickCapture, focus, focusIn, focusOut, keyDown, keyUp, hover,
  }
}
