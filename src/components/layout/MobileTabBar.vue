<script setup lang="ts">
import { computed } from 'vue'
import { useRouter, useRoute } from 'vue-router'
import { useContentSync } from '@/composables/useContentSync'

const router = useRouter()
const route = useRoute()
const { visible: contentSyncVisible, statusMessage: contentSyncMessage } = useContentSync()

interface Tab {
  label: string
  path: string
  icon: string
}

const tabs: Tab[] = [
  {
    label: 'Home',
    path: '/home',
    icon: 'M3 12l2-2m0 0l7-7 7 7M5 10v10a1 1 0 001 1h3m10-11l2 2m-2-2v10a1 1 0 01-1 1h-3m-6 0a1 1 0 001-1v-4a1 1 0 011-1h2a1 1 0 011 1v4a1 1 0 001 1m-6 0h6',
  },
  {
    label: 'Live Tutoring',
    path: '/tutoring',
    icon: 'M15 10l4.553-2.276A1 1 0 0121 8.618v6.764a1 1 0 01-1.447.894L15 14M5 18h8a2 2 0 002-2V8a2 2 0 00-2-2H5a2 2 0 00-2 2v8a2 2 0 002 2z',
  },
  {
    label: 'Classrooms',
    path: '/courses',
    icon: 'M12 6.253v13m0-13C10.832 5.477 9.246 5 7.5 5S4.168 5.477 3 6.253v13C4.168 18.477 5.754 18 7.5 18s3.332.477 4.5 1.253m0-13C13.168 5.477 14.754 5 16.5 5c1.747 0 3.332.477 4.5 1.253v13C19.832 18.477 18.247 18 16.5 18c-1.746 0-3.332.477-4.5 1.253',
  },
  {
    label: 'Skill Graph',
    path: '/skills',
    icon: 'M9 12l2 2 4-4M7.835 4.697a3.42 3.42 0 0 0 1.946-.806 3.42 3.42 0 0 1 4.438 0 3.42 3.42 0 0 0 1.946.806 3.42 3.42 0 0 1 3.138 3.138 3.42 3.42 0 0 0 .806 1.946 3.42 3.42 0 0 1 0 4.438 3.42 3.42 0 0 0-.806 1.946 3.42 3.42 0 0 1-3.138 3.138 3.42 3.42 0 0 0-1.946.806 3.42 3.42 0 0 1-4.438 0 3.42 3.42 0 0 0-1.946-.806 3.42 3.42 0 0 1-3.138-3.138 3.42 3.42 0 0 0-.806-1.946 3.42 3.42 0 0 1 0-4.438 3.42 3.42 0 0 0 .806-1.946 3.42 3.42 0 0 1 3.138-3.138z',
  },
]

function isActive(path: string) {
  return route.path === path || route.path.startsWith(path + '/')
}

function navigate(path: string) {
  router.push(path)
}

const mobileStatusMessage = computed(() => {
  if (!contentSyncVisible.value || !contentSyncMessage.value) return ''
  return contentSyncMessage.value
})
</script>

<template>
  <div class="md:hidden fixed bottom-0 left-0 right-0 z-[70]">
    <div
      v-if="mobileStatusMessage"
      class="mobile-sync-status border-t border-border/60 bg-card/95 px-3 py-1 text-[0.65rem] text-muted-foreground backdrop-blur"
      :title="mobileStatusMessage"
    >
      {{ mobileStatusMessage }}
    </div>

    <nav class="flex items-stretch border-t border-border bg-card safe-area-bottom">
      <button
        v-for="tab in tabs"
        :key="tab.path"
        class="flex flex-1 flex-col items-center justify-center gap-0.5 pt-2 pb-1 transition-colors"
        :class="isActive(tab.path) ? 'text-primary' : 'text-muted-foreground'"
        @click="navigate(tab.path)"
      >
        <svg class="w-5 h-5" fill="none" viewBox="0 0 24 24" stroke="currentColor" :stroke-width="isActive(tab.path) ? 2.25 : 1.75">
          <path stroke-linecap="round" stroke-linejoin="round" :d="tab.icon" />
        </svg>
        <span class="text-[0.6rem] font-medium leading-tight">{{ tab.label }}</span>
      </button>
    </nav>
  </div>
</template>

<style scoped>
.mobile-sync-status {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
</style>
