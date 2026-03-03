<script setup lang="ts">
import AppSidebar from '@/components/layout/AppSidebar.vue'
import AppTopBar from '@/components/layout/AppTopBar.vue'
import MobileTabBar from '@/components/layout/MobileTabBar.vue'
import { ref, onMounted } from 'vue'

// Persist sidebar state to localStorage (Mark 2 uses a cookie)
const STORAGE_KEY = 'alexandria-sidebar'
const sidebarCollapsed = ref(false)

onMounted(() => {
  const stored = localStorage.getItem(STORAGE_KEY)
  if (stored === 'collapsed') sidebarCollapsed.value = true
})

function toggleSidebar() {
  sidebarCollapsed.value = !sidebarCollapsed.value
  localStorage.setItem(STORAGE_KEY, sidebarCollapsed.value ? 'collapsed' : 'expanded')
}
</script>

<template>
  <div class="flex h-full overflow-hidden bg-background safe-area-top safe-area-lr">
    <!-- Sidebar — hidden on mobile, overflow-visible for edge toggle -->
    <div class="hidden md:flex relative h-full" style="overflow: visible;">
      <AppSidebar
        :collapsed="sidebarCollapsed"
        @toggle="toggleSidebar"
      />
    </div>

    <!-- Main column: topbar + content -->
    <div class="flex flex-1 flex-col overflow-hidden">
      <AppTopBar :sidebar-collapsed="sidebarCollapsed" @toggle-sidebar="toggleSidebar" />

      <main class="flex-1 overflow-y-auto mobile-content-padding">
        <div class="px-4 pt-6 pb-8 sm:px-6 lg:px-8">
          <slot />
        </div>
      </main>
    </div>

    <!-- Bottom tab bar — visible only on mobile -->
    <MobileTabBar />
  </div>
</template>
