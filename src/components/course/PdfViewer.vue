<script setup lang="ts">
import { ref, onMounted, watch, computed } from 'vue'
import { useLocalApi } from '@/composables/useLocalApi'


const props = defineProps<{
  contentCid: string | null
  pageCount?: number
}>()

const emit = defineEmits<{
  (e: 'complete'): void
  (e: 'progress', percent: number): void
}>()

const { invoke } = useLocalApi()
const pdfUrl = ref<string | null>(null)
const loading = ref(false)
const error = ref<string | null>(null)
const currentPage = ref(1)
const totalPages = ref(props.pageCount || 1)
const zoom = ref(100)

async function loadPdf() {
  if (!props.contentCid) { pdfUrl.value = null; return }
  loading.value = true
  error.value = null
  try {
    const bytes = await invoke<number[]>('content_resolve_bytes', { identifier: props.contentCid })
    const blob = new Blob([new Uint8Array(bytes)], { type: 'application/pdf' })
    if (pdfUrl.value) URL.revokeObjectURL(pdfUrl.value)
    pdfUrl.value = URL.createObjectURL(blob)
    if (props.pageCount) totalPages.value = props.pageCount
  } catch (e: unknown) {
    error.value = `Failed to load PDF: ${e}`
    pdfUrl.value = null
  } finally {
    loading.value = false
  }
}

function prevPage() {
  if (currentPage.value > 1) {
    currentPage.value--
    emitProgress()
  }
}

function nextPage() {
  if (currentPage.value < totalPages.value) {
    currentPage.value++
    emitProgress()
    if (currentPage.value === totalPages.value) {
      emit('complete')
    }
  }
}

function emitProgress() {
  const pct = totalPages.value > 0 ? Math.round((currentPage.value / totalPages.value) * 100) : 0
  emit('progress', pct)
}

function zoomIn() { zoom.value = Math.min(zoom.value + 25, 200) }
function zoomOut() { zoom.value = Math.max(zoom.value - 25, 50) }
function resetZoom() { zoom.value = 100 }

const progressPercent = computed(() => {
  return totalPages.value > 0 ? Math.round((currentPage.value / totalPages.value) * 100) : 0
})

function handleKeydown(e: KeyboardEvent) {
  if (e.key === 'ArrowRight' || e.key === 'PageDown') { nextPage(); e.preventDefault() }
  else if (e.key === 'ArrowLeft' || e.key === 'PageUp') { prevPage(); e.preventDefault() }
  else if (e.key === '+' || e.key === '=') { zoomIn(); e.preventDefault() }
  else if (e.key === '-') { zoomOut(); e.preventDefault() }
  else if (e.key === '0') { resetZoom(); e.preventDefault() }
}

onMounted(() => {
  loadPdf()
  window.addEventListener('keydown', handleKeydown)
})

watch(() => props.contentCid, loadPdf)
</script>

