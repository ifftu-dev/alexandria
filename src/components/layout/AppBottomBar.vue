<script setup lang="ts">
import { computed, onMounted } from 'vue'
import { useP2P } from '@/composables/useP2P'
import { useContentSync } from '@/composables/useContentSync'

const { status: p2pStatus, startPolling } = useP2P()
const { visible: contentSyncVisible, statusMessage: contentSyncMessage } = useContentSync()

onMounted(() => {
  startPolling(15000)
})

const networkStatusLabel = computed(() => {
  return p2pStatus.value?.is_running ? 'Connected' : 'Connecting...'
})

const peerCount = computed(() => {
  if (!p2pStatus.value?.is_running) return 0
  return p2pStatus.value.connected_peers
})

const networkIconClass = computed(() => {
  if (p2pStatus.value?.is_running) return 'text-success'
  return 'text-warning'
})

</script>

<template>
  <footer class="bottom-bar hidden md:flex">
    <div class="bottom-bar__left">
      <svg :class="['h-3.5 w-3.5', networkIconClass]" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
        <path stroke-linecap="round" stroke-linejoin="round" d="M21 12a9 9 0 01-9 9m9-9a9 9 0 00-9-9m9 9H3m9 9a9 9 0 01-9-9m9 9c1.657 0 3-4.03 3-9s-1.343-9-3-9m0 18c-1.657 0-3-4.03-3-9s1.343-9 3-9m-9 9a9 9 0 019-9" />
      </svg>
      <span class="font-medium">
        {{ networkStatusLabel }} | {{ peerCount }} peer{{ peerCount !== 1 ? 's' : '' }}
      </span>
    </div>

    <div v-if="contentSyncVisible && contentSyncMessage" class="bottom-bar__center" :title="contentSyncMessage">
      {{ contentSyncMessage }}
    </div>

    <div class="bottom-bar__right">
      Built with <span class="bottom-bar__heart">&#10084;</span> by
      <a href="https://www.ifftu.dev" target="_blank" rel="noopener noreferrer" class="bottom-bar__link">IFFTU</a>
    </div>
  </footer>
</template>

<style scoped>
.bottom-bar {
  height: 1.75rem;
  align-items: center;
  justify-content: flex-start;
  gap: 1rem;
  padding: 0 0.5rem;
  border-top: 1px solid var(--app-border);
  background: color-mix(in srgb, var(--app-muted) 78%, var(--app-background));
  color: var(--app-muted-foreground);
  font-size: 0.75rem;
}

.bottom-bar__left {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  min-width: 0;
  color: var(--app-foreground);
}

.bottom-bar__center {
  min-width: 0;
  max-width: 50%;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  color: var(--app-muted-foreground);
}

.bottom-bar__right {
  margin-left: auto;
  flex-shrink: 0;
  white-space: nowrap;
  color: var(--app-muted-foreground);
}

.bottom-bar__heart {
  color: #e11d48;
  font-size: 0.8em;
  vertical-align: baseline;
}

.bottom-bar__link {
  color: var(--app-primary);
  text-decoration: none;
  font-weight: 500;
  transition: opacity 0.15s;
  -webkit-app-region: no-drag;
  app-region: no-drag;
}
.bottom-bar__link:hover {
  opacity: 0.8;
  text-decoration: underline;
}
</style>
