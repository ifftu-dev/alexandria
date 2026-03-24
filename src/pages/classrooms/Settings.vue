<template>
  <div class="min-h-screen bg-gray-950 text-white">
    <!-- Header -->
    <div class="border-b border-gray-800 px-6 py-4 flex items-center gap-3">
      <RouterLink
        :to="{ name: 'classroom', params: { id: classroomId } }"
        class="text-gray-400 hover:text-white transition-colors"
      >
        ← Back
      </RouterLink>
      <div>
        <h1 class="text-xl font-bold">Classroom Settings</h1>
        <p class="text-sm text-gray-400">{{ currentClassroom?.name }}</p>
      </div>
    </div>

    <div class="max-w-2xl mx-auto px-6 py-8 space-y-8">
      <!-- General info -->
      <section class="bg-gray-900 rounded-xl border border-gray-800 p-6">
        <h2 class="text-base font-semibold text-white mb-4">General</h2>
        <div class="space-y-4">
          <div>
            <label class="block text-sm text-gray-400 mb-1">Invite code</label>
            <div class="flex gap-2">
              <code class="flex-1 px-3 py-2 bg-gray-800 border border-gray-700 rounded-lg text-white text-sm font-mono">
                {{ currentClassroom?.invite_code ?? '—' }}
              </code>
              <button
                @click="copyInviteCode"
                class="px-3 py-2 bg-gray-700 hover:bg-gray-600 text-white text-sm rounded-lg transition-colors"
              >
                Copy
              </button>
            </div>
            <p class="text-xs text-gray-500 mt-1">Share this code with others so they can find and request to join your classroom.</p>
          </div>
        </div>
      </section>

      <!-- Members -->
      <section class="bg-gray-900 rounded-xl border border-gray-800 p-6">
        <h2 class="text-base font-semibold text-white mb-4">
          Members ({{ members.length }})
        </h2>
        <div class="space-y-2">
          <div
            v-for="m in members"
            :key="m.stake_address"
            class="flex items-center justify-between py-2"
          >
            <div class="flex items-center gap-3">
              <div class="w-8 h-8 rounded-full bg-gray-700 flex items-center justify-center text-sm flex-shrink-0">
                {{ (m.display_name ?? m.stake_address).charAt(0).toUpperCase() }}
              </div>
              <div>
                <div class="text-sm text-white">
                  {{ m.display_name ?? formatAddress(m.stake_address) }}
                </div>
                <div class="text-xs text-gray-500 capitalize">{{ m.role }}</div>
              </div>
            </div>

            <!-- Role + kick actions (owner only, not for self) -->
            <div
              v-if="currentClassroom?.my_role === 'owner' && m.role !== 'owner'"
              class="flex items-center gap-2"
            >
              <select
                :value="m.role"
                @change="handleSetRole(m.stake_address, ($event.target as HTMLSelectElement).value)"
                class="text-xs bg-gray-800 border border-gray-700 rounded px-2 py-1 text-gray-300"
              >
                <option value="member">Member</option>
                <option value="moderator">Moderator</option>
              </select>
              <button
                @click="handleKick(m.stake_address)"
                class="text-xs text-red-400 hover:text-red-300 transition-colors px-2 py-1"
              >
                Kick
              </button>
            </div>
          </div>
        </div>
      </section>

      <!-- Danger zone -->
      <section class="bg-red-950/20 rounded-xl border border-red-800/40 p-6">
        <h2 class="text-base font-semibold text-red-400 mb-4">Danger Zone</h2>
        <div class="flex items-center justify-between">
          <div>
            <p class="text-sm text-white">Archive this classroom</p>
            <p class="text-xs text-gray-500">The classroom will be hidden and can no longer receive messages.</p>
          </div>
          <button
            @click="handleArchive"
            class="px-4 py-2 bg-red-900 hover:bg-red-800 text-white text-sm font-medium rounded-lg transition-colors"
          >
            Archive
          </button>
        </div>
      </section>

      <!-- Leave classroom -->
      <section
        v-if="currentClassroom?.my_role !== 'owner'"
        class="bg-gray-900 rounded-xl border border-gray-800 p-6"
      >
        <div class="flex items-center justify-between">
          <div>
            <p class="text-sm text-white">Leave this classroom</p>
            <p class="text-xs text-gray-500">You will need to request access again to rejoin.</p>
          </div>
          <button
            @click="handleLeave"
            class="px-4 py-2 bg-gray-700 hover:bg-gray-600 text-white text-sm font-medium rounded-lg transition-colors"
          >
            Leave
          </button>
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
