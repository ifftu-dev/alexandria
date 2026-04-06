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
        <h1 class="text-xl font-bold text-foreground">Join Requests</h1>
        <p class="text-sm text-muted-foreground">{{ currentClassroom?.name }}</p>
      </div>
    </div>

    <div class="max-w-2xl mx-auto px-4 sm:px-6 py-8">
      <!-- Empty state -->
      <div v-if="joinRequests.length === 0" class="flex flex-col items-center justify-center py-16 text-center">
        <div class="w-16 h-16 rounded-2xl bg-success/10 flex items-center justify-center text-3xl mb-4">
          ✅
        </div>
        <p class="text-muted-foreground text-sm">No pending join requests</p>
      </div>

      <!-- Requests list -->
      <div v-else class="space-y-3">
        <div
          v-for="req in joinRequests"
          :key="req.id"
          class="card p-4 flex items-center justify-between gap-4"
        >
          <div class="flex items-center gap-3 min-w-0">
            <div class="w-10 h-10 rounded-full bg-muted flex items-center justify-center text-sm text-muted-foreground flex-shrink-0">
              {{ (req.display_name ?? req.stake_address).charAt(0).toUpperCase() }}
            </div>
            <div class="min-w-0">
              <div class="text-sm font-medium text-foreground">
                {{ req.display_name ?? formatAddress(req.stake_address) }}
              </div>
              <div class="text-xs text-muted-foreground truncate">{{ req.stake_address }}</div>
              <div v-if="req.message" class="text-xs text-muted-foreground mt-1 italic">
                "{{ req.message }}"
              </div>
              <div class="text-xs text-muted-foreground/60 mt-1">
                {{ formatTime(req.requested_at) }}
              </div>
            </div>
          </div>

          <div class="flex gap-2 flex-shrink-0">
            <AppButton
              variant="secondary"
              size="xs"
              :disabled="processing === req.id"
              @click="handleDeny(req)"
            >
              Deny
            </AppButton>
            <AppButton
              variant="primary"
              size="xs"
              :disabled="processing === req.id"
              :loading="processing === req.id"
              @click="handleApprove(req)"
            >
              Approve
            </AppButton>
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
import AppButton from '@/components/ui/AppButton.vue'
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
