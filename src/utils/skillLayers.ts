import type { SkillGraphEdge } from '@/types'

/**
 * Layers a prerequisite DAG by longest path from its roots: a skill with no
 * loaded prerequisites is layer 0, any other skill is one past its deepest
 * prerequisite. Edges to skills outside `nodeIds` are ignored. A cycle is cut
 * where the walk meets a skill it is still inside, which counts as layer 0, so
 * every skill gets a finite layer.
 *
 * Entries are inserted as each skill's layer is settled (prerequisites
 * first), walking `nodeIds` in order; callers that group by layer rely on it.
 */
export function longestPathLayers(
  nodeIds: Iterable<string>,
  edges: readonly Pick<SkillGraphEdge, 'skill_id' | 'prerequisite_id'>[],
): Map<string, number> {
  const ids = new Set(nodeIds)
  const prerequisites = new Map<string, string[]>()
  for (const edge of edges) {
    if (!ids.has(edge.skill_id) || !ids.has(edge.prerequisite_id)) continue
    const list = prerequisites.get(edge.skill_id)
    if (list) list.push(edge.prerequisite_id)
    else prerequisites.set(edge.skill_id, [edge.prerequisite_id])
  }

  const layers = new Map<string, number>()
  function layerOf(id: string, visited: Set<string>): number {
    const settled = layers.get(id)
    if (settled !== undefined) return settled
    if (visited.has(id)) return 0
    visited.add(id)
    const prereqs = prerequisites.get(id) ?? []
    const deepest = prereqs.length > 0 ? Math.max(...prereqs.map(p => layerOf(p, visited))) : -1
    const layer = deepest + 1
    layers.set(id, layer)
    return layer
  }
  for (const id of ids) layerOf(id, new Set())
  return layers
}
