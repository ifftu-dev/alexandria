<script setup lang="ts">
import { RouterView, useRoute } from 'vue-router'
import AppSidebar from '@/components/layout/AppSidebar.vue'
import MobileTabBar from '@/components/layout/MobileTabBar.vue'
import ModeSwitcher from '@/components/layout/ModeSwitcher.vue'
import OmniSearch from '@/components/omni/OmniSearch.vue'
import MotionSettings from '@/components/ui/MotionSettings.vue'
import { useOmniSearch } from '@/composables/useOmniSearch'

// The step 2 motion surfaces over one unlocked profile: navigation, search,
// Home, Goals, the catalog and the course player.
const route = useRoute()
const search = useOmniSearch()
</script>

<template>
  <div class="flex h-dvh">
    <AppSidebar :collapsed="false" />
    <main class="min-w-0 flex-1 overflow-y-auto p-6 pb-24">
      <header class="mb-6 flex flex-wrap items-end gap-4 border-b border-border pb-4">
        <MotionSettings />
        <ModeSwitcher />
        <button type="button" class="btn btn-secondary btn-sm" @click="search.open()">Open search</button>
        <output>{{ route.fullPath }}</output>
      </header>
      <RouterView />
    </main>
  </div>
  <MobileTabBar />
  <OmniSearch />
</template>
