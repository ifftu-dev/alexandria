<script setup lang="ts">
import { useSlowNotice } from '@/composables/useSlowNotice'

const slow = useSlowNotice(() => true)
</script>

<template>
  <div class="flex items-center justify-center h-full bg-background safe-area-top" role="status">
    <div class="text-center">
      <div class="w-24 h-24 mx-auto mb-4" aria-hidden="true" />
      <p class="text-sm text-muted-foreground">{{ $t('common.app.initializing') }}</p>
      <p v-if="slow" class="mt-1 text-xs text-muted-foreground">{{ $t('common.app.takingLonger') }}</p>
    </div>
  </div>
</template>
