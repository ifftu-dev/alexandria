<script setup lang="ts">
import AppSidebar from '@/components/layout/AppSidebar.vue'
import AppTopBar from '@/components/layout/AppTopBar.vue'
import AppBottomBar from '@/components/layout/AppBottomBar.vue'
import MobileTabBar from '@/components/layout/MobileTabBar.vue'
import TutoringPiP from '@/components/layout/TutoringPiP.vue'
import OmniSearch from '@/components/omni/OmniSearch.vue'
import CourseCompletionModal from '@/components/course/CourseCompletionModal.vue'
import { usePlatform } from '@/composables/usePlatform'
import { useSettings, useSetting } from '@/composables/useSettings'
import { computed, onMounted, onUnmounted } from 'vue'
import { useRoute } from 'vue-router'
import { getCurrentWindow } from '@tauri-apps/api/window'

// The learning player owns its own full-bleed layout (edge-to-edge
// video, internal sidebar). Skip the shell's content padding for it so
// the player can reach the screen edges.
const route = useRoute()
const isImmersiveRoute = computed(() => route.name === 'learn' || route.name === 'settings')

// Sidebar collapsed state lives in the per-profile settings store
// (`ui.sidebar_collapsed`, scope=sync) so it propagates to the
// user's other devices.
const collapsedSetting = useSetting<boolean>('ui.sidebar_collapsed')
const sidebarCollapsed = computed({
  get: () => collapsedSetting.ref.value ?? false,
  set: (v: boolean) => {
    void collapsedSetting.set(v)
  },
})
const { isMobilePlatform } = usePlatform()

onMounted(async () => {
  await useSettings().initialize()
  if (!isMobilePlatform) {
    document.addEventListener('mousedown', onAppMouseDown)
  }
})

onUnmounted(() => {
  document.removeEventListener('mousedown', onAppMouseDown)
})

/** Drag the window from any non-interactive, non-scrollbar area. */
async function onAppMouseDown(e: MouseEvent) {
  if (e.button !== 0) return

  const target = e.target as HTMLElement | null
  if (!target) return

  // Skip interactive elements.
  if (target.closest(
    'button, input, textarea, select, a, video, audio, iframe, ' +
    '[role="option"], [role="slider"], [role="dialog"], ' +
    '[contenteditable="true"], .plugin-iframe'
  )) return

  // Skip scrollbar clicks. If the target (or a close ancestor) is
  // scrollable and the click lands within 16px of its right or bottom
  // edge, the user is probably grabbing the scrollbar.
  const scrollable = target.closest('.overflow-y-auto, .overflow-x-auto') as HTMLElement | null
  if (scrollable) {
    const rect = scrollable.getBoundingClientRect()
    const nearRight = e.clientX > rect.right - 16
    const nearBottom = e.clientY > rect.bottom - 16
    if (nearRight || nearBottom) return
  }

  // Skip text content areas — the user may want to select text.
  if (target.closest('.lesson-body, .prose, pre, code, [data-selectable]')) return

  const inTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window
  if (!inTauri) return

  try {
    await getCurrentWindow().startDragging()
  } catch {
    // Non-critical.
  }
}

function toggleSidebar() {
  sidebarCollapsed.value = !sidebarCollapsed.value
}
</script>

<template>
  <div class="flex flex-col h-full overflow-hidden bg-background safe-area-top safe-area-lr">
    <!-- Topbar — spans full width above everything -->
    <AppTopBar :sidebar-collapsed="sidebarCollapsed" @toggle-sidebar="toggleSidebar" />

    <!-- Below topbar: sidebar + content side by side -->
    <div class="flex flex-1 overflow-hidden">
      <!-- Sidebar — hidden on mobile -->
      <div v-if="!isMobilePlatform" class="hidden md:flex relative h-full">
        <AppSidebar
          :collapsed="sidebarCollapsed"
          @toggle="toggleSidebar"
        />
      </div>

      <!-- Content area -->
      <main class="flex-1 overflow-y-auto mobile-content-padding">
        <div :class="isImmersiveRoute ? 'h-full flex flex-col' : 'px-4 pt-6 pb-8 sm:px-6 lg:px-8'">
          <slot />
        </div>
      </main>
    </div>

    <AppBottomBar />

    <TutoringPiP />

    <!-- Bottom tab bar — visible only on mobile -->
    <MobileTabBar />

    <!-- Global omni search palette (Cmd+K / Ctrl+K / "/") -->
    <OmniSearch />

    <!-- Global course-completion celebration (survives navigating away) -->
    <CourseCompletionModal />
  </div>
</template>
