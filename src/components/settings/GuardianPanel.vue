<script setup lang="ts">
// Ward-side transparency: who oversees this profile and exactly what
// they can see. Adults can unlink; minors cannot (the backend refuses
// too — this mirrors that honestly).
import { onMounted, ref } from 'vue'
import { useGuardian } from '@/composables/useGuardian'
import { useAccountStatus } from '@/composables/useAccountStatus'
import { useLocalApi } from '@/composables/useLocalApi'
import { AppBadge, AppButton, EmptyState } from '@/components/ui'

const { guardians, loaded, refreshLinks, revokeLink } = useGuardian()
const { isMinor, role } = useAccountStatus()
const { invoke } = useLocalApi()

const revokingId = ref<string | null>(null)
const error = ref('')

// Adding another guardian later (e.g. second parent) — adults and
// active minors alike can generate a fresh invite.
const inviteCode = ref('')
const generating = ref(false)

onMounted(() => void refreshLinks())

async function unlink(linkId: string) {
  revokingId.value = linkId
  error.value = ''
  try {
    await revokeLink(linkId)
  } catch (e) {
    error.value = String(e)
  } finally {
    revokingId.value = null
  }
}

async function generateInvite() {
  generating.value = true
  error.value = ''
  try {
    inviteCode.value = await invoke<string>('guardian_create_invite')
  } catch (e) {
    error.value = String(e)
  } finally {
    generating.value = false
  }
}

const SHARED_DATA = [
  'Course enrollments and completion status',
  'Per-lesson progress, scores, and time spent',
  'Graded and instructor-reviewed submissions',
  'Classroom memberships',
  'Your display name and birthdate',
]
</script>

<template>
  <div class="space-y-6">
    <div>
      <h3 class="text-base font-semibold text-foreground">My Guardian</h3>
      <p class="mt-1 text-sm text-muted-foreground">
        Guardians see your learning activity on their own device. This page
        shows exactly who is linked and what is shared.
      </p>
    </div>

    <div v-if="!loaded" class="h-20 animate-pulse rounded-lg bg-muted-foreground/8" />

    <EmptyState
      v-else-if="!guardians.length"
      title="No guardian linked"
      :description="role === 'learner'
        ? 'No one is overseeing this profile.'
        : 'Guardian links only apply to learner profiles.'"
    />

    <div v-else class="space-y-3">
      <div
        v-for="g in guardians"
        :key="g.id"
        class="rounded-xl border border-border bg-card p-4"
      >
        <div class="flex items-center gap-3">
          <span class="flex h-10 w-10 items-center justify-center rounded-full bg-[color:var(--mode-guardian-accent)]/15 text-base font-bold text-[color:var(--mode-guardian-accent)]">
            {{ (g.peer_display_name ?? 'G').charAt(0).toUpperCase() }}
          </span>
          <div class="min-w-0 flex-1">
            <p class="truncate text-sm font-medium text-foreground">
              {{ g.peer_display_name ?? 'Guardian' }}
            </p>
            <p class="truncate text-xs text-muted-foreground">{{ g.peer_did.slice(0, 32) }}…</p>
          </div>
          <AppBadge :variant="g.status === 'active' ? 'success' : 'warning'" class="capitalize">
            {{ g.status }}
          </AppBadge>
          <AppButton
            v-if="!isMinor"
            variant="danger"
            size="xs"
            :loading="revokingId === g.id"
            @click="unlink(g.id)"
          >
            Unlink
          </AppButton>
        </div>
        <p class="mt-2 text-xs text-muted-foreground">
          Linked {{ g.created_at.slice(0, 10) }}
          <template v-if="g.last_sync_at"> · last synced {{ g.last_sync_at.slice(0, 16) }}</template>
        </p>
      </div>

      <p v-if="isMinor" class="text-xs text-muted-foreground">
        You'll be able to remove your guardian when you turn 18. Until then,
        only your guardian can unlink from their device.
      </p>
    </div>

    <div class="rounded-xl border border-border bg-card p-4">
      <h4 class="text-sm font-semibold text-foreground mb-2">What your guardian can see</h4>
      <ul class="space-y-1.5 text-sm text-muted-foreground">
        <li v-for="item in SHARED_DATA" :key="item" class="flex items-start gap-2">
          <svg class="mt-0.5 h-3.5 w-3.5 shrink-0 text-[color:var(--mode-guardian-accent)]" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2.5">
            <path stroke-linecap="round" stroke-linejoin="round" d="M5 13l4 4L19 7" />
          </svg>
          {{ item }}
        </li>
      </ul>
      <p class="mt-2 text-xs text-muted-foreground">
        Everything travels encrypted directly between your devices — never
        through a server, never published to the network.
      </p>
    </div>

    <div v-if="role === 'learner'" class="rounded-xl border border-border bg-card p-4">
      <div class="flex items-center justify-between mb-1">
        <h4 class="text-sm font-semibold text-foreground">Add a guardian</h4>
        <AppButton variant="outline" size="xs" :loading="generating" @click="generateInvite">
          Generate invite
        </AppButton>
      </div>
      <p class="text-xs text-muted-foreground mb-2">
        Share a one-time code with a parent/guardian to link them.
      </p>
      <code v-if="inviteCode" class="block max-h-20 overflow-y-auto break-all rounded-lg bg-muted/30 p-2 font-mono text-xs">{{ inviteCode }}</code>
    </div>

    <p v-if="error" class="text-sm text-error">{{ error }}</p>
  </div>
</template>
