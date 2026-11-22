import { describe, expect, it } from 'vitest'
import type { PackageActivity } from '@/types'
import { launchables, outline } from './packageTree'

const activity = (id: string, over: Partial<PackageActivity> = {}): PackageActivity =>
  ({ id, title: id, visible: true, ...over }) as PackageActivity

const lesson = (id: string, over: Partial<PackageActivity> = {}) =>
  activity(id, { launch: { path: `${id}.html`, kind: 'sco' }, ...over } as Partial<PackageActivity>)

// The workshop package: a heading of two lessons, an asset, and a hidden lesson.
const tree = activity('root', {
  children: [
    activity('getting-started', { children: [lesson('welcome'), lesson('quiz')] }),
    lesson('summary'),
    lesson('answers', { visible: false }),
  ],
})

describe('package tree', () => {
  it('lists the launchable activities in package order, hidden ones included', () => {
    expect(launchables(tree).map(item => item.id)).toEqual(['welcome', 'quiz', 'summary', 'answers'])
  })

  it('outlines the visible activities under the root with their depth', () => {
    expect(outline(tree.children).map(row => [row.item.id, row.depth])).toEqual([
      ['getting-started', 0], ['welcome', 1], ['quiz', 1], ['summary', 0],
    ])
  })
})
