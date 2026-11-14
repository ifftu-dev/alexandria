import { describe, expect, it } from 'vitest'
import { longestPathLayers } from './skillLayers'

const edge = (skillId: string, prerequisiteId: string) => ({ skill_id: skillId, prerequisite_id: prerequisiteId })

describe('longest-path skill layers', () => {
  it('puts each skill one layer past its deepest prerequisite and ignores skills that are not loaded', () => {
    // a → b → d, a → c → d, plus a shortcut a → d that must not pull d forward.
    const layers = longestPathLayers(['d', 'c', 'b', 'a', 'lone'], [
      edge('b', 'a'), edge('c', 'a'), edge('d', 'b'), edge('d', 'c'), edge('d', 'a'), edge('b', 'unloaded'),
    ])
    expect(Object.fromEntries(layers)).toEqual({ a: 0, b: 1, c: 1, d: 2, lone: 0 })
  })

  it('cuts a prerequisite cycle where the walk re-enters it, as the public graph always has', () => {
    // x and y require each other; z requires x. Walking from x meets x again
    // inside y, which counts as layer 0.
    const layers = longestPathLayers(['x', 'y', 'z'], [edge('x', 'y'), edge('y', 'x'), edge('z', 'x')])
    expect(Object.fromEntries(layers)).toEqual({ y: 1, x: 2, z: 3 })
  })
})
