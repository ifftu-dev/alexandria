import { bloomRadius } from '@/utils/bloom'

const HOVER_ALPHA_KEY = '__hoverAlpha' as const
const LABEL_ALPHA_KEY = '__labelAlpha' as const

// Node radius now encodes Bloom level (bigger = higher); status is still
// conveyed by color + glow. base 4 (remember) … 9 (create).
function nodeRadius(node: Record<string, unknown>): number {
  return bloomRadius((node as { bloom_level?: string }).bloom_level, 4, 1)
}

const LERP_SPEED = 0.12

function lerp(current: number, target: number, speed: number): number {
  const delta = target - current
  if (Math.abs(delta) < 0.005) return target
  return current + delta * speed
}

// --- label-aware collision (Obsidian-style) -----------------------------
// Labels are drawn centered under each node (see `renderNode`). Two nodes
// whose label boxes would overlap get pushed apart so the text stays legible.
// We model each node as the AABB enclosing its circle *and* its label and
// separate overlapping boxes along their axis of least penetration — like
// Obsidian's graph, where nodes spread to make room for their labels.
//
// Geometry uses a fixed reference font size (the label size at 1× zoom). The
// labels rendered when zoomed in are smaller in graph units, so a box sized at
// 1× is a generous lower bound — guaranteeing no overlap once labels appear.
// Upper bound of renderNode's label size (min(12, 10/globalScale)) so the
// reserved boxes cover the *largest* rendered labels — including a label shown
// on hover while zoomed out, where the graph-space font reaches 12.
const LABEL_FONT_PX = 12
const LABEL_GAP = 2 // px below the node where the label starts
const LABEL_PAD = 3 // breathing room around each box
const COLLIDE_STRENGTH = 0.5

interface SimNode {
  x?: number
  y?: number
  vx?: number
  vy?: number
  fx?: number | null
  fy?: number | null
  name?: string
  bloom_level?: string
  __halfW?: number
  __halfH?: number
}

let __measureCtx: CanvasRenderingContext2D | null = null

/**
 * Precompute each node's label-box half-extents (position-independent, so done
 * once per layout). The box encloses the node circle and its label. Exported so
 * callers can also size the link force from it (linked nodes must sit far enough
 * apart that their labels clear).
 */
export function measureLabelBoxes(nodes: SimNode[]) {
  if (!__measureCtx) {
    __measureCtx = document.createElement('canvas').getContext('2d')
    if (__measureCtx) __measureCtx.font = `${LABEL_FONT_PX}px sans-serif`
  }
  for (const n of nodes) {
    const r = nodeRadius(n as Record<string, unknown>)
    const label = n.name ?? ''
    const lw = __measureCtx ? __measureCtx.measureText(label).width : label.length * 6.5
    n.__halfW = Math.max(r, lw / 2) + LABEL_PAD
    n.__halfH = r + (LABEL_GAP + LABEL_FONT_PX) / 2 + LABEL_PAD
  }
}

/** Rest length so two linked nodes' label boxes clear side by side. */
export function labelLinkDistance(
  source: { __halfW?: number } | undefined,
  target: { __halfW?: number } | undefined,
  floor = 50,
): number {
  return Math.max(floor, (source?.__halfW ?? 0) + (target?.__halfW ?? 0) + 6)
}

/** A d3-force-compatible force that separates overlapping node+label boxes. */
export function createLabelCollisionForce() {
  let nodes: SimNode[] = []

  function initialize(ns: SimNode[]) {
    nodes = ns
    measureLabelBoxes(nodes)
  }

  function force(alpha: number) {
    const n = nodes.length
    for (let i = 0; i < n; i++) {
      const a = nodes[i]!
      const ax = a.x ?? 0
      const ay = a.y ?? 0
      const ahw = a.__halfW ?? 0
      const ahh = a.__halfH ?? 0
      for (let j = i + 1; j < n; j++) {
        const b = nodes[j]!
        const dx = (b.x ?? 0) - ax
        const dy = (b.y ?? 0) - ay
        const ox = ahw + (b.__halfW ?? 0) - Math.abs(dx)
        if (ox <= 0) continue
        const oy = ahh + (b.__halfH ?? 0) - Math.abs(dy)
        if (oy <= 0) continue
        // Boxes overlap — nudge apart along the shallower axis so they slide
        // out the short way. Split the correction between the two nodes.
        if (ox < oy) {
          const sign = dx === 0 ? (i % 2 === 0 ? 1 : -1) : Math.sign(dx)
          const push = ox * COLLIDE_STRENGTH * alpha * 0.5
          if (a.fx == null) a.vx = (a.vx ?? 0) - sign * push
          if (b.fx == null) b.vx = (b.vx ?? 0) + sign * push
        } else {
          const sign = dy === 0 ? (i % 2 === 0 ? 1 : -1) : Math.sign(dy)
          const push = oy * COLLIDE_STRENGTH * alpha * 0.5
          if (a.fy == null) a.vy = (a.vy ?? 0) - sign * push
          if (b.fy == null) b.vy = (b.vy ?? 0) + sign * push
        }
      }
    }
  }

  ;(force as unknown as { initialize: typeof initialize }).initialize = initialize
  return force
}

