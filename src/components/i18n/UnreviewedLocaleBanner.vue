<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'

import { useLocale } from '@/composables/useLocale'
import { useSetting } from '@/composables/useSettings'

const { t } = useI18n()
const { locale, isReviewed } = useLocale()

// Device-scoped list of locale codes whose banner the user has dismissed.
const dismissed = useSetting<string[]>('ui.dismissed_locale_notices')

const show = computed(() => {
  if (isReviewed.value) return false
  const list = dismissed.ref.value ?? []
  return !list.includes(locale.value)
})

function dismiss() {
  const list = dismissed.ref.value ?? []
  if (list.includes(locale.value)) return
  void dismissed.set([...list, locale.value])
}
</script>

<template>
  <div v-if="show" class="unreviewed-banner" role="status">
    <svg
      class="unreviewed-banner__icon"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      stroke-width="1.75"
      aria-hidden="true"
    >
      <path stroke-linecap="round" stroke-linejoin="round" d="M12 9v4m0 4h.01M10.29 3.86 1.82 18a2 2 0 0 0 1.71 3h16.94a2 2 0 0 0 1.71-3L13.71 3.86a2 2 0 0 0-3.42 0Z" />
    </svg>
    <p class="unreviewed-banner__text">{{ t('common.unreviewedBanner.message') }}</p>
    <button class="unreviewed-banner__dismiss" @click="dismiss">
      {{ t('common.unreviewedBanner.dismiss') }}
    </button>
  </div>
</template>

<style scoped>
.unreviewed-banner {
  display: flex;
  align-items: center;
  gap: 0.625rem;
  padding: 0.5rem 0.875rem;
  background: color-mix(in srgb, #f59e0b 14%, transparent);
  border-bottom: 1px solid color-mix(in srgb, #f59e0b 30%, transparent);
  color: var(--color-foreground, hsl(var(--foreground)));
  font-size: 0.8125rem;
}

.unreviewed-banner__icon {
  flex-shrink: 0;
  width: 1.125rem;
  height: 1.125rem;
  color: #d97706;
}

.unreviewed-banner__text {
  flex: 1;
  margin: 0;
  line-height: 1.35;
}

.unreviewed-banner__dismiss {
  flex-shrink: 0;
  padding: 0.25rem 0.625rem;
  border-radius: var(--radius, 0.5rem);
  font-weight: 500;
  background: color-mix(in srgb, currentColor 10%, transparent);
  transition: background-color 0.15s;
}

.unreviewed-banner__dismiss:hover {
  background: color-mix(in srgb, currentColor 18%, transparent);
}
</style>
