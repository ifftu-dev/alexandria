<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { getVersion } from '@tauri-apps/api/app'
import { AppButton } from '@/components/ui'
import { useAppUpdate } from '@/composables/useAppUpdate'

const {
  phase,
  availableVersion,
  downloadProgress,
  errorMessage,
  supported,
  checkForUpdate,
  downloadAndInstall,
} = useAppUpdate()

const appVersion = ref<string>('')

onMounted(async () => {
  try {
    appVersion.value = await getVersion()
  } catch {
    appVersion.value = ''
  }
})
</script>

<template>
  <div v-if="supported()">
    <h4 class="settings-group-title">{{ $t('update.settings.title') }}</h4>
    <div class="rounded-lg border border-border p-4 space-y-3">
      <p class="text-sm text-muted-foreground">{{ $t('update.settings.description') }}</p>

      <div class="flex items-center gap-3">
        <AppButton
          variant="outline"
          size="sm"
          :loading="phase === 'checking'"
          :disabled="phase === 'downloading'"
          @click="checkForUpdate()"
        >
          {{ $t('update.settings.check') }}
        </AppButton>

        <AppButton
          v-if="phase === 'available'"
          variant="primary"
          size="sm"
          :loading="false"
          @click="downloadAndInstall()"
        >
          {{ $t('update.banner.install') }}
        </AppButton>

        <span
          v-if="phase === 'downloading'"
          class="text-xs text-muted-foreground"
        >
          {{ $t('update.banner.downloading') }}
          <template v-if="downloadProgress !== null">
            {{ Math.round(downloadProgress * 100) }}%
          </template>
        </span>
      </div>

      <p
        v-if="phase === 'available' && availableVersion"
        class="text-xs text-primary"
      >
        {{ $t('update.settings.available', { version: availableVersion }) }}
      </p>
      <p
        v-else-if="phase === 'uptodate'"
        class="text-xs text-muted-foreground"
      >
        {{ $t('update.settings.uptodate') }}
      </p>
      <p
        v-else-if="phase === 'error'"
        class="text-xs text-destructive"
      >
        {{ errorMessage || $t('update.settings.error') }}
      </p>

      <p v-if="appVersion" class="text-xs text-muted-foreground">
        {{ $t('update.settings.current', { version: appVersion }) }}
      </p>
    </div>
  </div>
</template>
