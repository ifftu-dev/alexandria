import type ForceGraph from 'force-graph'
import type { LinkObject, NodeObject } from 'force-graph'

export type SkillGraphStatus = 'earned' | 'available' | 'locked'

export interface SkillGraphNode extends NodeObject {
  id: string
  name: string
  routeId: string
  status: SkillGraphStatus
  prerequisites: string[]
  bloom_level: string
  proficiency?: string
  confidence?: number
  __halfW?: number
  __halfH?: number
  [key: string]: unknown
}

export interface SkillGraphLink extends LinkObject<SkillGraphNode> {
  source: string | number | SkillGraphNode
  target: string | number | SkillGraphNode
  [key: string]: unknown
}

export type ForceGraphInstance = ForceGraph<SkillGraphNode, SkillGraphLink>

type RuntimeForceGraphFactory = () => (element: HTMLElement) => ForceGraphInstance

/**
 * The package declaration describes a constructable class, while its browser
 * bundle exposes the Kapsule-style `ForceGraph()(element)` factory. Keep that
 * compatibility assertion at this integration boundary instead of weakening
 * every graph consumer.
 */
export async function createForceGraph(element: HTMLElement): Promise<ForceGraphInstance> {
  const module = await import('force-graph')
  const factory = module.default as unknown as RuntimeForceGraphFactory
  return factory()(element)
}
