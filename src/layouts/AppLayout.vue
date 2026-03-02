<script setup lang="ts">
import AppSidebar from '@/components/layout/AppSidebar.vue'
import AppTopBar from '@/components/layout/AppTopBar.vue'
import MobileTabBar from '@/components/layout/MobileTabBar.vue'
import { ref } from 'vue'

const sidebarCollapsed = ref(false)
</script>

<template>
  <div class="flex h-dvh overflow-hidden bg-background">
    <!-- Sidebar — hidden on mobile -->
    <AppSidebar
      class="hidden md:flex"
      :collapsed="sidebarCollapsed"
      @toggle="sidebarCollapsed = !sidebarCollapsed"
    />

    <!-- Main column: topbar + content -->
    <div class="flex flex-1 flex-col overflow-hidden">
      <AppTopBar :sidebar-collapsed="sidebarCollapsed" />

      <main class="flex-1 overflow-y-auto mobile-content-padding">
        <div class="px-4 pt-4 pb-8 sm:px-6 sm:pt-6 lg:px-8">
          <slot />
        </div>
      </main>
    </div>

    <!-- Bottom tab bar — visible only on mobile -->
    <MobileTabBar />
  </div>
</template>
