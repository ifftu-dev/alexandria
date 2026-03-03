const HOVER_ALPHA_KEY = '__hoverAlpha' as const
const LABEL_ALPHA_KEY = '__labelAlpha' as const

const LERP_SPEED = 0.12

function lerp(current: number, target: number, speed: number): number {
  const delta = target - current
  if (Math.abs(delta) < 0.005) return target
  return current + delta * speed
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
    const radius = isEarned ? 6 : isAvailable ? 5 : 4
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

    ctx.beginPath()
    ctx.arc(x, y, radius, 0, 2 * Math.PI)
    ctx.fillStyle = isEarned ? '#22c55e' : isAvailable ? '#eab308' : 'rgba(148, 163, 184, 0.4)'
    ctx.fill()

    if (labelAlpha > 0.01) {
      const label = node.name as string
      const fontSize = Math.min(12, 10 / globalScale)
      ctx.font = `${fontSize}px sans-serif`
      ctx.textAlign = 'center'
      ctx.textBaseline = 'top'
      const baseColor = isEarned
        ? [34, 197, 94]
        : isAvailable
          ? [234, 179, 8]
          : [148, 163, 184]
      const baseOpacity = isEarned ? 0.9 : isAvailable ? 0.8 : 0.6
      ctx.fillStyle = `rgba(${baseColor[0]}, ${baseColor[1]}, ${baseColor[2]}, ${baseOpacity * labelAlpha * alpha})`
      ctx.fillText(label, x, y + radius + 2)
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

  return {
    buildAdjacency,
    createHoverHandler,
    renderNode,
    renderLink,
  }
}
