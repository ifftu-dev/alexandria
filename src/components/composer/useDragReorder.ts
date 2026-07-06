// Minimal HTML5 drag-to-reorder helper for the composer outline.
// No dependencies: the host component spreads the returned handlers on
// each draggable row and calls `commit` with the reordered ids.

import { ref } from 'vue'

export function useDragReorder(commit: (orderedIds: string[]) => void | Promise<void>) {
  const draggingId = ref<string | null>(null)
  const dragOverId = ref<string | null>(null)

  function onDragStart(id: string, e: DragEvent) {
    draggingId.value = id
    if (e.dataTransfer) {
      e.dataTransfer.effectAllowed = 'move'
      // Some WebKit builds require data for the drag to start.
      e.dataTransfer.setData('text/plain', id)
    }
  }

  function onDragOver(id: string, e: DragEvent) {
    e.preventDefault()
    if (e.dataTransfer) e.dataTransfer.dropEffect = 'move'
    dragOverId.value = id
  }

  function onDrop(targetId: string, currentIds: string[]) {
    const from = draggingId.value
    draggingId.value = null
    dragOverId.value = null
    if (!from || from === targetId) return

    const ids = [...currentIds]
    const fromIdx = ids.indexOf(from)
    const toIdx = ids.indexOf(targetId)
    if (fromIdx === -1 || toIdx === -1) return
    ids.splice(toIdx, 0, ...ids.splice(fromIdx, 1))
    void commit(ids)
  }

  function onDragEnd() {
    draggingId.value = null
    dragOverId.value = null
  }

  return { draggingId, dragOverId, onDragStart, onDragOver, onDrop, onDragEnd }
}
