/**
 * The directory list lives in one panel (HolderDirectories) but other panels
 * on the same page describe what happens with it. They hear about changes
 * through this event instead of each holding a stale copy until the page is
 * reopened.
 */
export const DIRECTORIES_CHANGED = 'alexandria:directories-changed'

export function announceDirectoriesChanged(count: number): void {
  window.dispatchEvent(new CustomEvent<number>(DIRECTORIES_CHANGED, { detail: count }))
}
