<script setup lang="ts">
import { ref, watch, nextTick, onBeforeUnmount } from 'vue'
import { useRouter } from 'vue-router'
import { useSkillGraphHover } from '@/composables/useSkillGraphHover'
import type { SkillStatus } from '@/composables/useSkillGraphState'

interface ModalSkillNode {
  id: string
  name: string
  routeId: string
  status: SkillStatus
  prerequisites: string[]
}

const props = defineProps<{
  visible: boolean
  nodes: ModalSkillNode[]
  earnedCount: number
  availableCount: number
  lockedCount: number
  totalCount: number
}>()

const emit = defineEmits<{ close: [] }>()

const router = useRouter()
const containerRef = ref<HTMLElement | null>(null)
// eslint-disable-next-line @typescript-eslint/no-explicit-any
const graphInstance = ref<any>(null)
let resizeObserver: ResizeObserver | null = null

const { buildAdjacency, createHoverHandler, renderNode, renderLink } = useSkillGraphHover()

function onKeydown(e: KeyboardEvent) {
  if (e.key === 'Escape') emit('close')
}

watch(() => props.visible, async (val) => {
  if (val) {
    document.body.style.overflow = 'hidden'
    document.addEventListener('keydown', onKeydown)
    await nextTick()
    initGraph()
  } else {
    document.body.style.overflow = ''
    document.removeEventListener('keydown', onKeydown)
    destroyGraph()
  }
})

watch(() => props.nodes, async () => {
  if (!props.visible) return
  await nextTick()
  initGraph()
}, { deep: true })

function destroyGraph() {
  resizeObserver?.disconnect()
  resizeObserver = null
  if (graphInstance.value) {
    graphInstance.value._destructor?.()
    graphInstance.value = null
  }
}

async function initGraph() {
  if (!containerRef.value || !props.nodes.length) return

  destroyGraph()

  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const ForceGraph = (await import('force-graph')).default as any

  const links: Array<{ source: string; target: string }> = []
  for (const node of props.nodes) {
    for (const prereqId of node.prerequisites ?? []) {
      links.push({ source: prereqId, target: node.id })
    }
  }
  buildAdjacency(links)

  const width = containerRef.value.clientWidth
  const height = containerRef.value.clientHeight

  const graph = ForceGraph()(containerRef.value)
    .width(width)
    .height(height)
    .graphData({ nodes: props.nodes, links })
    .autoPauseRedraw(false)
    .nodeLabel(() => '')
    .onNodeHover(createHoverHandler())
    .nodeCanvasObject((node: Record<string, unknown>, ctx: CanvasRenderingContext2D, globalScale: number) => {
      renderNode(node, ctx, globalScale)
    })
    .linkCanvasObject((link: Record<string, unknown>, ctx: CanvasRenderingContext2D) => {
      renderLink(link, ctx)
    })
    .linkCanvasObjectMode(() => 'replace' as const)
    .backgroundColor('transparent')
    .onNodeClick((node: Record<string, unknown>) => {
      const routeId = String(node.routeId ?? node.id ?? '')
      if (routeId) {
        router.push(`/skills/${routeId}`)
        emit('close')
      }
    })
    .cooldownTicks(100)
    .onEngineStop(() => {
      graph.zoomToFit(400, 40)
    })

  graph.d3Force('charge')?.strength(-80)
  graph.d3Force('link')?.distance(50)

  graphInstance.value = graph

  resizeObserver = new ResizeObserver((entries) => {
    for (const entry of entries) {
      graph.width(entry.contentRect.width)
      graph.height(entry.contentRect.height)
    }
  })
  resizeObserver.observe(containerRef.value)
}

onBeforeUnmount(() => {
  document.body.style.overflow = ''
  document.removeEventListener('keydown', onKeydown)
  destroyGraph()
})
</script>

<template>
  <Teleport to="body">
    <Transition name="modal">
      <div v-if="visible" class="fixed inset-0 z-[60] flex items-center justify-center">
        <div class="absolute inset-0 bg-black/60 backdrop-blur-sm" @click="emit('close')" />

        <div class="relative z-10 mx-4 flex h-[85vh] w-full max-w-7xl flex-col overflow-hidden rounded-2xl border border-border bg-card shadow-2xl">
          <div class="flex items-center justify-between border-b border-border px-6 py-4">
            <div class="flex items-center gap-3">
              <h2 class="text-lg font-semibold text-foreground">Skill Graph</h2>
              <span class="text-sm text-muted-foreground">{{ earnedCount }} / {{ totalCount }} skills earned</span>
            </div>
            <button
              title="Close"
              class="rounded-lg p-2 text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
              @click="emit('close')"
            >
              <svg class="h-5 w-5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                <path stroke-linecap="round" stroke-linejoin="round" d="M6 18L18 6M6 6l12 12" />
              </svg>
            </button>
          </div>

          <div class="relative flex-1 bg-slate-950">
            <div ref="containerRef" class="h-full w-full" />

            <div class="absolute bottom-4 left-4 flex items-center gap-4 rounded-lg bg-black/80 px-3 py-2 text-xs backdrop-blur-sm">
              <span class="flex items-center gap-1.5">
                <span class="inline-block h-2.5 w-2.5 rounded-full bg-success" />
                <span class="text-white">Earned ({{ earnedCount }})</span>
              </span>
              <span v-if="availableCount > 0" class="flex items-center gap-1.5">
                <span class="inline-block h-2.5 w-2.5 rounded-full bg-warning" />
                <span class="text-white">Available ({{ availableCount }})</span>
              </span>
              <span class="flex items-center gap-1.5">
                <span class="inline-block h-2 w-2 rounded-full bg-slate-500/40" />
                <span class="text-slate-300/80">Locked ({{ lockedCount }})</span>
              </span>
            </div>

            <div class="pointer-events-none absolute bottom-0 left-0 right-0 h-16 bg-gradient-to-t from-slate-950 to-transparent" />
          </div>
        </div>
      </div>
    </Transition>
  </Teleport>
</template>

<style scoped>
.modal-enter-active,
.modal-leave-active {
  transition: opacity 0.25s ease, transform 0.25s ease;
}

.modal-enter-active > .relative {
  transition: transform 0.25s ease;
}

.modal-enter-from,
.modal-leave-to {
  opacity: 0;
}

.modal-enter-from > .relative {
  transform: scale(0.95);
}
</style>
