<script setup lang="ts">
// Shown for any path no route matches. Retired routes are deleted rather than
// redirected, so a stale deep link from an earlier release lands here instead
// of being quietly rewritten to a page that may no longer mean the same thing.
import { useRoute, useRouter } from 'vue-router'
import { AppButton, EmptyState } from '@/components/ui'

const route = useRoute()
const router = useRouter()
</script>

<template>
  <div class="mx-auto max-w-2xl space-y-6 py-12">
    <EmptyState
      :title="$t('common.notFound.title')"
      :description="$t('common.notFound.body')"
    />
    <p class="text-center font-mono text-xs text-muted-foreground break-all">
      {{ route.fullPath }}
    </p>
    <div class="flex justify-center">
      <AppButton @click="router.push({ name: 'home' })">
        {{ $t('common.notFound.goHome') }}
      </AppButton>
    </div>
  </div>
</template>
