// Bloom's taxonomy ordering, palette, and node-size helpers — a single source
// of truth shared by every skill-graph renderer and badge, so the encoding
// (color + size + badge variant) stays consistent and doesn't drift.

export const BLOOM_ORDER = [
  'remember',
  'understand',
  'apply',
  'analyze',
  'evaluate',
  'create',
] as const

export type BloomLevel = (typeof BLOOM_ORDER)[number]

/** 0 (remember) … 5 (create). Unknown/missing levels fall back to 'apply' (2),
 *  matching the `skills.bloom_level` DB default. */
export function bloomRank(level: string | null | undefined): number {
  const i = BLOOM_ORDER.indexOf((level ?? '').toLowerCase() as BloomLevel)
  return i < 0 ? 2 : i
}

/** Canonical fill colors for graph nodes, keyed by Bloom level. */
export const BLOOM_FILLS: Record<BloomLevel, string> = {
  remember: '#94a3b8',
  understand: '#6366f1',
  apply: '#a855f7',
  analyze: '#f59e0b',
  evaluate: '#10b981',
  create: '#e11d48',
}

export function bloomFill(level: string | null | undefined): string {
  return BLOOM_FILLS[(level ?? '').toLowerCase() as BloomLevel] ?? '#6366f1'
}

/** AppBadge variant per Bloom level. Centralizes the map that was duplicated
 *  across the skills pages. */
export type BloomBadgeVariant =
  | 'secondary'
  | 'primary'
  | 'accent'
  | 'warning'
  | 'success'
  | 'governance'

export const BLOOM_BADGE: Record<BloomLevel, BloomBadgeVariant> = {
  remember: 'secondary',
  understand: 'primary',
  apply: 'accent',
  analyze: 'warning',
  evaluate: 'success',
  create: 'governance',
}

export function bloomBadge(level: string | null | undefined): BloomBadgeVariant {
  return BLOOM_BADGE[(level ?? '').toLowerCase() as BloomLevel] ?? 'primary'
}

/** Graph-node draw radius scaled by Bloom level: bigger = higher level.
 *  radius = base + rank * step  (rank 0..5). Used for both the rendered
 *  circle and the pointer hit-area so clicks/hover stay aligned. */
export function bloomRadius(level: string | null | undefined, base: number, step: number): number {
  return base + bloomRank(level) * step
}
