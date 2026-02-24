<script setup lang="ts">
import AppSidebar from '@/components/layout/AppSidebar.vue'
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

    <!-- Main content -->
    <main class="flex-1 overflow-y-auto p-6">
      <slot />
    </main>
  </div>
</template>
