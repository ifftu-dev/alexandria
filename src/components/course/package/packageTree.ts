// Walks of a package's activity tree, shared by the player, the session and
// the course editor. Works on the imported description (`PackageActivity`)
// and on the SCORM 2004 engine's view of it (`SequencingNode`).

interface TreeNode<T> {
  visible: boolean
  children?: readonly T[]
}

/** One row of an outline: an activity and how deep it sits under the root's children. */
export interface OutlineRow<T> {
  item: T
  depth: number
}

/** The visible activities among `children` and below, in order, with their depth; a hidden one hides its subtree. */
export function outline<T extends TreeNode<T>>(children: readonly T[] | undefined): OutlineRow<T>[] {
  const out: OutlineRow<T>[] = []
  const walk = (item: T, depth: number) => {
    if (!item.visible) return
    out.push({ item, depth })
    for (const child of item.children ?? []) walk(child, depth + 1)
  }
  for (const child of children ?? []) walk(child, 0)
  return out
}

/** Every launchable activity (lesson or asset) under `root`, in package order. */
export function launchables<T extends { launch?: unknown, children?: readonly T[] }>(root: T | null | undefined): T[] {
  const out: T[] = []
  const walk = (item: T) => {
    if (item.launch) out.push(item)
    for (const child of item.children ?? []) walk(child)
  }
  if (root) walk(root)
  return out
}