interface HoverState {
  hoveredId: string | null
  connectedIds: Set<string>
  adjacencyMap: Map<string, Set<string>>
}

export function useSkillGraphHover() {
  const state: HoverState = {
    hoveredId: null,
    connectedIds: new Set(),
    adjacencyMap: new Map(),
  }

  function buildAdjacency(links: Array<{ source: string; target: string }>) {
    state.adjacencyMap = new Map()
    for (const link of links) {
      if (!state.adjacencyMap.has(link.source)) state.adjacencyMap.set(link.source, new Set())
      if (!state.adjacencyMap.has(link.target)) state.adjacencyMap.set(link.target, new Set())
      state.adjacencyMap.get(link.source)!.add(link.target)
      state.adjacencyMap.get(link.target)!.add(link.source)
    }
  }

  function createHoverHandler() {
    return (node: Record<string, unknown> | null) => {
      if (node) {
        const id = node.id as string
        state.hoveredId = id
        state.connectedIds = state.adjacencyMap.get(id) ?? new Set()
      } else {
        state.hoveredId = null
        state.connectedIds = new Set()
      }
    }
  }

  function renderNode(
    node: Record<string, unknown>,
    ctx: CanvasRenderingContext2D,
    globalScale: number,
  ) {
    const nodeId = node.id as string
    const status = (node as { status?: string }).status ?? 'locked'
    const isEarned = status === 'earned'
    const isAvailable = status === 'available'
    const radius = nodeRadius(node)
    const x = node.x as number
    const y = node.y as number

    const hovered = state.hoveredId
    const isHighlighted = !hovered || nodeId === hovered || state.connectedIds.has(nodeId)
    const isHoveredNode = nodeId === hovered
    const targetAlpha = hovered ? (isHighlighted ? 1.0 : 0.08) : 1.0
    const currentAlpha = (node[HOVER_ALPHA_KEY] as number) ?? 1.0
    const alpha = lerp(currentAlpha, targetAlpha, LERP_SPEED)
    ;(node as Record<string, number>)[HOVER_ALPHA_KEY] = alpha

    const showLabelTarget = (hovered && isHighlighted) ? 1.0 : (globalScale > 1.5 ? 1.0 : 0.0)
    const currentLabelAlpha = (node[LABEL_ALPHA_KEY] as number) ?? (globalScale > 1.5 ? 1.0 : 0.0)
    const labelAlpha = lerp(currentLabelAlpha, showLabelTarget, LERP_SPEED)
    ;(node as Record<string, number>)[LABEL_ALPHA_KEY] = labelAlpha

    ctx.globalAlpha = alpha

    if (isEarned) {
      ctx.beginPath()
      ctx.arc(x, y, radius + 4, 0, 2 * Math.PI)
      ctx.fillStyle = 'rgba(34, 197, 94, 0.15)'
      ctx.fill()
    }

    if (isAvailable) {
      ctx.beginPath()
      ctx.arc(x, y, radius + 3, 0, 2 * Math.PI)
      ctx.fillStyle = 'rgba(234, 179, 8, 0.12)'
      ctx.fill()
    }

    if (isHoveredNode && alpha > 0.5) {
      const ringAlpha = (alpha - 0.5) * 2
      ctx.beginPath()
      ctx.arc(x, y, radius + 5, 0, 2 * Math.PI)
      ctx.strokeStyle = isEarned
        ? `rgba(34, 197, 94, ${0.5 * ringAlpha})`
        : isAvailable
          ? `rgba(234, 179, 8, ${0.5 * ringAlpha})`
          : `rgba(148, 163, 184, ${0.4 * ringAlpha})`
      ctx.lineWidth = 1.5
      ctx.stroke()
    }

    // Opaque backdrop disc so connector lines (drawn under the nodes) never
    // show through — the locked-grey fill below is translucent.
    ctx.beginPath()
    ctx.arc(x, y, radius, 0, 2 * Math.PI)
    ctx.fillStyle = 'rgb(2, 6, 23)'
    ctx.fill()

    ctx.beginPath()
    ctx.arc(x, y, radius, 0, 2 * Math.PI)
    ctx.fillStyle = isEarned ? '#22c55e' : isAvailable ? '#eab308' : 'rgba(148, 163, 184, 0.4)'
    ctx.fill()

    if (labelAlpha > 0.01) {
      const label = node.name as string
      const fontSize = Math.min(12, 10 / globalScale)
      const subFont = Math.min(10, 8 / globalScale)
      const labelY = y + radius + 2
      ctx.textAlign = 'center'
      ctx.textBaseline = 'top'

      // Hovered nodes get a second line spelling out the Bloom level (size
      // already encodes it; the text spells it out for the focused node).
      ctx.font = `${fontSize}px sans-serif`
      const nameW = ctx.measureText(label).width
      // Bloom line, only on the hovered node — the user's *proven* proficiency
      // (VC-derived), not the skill's intrinsic level. Defaults to "Remember"
      // once earned and climbs with evidence; "Unknown" until then.
      let bloomText = ''
      if (isHoveredNode) {
        const prof = (node as { proficiency?: string }).proficiency
        if (status === 'earned' && prof) {
          const conf = (node as { confidence?: number }).confidence
          const pct = conf != null && conf > 0 ? ` · ${Math.round(conf * 100)}%` : ''
          bloomText = prof.charAt(0).toUpperCase() + prof.slice(1) + pct
        } else {
          bloomText = 'Unknown'
        }
      }
      let bloomW = 0
      if (bloomText) {
        ctx.font = `${subFont}px sans-serif`
        bloomW = ctx.measureText(bloomText).width
      }

      // Background halo behind the label so the connector lines (drawn under
      // the nodes) don't show through the text. Filled with the graph's dark
      // backdrop colour, faded with the label.
      const boxW = Math.max(nameW, bloomW)
      const boxH = fontSize + (bloomText ? 1 + subFont : 0)
      const padX = 3
      const padY = 1.5
      ctx.fillStyle = `rgba(2, 6, 23, ${0.85 * labelAlpha * alpha})`
      ctx.fillRect(x - boxW / 2 - padX, labelY - padY, boxW + padX * 2, boxH + padY * 2)

      // Name
      ctx.font = `${fontSize}px sans-serif`
      const baseColor = isEarned
        ? [34, 197, 94]
        : isAvailable
          ? [234, 179, 8]
          : [148, 163, 184]
      const baseOpacity = isEarned ? 0.9 : isAvailable ? 0.8 : 0.6
      ctx.fillStyle = `rgba(${baseColor[0]}, ${baseColor[1]}, ${baseColor[2]}, ${baseOpacity * labelAlpha * alpha})`
      ctx.fillText(label, x, labelY)

      // Bloom level (hover only)
      if (bloomText) {
        ctx.font = `${subFont}px sans-serif`
        ctx.fillStyle = `rgba(148, 163, 184, ${0.75 * labelAlpha * alpha})`
        ctx.fillText(bloomText, x, labelY + fontSize + 1)
      }
    }

    ctx.globalAlpha = 1.0
  }

  function renderLink(link: Record<string, unknown>, ctx: CanvasRenderingContext2D) {
    const source = link.source as Record<string, unknown>
    const target = link.target as Record<string, unknown>
    if (!source || !target) return

    const sx = source.x as number
    const sy = source.y as number
    const tx = target.x as number
    const ty = target.y as number
    if (sx == null || sy == null || tx == null || ty == null) return

    const hovered = state.hoveredId
    const sourceId = source.id as string
    const targetId = target.id as string
    const isConnected = hovered ? (sourceId === hovered || targetId === hovered) : false

    const linkKey = '__linkAlpha' as const
    const targetAlpha = hovered ? (isConnected ? 0.9 : 0.03) : 0.15
    const currentAlpha = (link[linkKey] as number) ?? 0.15
    const alpha = lerp(currentAlpha, targetAlpha, LERP_SPEED)
    ;(link as Record<string, number>)[linkKey] = alpha

    const widthKey = '__linkWidth' as const
    const targetWidth = hovered ? (isConnected ? 1.5 : 0.3) : 1
    const currentWidth = (link[widthKey] as number) ?? 1
    const width = lerp(currentWidth, targetWidth, LERP_SPEED)
    ;(link as Record<string, number>)[widthKey] = width

    ctx.beginPath()
    ctx.moveTo(sx, sy)
    ctx.lineTo(tx, ty)
    ctx.strokeStyle = isConnected && hovered
      ? `rgba(220, 230, 240, ${alpha})`
      : `rgba(148, 163, 184, ${alpha})`
    ctx.lineWidth = width
    ctx.stroke()
  }

  // Keep the clickable/hover hit-area aligned with the Bloom-scaled drawn
  // radius — force-graph otherwise sizes the pointer area from `nodeVal`.
  function nodePointerAreaPaint(
    node: Record<string, unknown>,
    color: string,
    ctx: CanvasRenderingContext2D,
  ) {
    ctx.fillStyle = color
    ctx.beginPath()
    ctx.arc(node.x as number, node.y as number, nodeRadius(node), 0, 2 * Math.PI)
    ctx.fill()
  }

  return {
    buildAdjacency,
    createHoverHandler,
    renderNode,
    renderLink,
    nodePointerAreaPaint,
  }
}
