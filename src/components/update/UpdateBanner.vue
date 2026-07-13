<script setup lang="ts">
import { computed } from 'vue'
import { AppButton } from '@/components/ui'
import { useAppUpdate } from '@/composables/useAppUpdate'

const {
  phase,
  availableVersion,
  downloadProgress,
  bannerDismissed,
  downloadAndInstall,
  dismissBanner,
} = useAppUpdate()

// Visible only once a check has surfaced an update the user hasn't dismissed,
// and stays up through the download so the progress is visible.
const visible = computed(
  () =>
    !bannerDismissed.value &&
    (phase.value === 'available' || phase.value === 'downloading' || phase.value === 'ready'),
)
</script>

<template>
  <Transition name="update-banner">
    <div
      v-if="visible"
      class="fixed inset-x-0 bottom-0 z-50 border-t border-border bg-background/95 backdrop-blur px-4 py-3 safe-area-bottom"
      role="status"
    >
      <div class="mx-auto flex max-w-3xl items-center gap-4">
        <div class="min-w-0 flex-1">
          <p class="text-sm font-medium text-foreground">{{ $t('update.banner.title') }}</p>
          <p class="truncate text-xs text-muted-foreground">
            <template v-if="phase === 'downloading'">
              {{ $t('update.banner.downloading') }}
              <template v-if="downloadProgress !== null"> {{ Math.round(downloadProgress * 100) }}%</template>
            </template>
            <template v-else-if="phase === 'ready'">
              {{ $t('update.banner.installing') }}
            </template>
            <template v-else-if="availableVersion">
              {{ $t('update.banner.message', { version: availableVersion }) }}
            </template>
          </p>
        </div>
        <div class="flex shrink-0 items-center gap-2">
          <AppButton
            v-if="phase === 'available'"
            variant="ghost"
            size="sm"
            @click="dismissBanner()"
          >
            {{ $t('update.banner.later') }}
          </AppButton>
          <AppButton
            variant="primary"
            size="sm"
            :loading="phase === 'downloading' || phase === 'ready'"
            :disabled="phase !== 'available'"
            @click="downloadAndInstall()"
          >
            {{ $t('update.banner.install') }}
          </AppButton>
        </div>
      </div>
    </div>
  </Transition>
</template>

<style scoped>
.update-banner-enter-active,
.update-banner-leave-active {
  transition:
    transform 0.2s ease,
    opacity 0.2s ease;
}
.update-banner-enter-from,
.update-banner-leave-to {
  transform: translateY(100%);
  opacity: 0;
}
</style>