<template>
  <div class="pdf-viewer">
    <!-- Loading -->
    <div v-if="loading" class="flex items-center justify-center py-16">
      <div class="h-8 w-8 animate-spin rounded-full border-2 border-[rgb(var(--color-primary))] border-t-transparent" />
    </div>

    <!-- Error -->
    <div v-else-if="error" class="rounded-lg border border-red-500/20 bg-red-500/10 p-4">
      <p class="text-sm text-red-600 dark:text-red-400">{{ error }}</p>
      <a
        v-if="pdfUrl"
        :href="pdfUrl"
        download
        class="mt-2 inline-flex items-center text-sm text-[rgb(var(--color-primary))] hover:underline"
      >
        <svg class="mr-1.5 h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
          <path stroke-linecap="round" stroke-linejoin="round" d="M4 16v1a3 3 0 003 3h10a3 3 0 003-3v-1m-4-4l-4 4m0 0l-4-4m4 4V4" />
        </svg>
        Download PDF instead
      </a>
    </div>

    <!-- PDF Content -->
    <div v-else-if="pdfUrl" class="space-y-3">
      <!-- Toolbar -->
      <div class="flex items-center justify-between rounded-lg border border-[rgb(var(--color-border))] bg-[rgb(var(--color-card))] px-4 py-2">
        <!-- Page navigation -->
        <div class="flex items-center gap-2">
          <button
            class="rounded p-1 text-[rgb(var(--color-muted-foreground))] transition-colors hover:bg-[rgb(var(--color-muted))] hover:text-[rgb(var(--color-foreground))] disabled:opacity-30"
            :disabled="currentPage <= 1"
            @click="prevPage"
          >
            <svg class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
              <path stroke-linecap="round" stroke-linejoin="round" d="M15 19l-7-7 7-7" />
            </svg>
          </button>
          <span class="text-sm text-[rgb(var(--color-foreground))]">
            <input
              v-model.number="currentPage"
              type="number"
              :min="1"
              :max="totalPages"
              class="w-10 rounded border border-[rgb(var(--color-border))] bg-transparent px-1 py-0.5 text-center text-xs"
            />
            <span class="text-[rgb(var(--color-muted-foreground))]"> / {{ totalPages }}</span>
          </span>
          <button
            class="rounded p-1 text-[rgb(var(--color-muted-foreground))] transition-colors hover:bg-[rgb(var(--color-muted))] hover:text-[rgb(var(--color-foreground))] disabled:opacity-30"
            :disabled="currentPage >= totalPages"
            @click="nextPage"
          >
            <svg class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
              <path stroke-linecap="round" stroke-linejoin="round" d="M9 5l7 7-7 7" />
            </svg>
          </button>
        </div>

        <!-- Zoom controls -->
        <div class="flex items-center gap-2">
          <button
            class="rounded p-1 text-[rgb(var(--color-muted-foreground))] transition-colors hover:bg-[rgb(var(--color-muted))] hover:text-[rgb(var(--color-foreground))]"
            @click="zoomOut"
          >
            <svg class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
              <path stroke-linecap="round" stroke-linejoin="round" d="M20 12H4" />
            </svg>
          </button>
          <button
            class="text-xs text-[rgb(var(--color-muted-foreground))] hover:text-[rgb(var(--color-foreground))]"
            @click="resetZoom"
          >
            {{ zoom }}%
          </button>
          <button
            class="rounded p-1 text-[rgb(var(--color-muted-foreground))] transition-colors hover:bg-[rgb(var(--color-muted))] hover:text-[rgb(var(--color-foreground))]"
            @click="zoomIn"
          >
            <svg class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
              <path stroke-linecap="round" stroke-linejoin="round" d="M12 4v16m8-8H4" />
            </svg>
          </button>
          <span class="mx-2 h-4 w-px bg-[rgb(var(--color-border))]" />
          <a
            :href="pdfUrl"
            download
            class="rounded p-1 text-[rgb(var(--color-muted-foreground))] transition-colors hover:bg-[rgb(var(--color-muted))] hover:text-[rgb(var(--color-foreground))]"
            title="Download PDF"
          >
            <svg class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
              <path stroke-linecap="round" stroke-linejoin="round" d="M4 16v1a3 3 0 003 3h10a3 3 0 003-3v-1m-4-4l-4 4m0 0l-4-4m4 4V4" />
            </svg>
          </a>
        </div>
      </div>

      <!-- PDF iframe -->
      <div class="relative overflow-hidden rounded-lg border border-[rgb(var(--color-border))] bg-[rgb(var(--color-muted))]">
        <iframe
          :src="`${pdfUrl}#page=${currentPage}`"
          class="h-[600px] w-full border-0"
          :style="{ transform: `scale(${zoom / 100})`, transformOrigin: 'top left', width: `${10000 / zoom}%`, height: `${600 * 100 / zoom}px` }"
        />
      </div>

      <!-- Progress bar -->
      <div class="space-y-1">
        <div class="flex items-center justify-between text-xs text-[rgb(var(--color-muted-foreground))]">
          <span>Reading progress</span>
          <span>{{ progressPercent }}%</span>
        </div>
        <div class="h-1 overflow-hidden rounded-full bg-[rgb(var(--color-muted)/0.3)]">
          <div
            class="h-full rounded-full bg-[rgb(var(--color-primary))] transition-all duration-300"
            :style="{ width: `${progressPercent}%` }"
          />
        </div>
      </div>
    </div>

    <!-- No content -->
    <div v-else class="py-12 text-center text-sm text-[rgb(var(--color-muted-foreground))]">
      No PDF content available.
    </div>
  </div>
</template>
