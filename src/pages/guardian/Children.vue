<script setup lang="ts">
// Parent home: linked children with add-child + sync controls.
import { onMounted, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useI18n } from 'vue-i18n'
import { useGuardian, childAge } from '@/composables/useGuardian'
import { AppButton, AppBadge, AppInput, AppModal, EmptyState } from '@/components/ui'

const route = useRoute()
const router = useRouter()
const { t } = useI18n()
const { children, loaded, refreshLinks, acceptInvite, syncNow } = useGuardian()

const showAdd = ref(false)
const inviteCode = ref('')
const accepting = ref(false)
const addError = ref('')
const syncing = ref(false)

onMounted(() => void refreshLinks())

// Arrived from a deep link (alexandria://guardian/accept?code=…): prefill the
// add-child modal so the parent can confirm, then drop the query so a refresh
// doesn't re-trigger it. A watcher (not just onMounted) also covers the case
// where the parent is already on this page when the link fires.
watch(
  () => route.query.accept,
  (code) => {
    if (typeof code === 'string' && code.trim()) {
      inviteCode.value = code.trim()
      showAdd.value = true
      void router.replace({ path: '/guardian', query: {} })
    }
  },
  { immediate: true },
)

async function submitInvite() {
  if (!inviteCode.value.trim()) return
  accepting.value = true
  addError.value = ''
  try {
    const link = await acceptInvite(inviteCode.value.trim())
    inviteCode.value = ''
    showAdd.value = false
    if (link.status === 'pending') {
      addError.value = ''
    }
  } catch (e) {
    addError.value = String(e)
  } finally {
    accepting.value = false
  }
}

async function runSync() {
  syncing.value = true
  try {
    await syncNow()
  } finally {
    syncing.value = false
  }
}

function statusVariant(status: string): 'success' | 'warning' | 'error' {
  if (status === 'active') return 'success'
  if (status === 'pending') return 'warning'
  return 'error'
}

function statusLabel(status: string): string {
  if (status === 'active') return t('guardian.children.statusActive')
  if (status === 'pending') return t('guardian.children.statusWaiting')
  return t('guardian.children.statusInactive')
}
</script>

<template>
  <div class="space-y-6">
    <div class="flex items-start justify-between gap-4">
      <div>
        <h1 class="text-2xl font-bold text-foreground">{{ $t('guardian.children.title') }}</h1>
        <p class="mt-1 text-sm text-muted-foreground">
          {{ $t('guardian.children.subtitle') }}
        </p>
      </div>
      <div class="flex shrink-0 gap-2">
        <AppButton variant="outline" size="sm" :loading="syncing" @click="runSync">
          {{ $t('guardian.actions.syncNow') }}
        </AppButton>
        <AppButton size="sm" @click="showAdd = true">{{ $t('guardian.actions.addChild') }}</AppButton>
      </div>
    </div>

    <div v-if="!loaded" class="grid gap-4 sm:grid-cols-2">
      <div v-for="i in 2" :key="i" class="h-36 animate-pulse rounded-xl bg-muted-foreground/8" />
    </div>

    <EmptyState
      v-else-if="!children.length"
      :title="$t('guardian.children.emptyTitle')"
      :description="$t('guardian.children.emptyDescription')"
    />

    <div v-else class="grid gap-4 sm:grid-cols-2">
      <button
        v-for="child in children"
        :key="child.id"
        class="rounded-xl border border-border bg-card p-5 text-start transition-colors hover:border-primary/50"
        @click="router.push(`/guardian/child/${child.id}`)"
      >
        <div class="flex items-center gap-3">
          <span class="flex h-11 w-11 items-center justify-center rounded-full bg-[color:var(--mode-guardian-accent)]/15 text-lg font-bold text-[color:var(--mode-guardian-accent)]">
            {{ (child.peer_display_name ?? '?').charAt(0).toUpperCase() }}
          </span>
          <div class="min-w-0">
            <p class="truncate font-semibold text-foreground">
              {{ child.peer_display_name ?? $t('guardian.children.unnamedChild') }}
              <span v-if="childAge(child) !== null" class="ms-1 text-sm font-normal text-muted-foreground">
                · {{ $t('guardian.children.ageShort', { age: childAge(child) }) }}
              </span>
            </p>
            <p class="truncate text-xs text-muted-foreground">{{ child.peer_did.slice(0, 28) }}…</p>
          </div>
          <AppBadge :variant="statusVariant(child.status)" class="ms-auto shrink-0 capitalize">
            {{ statusLabel(child.status) }}
          </AppBadge>
        </div>
        <p class="mt-3 text-xs text-muted-foreground">
          {{ child.last_sync_at ? $t('guardian.children.lastSynced', { time: child.last_sync_at.slice(0, 16) }) : $t('guardian.children.notSynced') }}
        </p>
      </button>
    </div>

    <!-- Add child modal -->
    <AppModal :open="showAdd" :title="$t('guardian.children.addModalTitle')" @close="showAdd = false">
      <div class="space-y-4">
        <p class="text-sm text-muted-foreground">
          {{ $t('guardian.children.addModalBody') }}
        </p>
        <AppInput
          v-model="inviteCode"
          :label="$t('guardian.children.inviteCodeLabel')"
          :placeholder="$t('guardian.children.inviteCodePlaceholder')"
        />
        <p v-if="addError" class="text-sm text-error">{{ addError }}</p>
        <div class="flex justify-end gap-2">
          <AppButton variant="ghost" @click="showAdd = false">{{ $t('common.actions.cancel') }}</AppButton>
          <AppButton :loading="accepting" @click="submitInvite">{{ $t('guardian.children.linkChild') }}</AppButton>
        </div>
      </div>
    </AppModal>
  </div>
</template>
