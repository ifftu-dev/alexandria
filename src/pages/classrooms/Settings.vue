<template>
  <div>
    <!-- Header -->
    <div class="border-b border-border px-6 py-4 flex items-center gap-3">
      <RouterLink
        :to="{ name: 'classroom', params: { id: classroomId } }"
        class="text-muted-foreground hover:text-foreground transition-colors"
      >
        ← Back
      </RouterLink>
      <div>
        <h1 class="text-xl font-bold text-foreground">Classroom Settings</h1>
        <p class="text-sm text-muted-foreground">{{ currentClassroom?.name }}</p>
      </div>
    </div>

    <div class="max-w-2xl mx-auto px-4 sm:px-6 py-8 space-y-6">
      <!-- General info -->
      <section class="card p-6">
        <h2 class="text-base font-semibold text-foreground mb-4">General</h2>
        <div class="space-y-4">
          <div>
            <label class="block text-sm text-muted-foreground mb-1">Invite code</label>
            <div class="flex gap-2">
              <code class="flex-1 px-3 py-2 bg-muted border border-border rounded-lg text-foreground text-sm font-mono truncate">
                {{ currentClassroom?.invite_code ?? '—' }}
              </code>
              <AppButton variant="secondary" size="sm" @click="copyInviteCode">
                Copy
              </AppButton>
            </div>
            <p class="text-xs text-muted-foreground mt-1">Share this code with others so they can find and request to join your classroom.</p>
          </div>
        </div>
      </section>

      <!-- Members -->
      <section class="card p-6">
        <h2 class="text-base font-semibold text-foreground mb-4">
          Members ({{ members.length }})
        </h2>
        <div class="space-y-2">
          <div
            v-for="m in members"
            :key="m.stake_address"
            class="flex items-center justify-between py-2"
          >
            <div class="flex items-center gap-3">
              <div class="w-8 h-8 rounded-full bg-muted flex items-center justify-center text-sm text-muted-foreground flex-shrink-0">
                {{ (m.display_name ?? m.stake_address).charAt(0).toUpperCase() }}
              </div>
              <div class="min-w-0">
                <div class="text-sm text-foreground truncate">
                  {{ m.display_name ?? formatAddress(m.stake_address) }}
                </div>
                <div class="text-xs text-muted-foreground capitalize">{{ m.role }}</div>
              </div>
            </div>

            <!-- Role + kick actions (owner only, not for self) -->
            <div
              v-if="currentClassroom?.my_role === 'owner' && m.role !== 'owner'"
              class="flex items-center gap-2 flex-shrink-0"
            >
              <select
                :value="m.role"
                @change="handleSetRole(m.stake_address, ($event.target as HTMLSelectElement).value)"
                class="text-xs bg-muted border border-border rounded px-2 py-1 text-foreground"
              >
                <option value="member">Member</option>
                <option value="moderator">Moderator</option>
              </select>
              <AppButton variant="ghost" size="xs" @click="handleKick(m.stake_address)" class="text-destructive hover:text-destructive">
                Kick
              </AppButton>
            </div>
          </div>
        </div>
      </section>

      <!-- Danger zone -->
      <section class="card p-6 border-destructive/30">
        <h2 class="text-base font-semibold text-destructive mb-4">Danger Zone</h2>
        <div class="flex items-center justify-between gap-4">
          <div class="min-w-0">
            <p class="text-sm text-foreground">Archive this classroom</p>
            <p class="text-xs text-muted-foreground">The classroom will be hidden and can no longer receive messages.</p>
          </div>
          <AppButton variant="danger" size="sm" @click="handleArchive">
            Archive
          </AppButton>
        </div>
      </section>

      <!-- Leave classroom -->
      <section
        v-if="currentClassroom?.my_role !== 'owner'"
        class="card p-6"
      >
        <div class="flex items-center justify-between gap-4">
          <div class="min-w-0">
            <p class="text-sm text-foreground">Leave this classroom</p>
            <p class="text-xs text-muted-foreground">You will need to request access again to rejoin.</p>
          </div>
          <AppButton variant="secondary" size="sm" @click="handleLeave">
            Leave
          </AppButton>
        </div>
      </section>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useLocalApi } from '@/composables/useLocalApi'
import { useClassroom } from '@/composables/useClassroom'
import AppButton from '@/components/ui/AppButton.vue'

const route = useRoute()
const router = useRouter()
const classroomId = computed(() => route.params.id as string)

const { currentClassroom, members, leaveClassroom } = useClassroom()
const { invoke } = useLocalApi()

function formatAddress(addr: string): string {
  return addr.length > 12 ? `${addr.slice(0, 8)}...${addr.slice(-4)}` : addr
}

function copyInviteCode() {
  const code = currentClassroom.value?.invite_code
  if (code) navigator.clipboard.writeText(code)
}

async function handleSetRole(stakeAddress: string, role: string) {
  try {
    await invoke('classroom_set_role', {
      classroomId: classroomId.value,
      stakeAddress,
      role,
    })
  } catch (e) {
    console.error(e)
  }
}

async function handleKick(stakeAddress: string) {
  if (!confirm('Are you sure you want to kick this member?')) return
  try {
    await invoke('classroom_kick_member', {
      classroomId: classroomId.value,
      stakeAddress,
    })
  } catch (e) {
    console.error(e)
  }
}

async function handleArchive() {
  if (!confirm('Archive this classroom? This cannot be undone.')) return
  try {
    await invoke('classroom_archive', { classroomId: classroomId.value })
    router.push({ name: 'classrooms' })
  } catch (e) {
    console.error(e)
  }
}

async function handleLeave() {
  if (!confirm('Leave this classroom?')) return
  try {
    await leaveClassroom(classroomId.value)
    router.push({ name: 'classrooms' })
  } catch (e) {
    console.error(e)
  }
}
</script>
