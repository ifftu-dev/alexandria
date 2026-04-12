<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import { useOmniSearch } from '@/composables/useOmniSearch'
import { usePlatform } from '@/composables/usePlatform'

const router = useRouter()
const { isMac } = usePlatform()
const {
  isOpen,
  query,
  loading,
  selectedIndex,
  visibleItems,
  groupedItems,
  open,
  close,
  setQuery,
  navigate,
  select,
  selectAt,
} = useOmniSearch()

const inputRef = ref<HTMLInputElement | null>(null)
const listRef = ref<HTMLElement | null>(null)

const showEmptyHint = computed(
  () => isOpen.value && query.value.trim().length === 0 && visibleItems.value.length === 0,
)

const showNoResults = computed(
  () =>
    isOpen.value &&
    query.value.trim().length > 0 &&
    !loading.value &&
    visibleItems.value.length === 0,
)

function onInput(e: Event) {
  const target = e.target as HTMLInputElement
  setQuery(target.value)
}

function onKeydown(e: KeyboardEvent) {
  if (!isOpen.value) return
  if (e.key === 'ArrowDown') {
    e.preventDefault()
    navigate(1)
    void scrollSelectedIntoView()
  } else if (e.key === 'ArrowUp') {
    e.preventDefault()
    navigate(-1)
    void scrollSelectedIntoView()
  } else if (e.key === 'Enter') {
    e.preventDefault()
    const route = select()
    if (route) void router.push(route)
  } else if (e.key === 'Escape') {
    e.preventDefault()
    close()
  }
}

function onClickItem(index: number) {
  const route = selectAt(index)
  if (route) void router.push(route)
}

function onGlobalKeydown(e: KeyboardEvent) {
  const mod = isMac ? e.metaKey : e.ctrlKey
  if (mod && (e.key === 'k' || e.key === 'K') && !e.altKey && !e.shiftKey) {
    e.preventDefault()
    if (isOpen.value) close()
    else open()
  }
}

/** Flat index for visibleItems so each list row can know its position. */
function flatIndexFor(groupType: string, itemId: string): number {
  return visibleItems.value.findIndex(v => v.type === groupType && v.id === itemId)
}

async function scrollSelectedIntoView() {
  await nextTick()
  const container = listRef.value
  if (!container) return
  const el = container.querySelector<HTMLElement>('[data-omni-selected="true"]')
  if (el) el.scrollIntoView({ block: 'nearest' })
}

watch(isOpen, async (val) => {
  if (val) {
    await nextTick()
    inputRef.value?.focus()
    inputRef.value?.select()
  }
})

onMounted(() => document.addEventListener('keydown', onGlobalKeydown))
onUnmounted(() => document.removeEventListener('keydown', onGlobalKeydown))
</script>

