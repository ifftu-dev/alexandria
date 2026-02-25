<script setup lang="ts">
import AppSidebar from '@/components/layout/AppSidebar.vue'
import AppTopBar from '@/components/layout/AppTopBar.vue'
import { ref } from 'vue'

const sidebarCollapsed = ref(false)
</script>

<template>
  <div class="flex h-screen overflow-hidden bg-[rgb(var(--color-background))]">
    <!-- Sidebar -->
    <AppSidebar
      :collapsed="sidebarCollapsed"
      @toggle="sidebarCollapsed = !sidebarCollapsed"
    />

    <!-- Main column: topbar + content -->
    <div class="flex flex-1 flex-col overflow-hidden">
      <AppTopBar :sidebar-collapsed="sidebarCollapsed" />

      <main class="flex-1 overflow-y-auto">
        <div class="px-4 pt-6 pb-8 sm:px-6 lg:px-8">
          <slot />
        </div>
      </main>
    </div>
  </div>
</template>
