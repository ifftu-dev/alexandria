<script setup lang="ts">
/**
 * Who sent something, as far as the app can vouch for it.
 *
 * A verified organisation is named by the file its own domain publishes, so
 * the name and the domain are shown together: the domain is what was proven.
 * An unverified one gets no name at all. The payload carries a name too, but
 * that is whatever the sender typed, and printing it here in the same place
 * as a proven one would present a claim as a fact.
 */
import { useI18n } from 'vue-i18n'
import type { OrgCheck } from '@/types'

defineProps<{ org: OrgCheck }>()

const { t } = useI18n()
</script>

<template>
  <div v-if="org.verified" class="min-w-0" data-testid="org-badge">
    <p class="truncate text-sm font-semibold text-foreground">{{ org.name }}</p>
    <p class="flex items-center gap-1 text-xs text-success">
      <svg class="h-3.5 w-3.5 shrink-0" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true">
        <path stroke-linecap="round" stroke-linejoin="round" d="M9 12l2 2 4-4m5.618-4.016A11.955 11.955 0 0112 2.944a11.955 11.955 0 01-8.618 3.04A12.02 12.02 0 003 9c0 5.591 3.824 10.29 9 11.622 5.176-1.332 9-6.03 9-11.622 0-1.042-.133-2.052-.382-3.016z" />
      </svg>
      <span class="truncate">{{ t('organisations.badge.verified', { domain: org.domain ?? '' }) }}</span>
    </p>
  </div>
  <div
    v-else
    role="note"
    class="min-w-0 rounded-lg border border-warning/40 bg-warning/10 px-3 py-2 text-xs text-warning"
    data-testid="org-badge-unverified"
  >
    <p class="font-medium">
      {{ t('organisations.badge.unverified', { problem: org.problem ?? t('organisations.badge.noProof') }) }}
    </p>
    <p v-if="org.domain" class="mt-0.5">{{ t('organisations.badge.claims', { domain: org.domain }) }}</p>
  </div>
</template>