<template>
  <Teleport to="body">
    <Transition
      enter-active-class="transition duration-150 ease-out"
      enter-from-class="opacity-0"
      enter-to-class="opacity-100"
      leave-active-class="transition duration-100 ease-in"
      leave-from-class="opacity-100"
      leave-to-class="opacity-0"
    >
      <div
        v-if="isOpen"
        class="fixed inset-0 z-50 flex items-start justify-center bg-black/40 backdrop-blur-sm px-4 pt-[15vh]"
        @click.self="close"
      >
        <Transition
          enter-active-class="transition duration-150 ease-out"
          enter-from-class="opacity-0 translate-y-[-8px]"
          enter-to-class="opacity-100 translate-y-0"
          leave-active-class="transition duration-100 ease-in"
          leave-from-class="opacity-100 translate-y-0"
          leave-to-class="opacity-0 translate-y-[-8px]"
        >
          <div
            v-if="isOpen"
            class="card overflow-hidden w-full max-w-[40rem] shadow-lg"
            role="dialog"
            aria-modal="true"
            aria-label="Omni search"
          >
            <!-- Search input -->
            <div class="flex items-center gap-3 px-4 h-12 border-b border-border">
              <svg class="w-4 h-4 text-muted-foreground flex-shrink-0" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                <path stroke-linecap="round" stroke-linejoin="round" d="M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z" />
              </svg>
              <input
                ref="inputRef"
                :value="query"
                type="text"
                placeholder="Search skills, courses, DAOs, classrooms..."
                class="flex-1 bg-transparent outline-none text-sm text-foreground placeholder:text-muted-foreground"
                @input="onInput"
                @keydown="onKeydown"
              />
              <span v-if="loading" class="text-xs text-muted-foreground">Searching…</span>
              <kbd class="hidden sm:inline-flex items-center gap-0.5 px-1.5 h-5 text-[10px] font-mono text-muted-foreground bg-muted/50 border border-border rounded">
                esc
              </kbd>
            </div>

            <!-- Empty hint (no query, no recents) -->
            <div v-if="showEmptyHint" class="px-4 py-8 text-center">
              <p class="text-sm text-muted-foreground">
                Start typing to search across skills, courses, DAOs, and classrooms.
              </p>
              <p class="mt-2 text-xs text-muted-foreground/70">
                Try: <span class="font-mono">graphs</span>, <span class="font-mono">cybersecurity</span>, <span class="font-mono">design</span>
              </p>
            </div>

            <!-- No results -->
            <div v-else-if="showNoResults" class="px-4 py-8 text-center">
              <p class="text-sm text-muted-foreground">
                No results for "<span class="font-medium text-foreground">{{ query }}</span>"
              </p>
            </div>

            <!-- Results list -->
            <div
              v-else-if="visibleItems.length > 0"
              ref="listRef"
              class="max-h-[60vh] overflow-y-auto py-2"
            >
              <!-- Recents header when showing empty-query recents -->
              <p
                v-if="query.length === 0"
                class="px-4 pt-1 pb-2 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground"
              >
                Recent
              </p>

              <!-- Grouped results (when query present) -->
              <template v-if="query.length > 0">
                <div v-for="group in groupedItems" :key="group.type" class="mb-1">
                  <p class="px-4 pt-2 pb-1 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
                    {{ group.label }}
                  </p>
                  <button
                    v-for="item in group.items"
                    :key="item.id"
                    type="button"
                    :data-omni-selected="flatIndexFor(item.type, item.id) === selectedIndex"
                    class="w-full flex items-center gap-3 px-4 py-2 text-left transition-colors"
                    :class="
                      flatIndexFor(item.type, item.id) === selectedIndex
                        ? 'bg-primary/10 text-foreground'
                        : 'hover:bg-muted/40 text-foreground'
                    "
                    @click="onClickItem(flatIndexFor(item.type, item.id))"
                    @mousemove="selectedIndex = flatIndexFor(item.type, item.id)"
                  >
                    <span v-if="item.icon" class="text-base flex-shrink-0">{{ item.icon }}</span>
                    <span
                      v-else
                      class="w-4 h-4 flex-shrink-0 text-muted-foreground"
                    >
                      <!-- Domain icon -->
                      <svg v-if="item.type === 'skill'" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                        <path stroke-linecap="round" stroke-linejoin="round" d="M9.663 17h4.673M12 3v1m6.364 1.636l-.707.707M21 12h-1M4 12H3m3.343-5.657l-.707-.707m2.828 9.9a5 5 0 117.072 0l-.548.547A3.374 3.374 0 0014 18.469V19a2 2 0 11-4 0v-.531c0-.895-.356-1.754-.988-2.386l-.548-.547z" />
                      </svg>
                      <svg v-else-if="item.type === 'course' || item.type === 'catalog'" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                        <path stroke-linecap="round" stroke-linejoin="round" d="M12 6.253v13m0-13C10.832 5.477 9.246 5 7.5 5S4.168 5.477 3 6.253v13C4.168 18.477 5.754 18 7.5 18s3.332.477 4.5 1.253m0-13C13.168 5.477 14.754 5 16.5 5c1.747 0 3.332.477 4.5 1.253v13C19.832 18.477 18.247 18 16.5 18c-1.746 0-3.332.477-4.5 1.253" />
                      </svg>
                      <svg v-else-if="item.type === 'classroom'" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                        <path stroke-linecap="round" stroke-linejoin="round" d="M17 20h5v-2a4 4 0 00-3-3.87M9 20H4v-2a3 3 0 015.356-1.857M17 10.5a4 4 0 10-8 0 4 4 0 008 0z" />
                      </svg>
                    </span>
                    <span class="flex-1 min-w-0">
                      <span class="block text-sm truncate">{{ item.title }}</span>
                      <span
                        v-if="item.subtitle"
                        class="block text-xs text-muted-foreground truncate"
                      >
                        {{ item.subtitle }}
                      </span>
                    </span>
                  </button>
                </div>
              </template>

              <!-- Ungrouped recents when query is empty -->
              <template v-else>
                <button
                  v-for="(item, index) in visibleItems"
                  :key="item.id"
                  type="button"
                  :data-omni-selected="index === selectedIndex"
                  class="w-full flex items-center gap-3 px-4 py-2 text-left transition-colors"
                  :class="
                    index === selectedIndex
                      ? 'bg-primary/10 text-foreground'
                      : 'hover:bg-muted/40 text-foreground'
                  "
                  @click="onClickItem(index)"
                  @mousemove="selectedIndex = index"
                >
                  <span v-if="item.icon" class="text-base flex-shrink-0">{{ item.icon }}</span>
                  <span v-else class="w-4 h-4 text-muted-foreground flex-shrink-0">
                    <svg fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                      <path stroke-linecap="round" stroke-linejoin="round" d="M12 8v4l3 3m6-3a9 9 0 11-18 0 9 9 0 0118 0z" />
                    </svg>
                  </span>
                  <span class="flex-1 min-w-0">
                    <span class="block text-sm truncate">{{ item.title }}</span>
                    <span
                      v-if="item.subtitle"
                      class="block text-xs text-muted-foreground truncate"
                    >
                      {{ item.subtitle }}
                    </span>
                  </span>
                </button>
              </template>
            </div>

            <!-- Footer: keyboard hints -->
            <div class="flex items-center justify-between px-4 h-9 border-t border-border text-[11px] text-muted-foreground">
              <div class="flex items-center gap-3">
                <span class="flex items-center gap-1">
                  <kbd class="inline-flex items-center px-1 h-4 font-mono text-[10px] bg-muted/50 border border-border rounded">↑</kbd>
                  <kbd class="inline-flex items-center px-1 h-4 font-mono text-[10px] bg-muted/50 border border-border rounded">↓</kbd>
                  navigate
                </span>
                <span class="flex items-center gap-1">
                  <kbd class="inline-flex items-center px-1 h-4 font-mono text-[10px] bg-muted/50 border border-border rounded">↵</kbd>
                  select
                </span>
              </div>
              <span>
                {{ visibleItems.length }} {{ visibleItems.length === 1 ? 'result' : 'results' }}
              </span>
            </div>
          </div>
        </Transition>
      </div>
    </Transition>
  </Teleport>
</template>
