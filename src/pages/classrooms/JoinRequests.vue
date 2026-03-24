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
        <h1 class="text-xl font-bold">Join Requests</h1>
        <p class="text-sm text-gray-400">{{ currentClassroom?.name }}</p>
      </div>
    </div>

    <div class="max-w-2xl mx-auto px-6 py-8">
      <!-- Empty state -->
      <div v-if="joinRequests.length === 0" class="text-center py-16">
        <div class="text-4xl mb-3">✅</div>
        <p class="text-gray-400 text-sm">No pending join requests</p>
      </div>

      <!-- Requests list -->
      <div v-else class="space-y-3">
        <div
          v-for="req in joinRequests"
          :key="req.id"
          class="bg-gray-900 rounded-xl border border-gray-800 p-4 flex items-center justify-between gap-4"
        >
          <div class="flex items-center gap-3 min-w-0">
            <div class="w-10 h-10 rounded-full bg-gray-700 flex items-center justify-center text-sm flex-shrink-0">
              {{ (req.display_name ?? req.stake_address).charAt(0).toUpperCase() }}
            </div>
            <div class="min-w-0">
              <div class="text-sm font-medium text-white">
                {{ req.display_name ?? formatAddress(req.stake_address) }}
              </div>
              <div class="text-xs text-gray-500 truncate">{{ req.stake_address }}</div>
              <div v-if="req.message" class="text-xs text-gray-400 mt-1 italic">
                "{{ req.message }}"
              </div>
              <div class="text-xs text-gray-600 mt-1">
                {{ formatTime(req.requested_at) }}
              </div>
            </div>
          </div>

          <div class="flex gap-2 flex-shrink-0">
            <button
              @click="handleDeny(req)"
              :disabled="processing === req.id"
              class="px-3 py-1.5 bg-gray-800 hover:bg-gray-700 text-white text-sm rounded-lg transition-colors disabled:opacity-50"
            >
              Deny
            </button>
            <button
              @click="handleApprove(req)"
              :disabled="processing === req.id"
              class="px-3 py-1.5 bg-green-700 hover:bg-green-600 text-white text-sm rounded-lg transition-colors disabled:opacity-50"
            >
              {{ processing === req.id ? '...' : 'Approve' }}
            </button>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed } from 'vue'
import { useRoute } from 'vue-router'
import { useClassroom } from '@/composables/useClassroom'
import type { JoinRequest } from '@/types'

const route = useRoute()
const classroomId = computed(() => route.params.id as string)

const { currentClassroom, joinRequests, approveRequest, denyRequest } = useClassroom()

const processing = ref<string | null>(null)

function formatAddress(addr: string): string {
  return addr.length > 12 ? `${addr.slice(0, 8)}...${addr.slice(-4)}` : addr
}

function formatTime(iso: string): string {
  try {
    return new Date(iso).toLocaleString()
  } catch {
    return iso
  }
}

async function handleApprove(req: JoinRequest) {
  processing.value = req.id
  try {
    await approveRequest(classroomId.value, req.stake_address)
  } finally {
    processing.value = null
  }
}

async function handleDeny(req: JoinRequest) {
  processing.value = req.id
  try {
    await denyRequest(classroomId.value, req.stake_address)
  } finally {
    processing.value = null
  }
}
</script>
