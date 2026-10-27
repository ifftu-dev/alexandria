<script setup lang="ts">
// The inbox. Rows come from `useNotifications`, newest first; a tap marks the
// row read and follows its route. Titles and bodies are rendered from the
// kind’s generic localized copy, with English fallback. Stored or provider
// text is never used as a fallback for an unknown kind.
import { computed, onMounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { useRoute, useRouter } from 'vue-router'
import { AppBadge, AppButton, EmptyState } from '@/components/ui'
import { useNotifications } from '@/composables/useNotifications'
import type { AppNotification } from '@/types'

const { t, te } = useI18n()
const route = useRoute()
const router = useRouter()
const { items, unread, loading, load, markAllRead, clear, open } = useNotifications()

const confirmingClear = ref(false)

onMounted(async () => {
  await load().catch(() => {})
  // `alexandria://notification/<id>` lands here with `?open=<id>`: open that
  // row as a tap would, then drop the query so a reload does not repeat it.
  const wanted = typeof route.query.open === 'string' ? route.query.open : null
  if (!wanted) return
  await router.replace({ path: route.path })
  const row = items.value.find((n) => n.id === wanted)
  if (row) await open(row).catch(() => {})
})

function titleOf(n: AppNotification): string {
  const key = `notifications.kinds.${n.kind}.title`
  return te(key) || te(key, 'en') ? t(key) : t('notifications.kinds.generic.title')
}

function bodyOf(n: AppNotification): string {
  const key = `notifications.kinds.${n.kind}.body`
  return te(key) || te(key, 'en') ? t(key) : t('notifications.kinds.generic.body')
}

const formatter = computed(() => new Intl.DateTimeFormat(undefined, { dateStyle: 'medium', timeStyle: 'short' }))

function when(n: AppNotification): string {
  const d = new Date(n.createdAt)
  return Number.isNaN(d.getTime()) ? n.createdAt : formatter.value.format(d)
}

async function onClear() {
  if (!confirmingClear.value) {
    confirmingClear.value = true
    return
  }
  confirmingClear.value = false
  await clear().catch(() => {})
}
</script>

<template>
  <div class="mx-auto max-w-3xl space-y-4 p-4 md:p-6">
    <header class="flex flex-wrap items-center justify-between gap-3">
      <h1 class="text-xl font-semibold text-foreground">
        {{ $t('notifications.title') }}
        <AppBadge v-if="unread > 0" variant="primary" class="ms-2 align-middle">{{ unread }}</AppBadge>
      </h1>
      <div class="flex items-center gap-2">
        <AppButton variant="outline" size="sm" :disabled="unread === 0" @click="markAllRead().catch(() => {})">
          {{ $t('notifications.page.markAllRead') }}
        </AppButton>
        <AppButton variant="ghost" size="sm" :disabled="items.length === 0" @click="onClear">
          {{ confirmingClear ? $t('notifications.page.clearConfirm') : $t('notifications.page.clear') }}
        </AppButton>
      </div>
    </header>

    <EmptyState
      v-if="!loading && items.length === 0"
      :title="$t('notifications.page.empty')"
      :description="$t('notifications.page.emptyHint')"
    />

    <ul v-else class="divide-y divide-border rounded-lg border border-border bg-card">
      <li v-for="n in items" :key="n.id">
        <button
          type="button"
          class="flex w-full items-start gap-3 px-4 py-3 text-start transition-colors hover:bg-muted/40"
          :class="n.readAt ? 'opacity-80' : ''"
          @click="open(n)"
        >
          <span
            class="mt-2 h-2 w-2 shrink-0 rounded-full"
            :class="n.readAt ? 'bg-transparent' : 'bg-primary'"
            :aria-label="n.readAt ? undefined : $t('notifications.page.unreadLabel')"
          />
          <span class="min-w-0 flex-1">
            <span class="flex flex-wrap items-center gap-2">
              <span class="text-sm font-medium text-foreground">{{ titleOf(n) }}</span>
              <AppBadge v-if="n.source === 'push'" variant="secondary">{{ $t('notifications.page.viaPush') }}</AppBadge>
            </span>
            <span class="mt-0.5 block text-sm text-muted-foreground">{{ bodyOf(n) }}</span>
            <span class="mt-1 block text-xs text-muted-foreground/70">{{ when(n) }}</span>
          </span>
          <svg v-if="n.route" class="mt-1 h-4 w-4 shrink-0 text-muted-foreground rtl:-scale-x-100" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true">
            <path stroke-linecap="round" stroke-linejoin="round" d="M9 5l7 7-7 7" />
          </svg>
        </button>
      </li>
    </ul>
  </div>
</template>
