<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { useLocalApi } from '@/composables/useLocalApi'

const { invoke } = useLocalApi()

interface HealthResponse {
  status: string
  version: string
  database: string
}

interface Identity {
  stake_address: string
  payment_address: string
  display_name: string | null
  bio: string | null
}

const health = ref<HealthResponse | null>(null)
const profile = ref<Identity | null>(null)

onMounted(async () => {
  try {
    health.value = await invoke<HealthResponse>('check_health')
    profile.value = await invoke<Identity | null>('get_profile')
  } catch (e) {
    console.error('Failed to load home data:', e)
  }
})
</script>

<template>
  <div>
    <h1 class="text-xl font-bold mb-1">
      {{ profile?.display_name ? `Welcome back, ${profile.display_name}` : 'Welcome to Alexandria' }}
    </h1>
    <p class="text-sm text-[rgb(var(--color-muted-foreground))] mb-6">
      Your decentralized learning node.
    </p>

    <!-- Status cards -->
    <div class="grid grid-cols-3 gap-4 mb-8">
      <div class="card p-4">
        <div class="text-xs text-[rgb(var(--color-muted-foreground))] mb-1">Node Status</div>
        <div class="text-sm font-medium" :class="health?.status === 'ok' ? 'text-[rgb(var(--color-success))]' : 'text-[rgb(var(--color-error))]'">
          {{ health?.status ?? '...' }}
        </div>
      </div>
      <div class="card p-4">
        <div class="text-xs text-[rgb(var(--color-muted-foreground))] mb-1">Version</div>
        <div class="text-sm font-medium">{{ health?.version ?? '...' }}</div>
      </div>
      <div class="card p-4">
        <div class="text-xs text-[rgb(var(--color-muted-foreground))] mb-1">Database</div>
        <div class="text-sm font-medium">{{ health?.database ?? '...' }}</div>
      </div>
    </div>

    <!-- Identity card -->
    <div v-if="profile" class="card p-5">
      <h2 class="text-base font-semibold mb-3">Your Identity</h2>
      <div class="space-y-2 text-sm">
        <div class="flex items-start gap-2">
          <span class="text-[rgb(var(--color-muted-foreground))] w-28 shrink-0">Stake address</span>
          <code class="font-mono text-xs break-all">{{ profile.stake_address }}</code>
        </div>
        <div class="flex items-start gap-2">
          <span class="text-[rgb(var(--color-muted-foreground))] w-28 shrink-0">Payment address</span>
          <code class="font-mono text-xs break-all">{{ profile.payment_address }}</code>
        </div>
        <div v-if="profile.display_name" class="flex items-start gap-2">
          <span class="text-[rgb(var(--color-muted-foreground))] w-28 shrink-0">Display name</span>
          <span>{{ profile.display_name }}</span>
        </div>
      </div>
    </div>

    <!-- Quick actions (Phase 1 — local only) -->
    <div class="mt-8">
      <h2 class="text-base font-semibold mb-3">Quick Actions</h2>
      <div class="grid grid-cols-2 gap-4">
        <router-link
          to="/courses"
          class="card p-4 flex items-center gap-3 hover:shadow-md transition-shadow cursor-pointer"
        >
          <span class="text-lg">&#128218;</span>
          <div>
            <div class="text-sm font-medium">Browse Courses</div>
            <div class="text-xs text-[rgb(var(--color-muted-foreground))]">Explore the course catalog</div>
          </div>
        </router-link>
        <router-link
          to="/dashboard/settings"
          class="card p-4 flex items-center gap-3 hover:shadow-md transition-shadow cursor-pointer"
        >
          <span class="text-lg">&#9881;</span>
          <div>
            <div class="text-sm font-medium">Settings</div>
            <div class="text-xs text-[rgb(var(--color-muted-foreground))]">Profile, theme, node config</div>
          </div>
        </router-link>
      </div>
    </div>
  </div>
</template>
