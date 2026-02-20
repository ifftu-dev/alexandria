<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { useLocalApi } from '@/composables/useLocalApi'
import { useAuth } from '@/composables/useAuth'
import { StatusBadge, DataRow } from '@/components/ui'
import type { HealthResponse, P2PStatus } from '@/types'

const { invoke } = useLocalApi()
const { identity } = useAuth()

const health = ref<HealthResponse | null>(null)
const p2p = ref<P2PStatus | null>(null)

onMounted(async () => {
  try {
    health.value = await invoke<HealthResponse>('check_health')
  } catch (e) {
    console.error('Failed to load health:', e)
  }

  try {
    p2p.value = await invoke<P2PStatus>('p2p_status')
  } catch {
    // P2P may not be running
  }
})
</script>

<template>
  <div>
    <h1 class="text-xl font-bold mb-1">
      {{ identity?.display_name ? `Welcome back, ${identity.display_name}` : 'Welcome to Alexandria' }}
    </h1>
    <p class="text-sm text-[rgb(var(--color-muted-foreground))] mb-6">
      Your decentralized learning node.
    </p>

    <!-- Status cards -->
    <div class="grid grid-cols-3 gap-4 mb-8">
      <div class="card p-4">
        <div class="text-xs text-[rgb(var(--color-muted-foreground))] mb-1">Node Status</div>
        <StatusBadge :status="health?.status ?? 'loading'" />
      </div>
      <div class="card p-4">
        <div class="text-xs text-[rgb(var(--color-muted-foreground))] mb-1">Version</div>
        <div class="text-sm font-medium">{{ health?.version ?? '...' }}</div>
      </div>
      <div class="card p-4">
        <div class="text-xs text-[rgb(var(--color-muted-foreground))] mb-1">P2P Network</div>
        <StatusBadge :status="p2p?.running ? 'online' : 'offline'" />
        <div v-if="p2p?.running" class="text-xs text-[rgb(var(--color-muted-foreground))] mt-1">
          {{ p2p.connected_peers }} peer{{ p2p.connected_peers !== 1 ? 's' : '' }}
        </div>
      </div>
    </div>

    <!-- Identity card -->
    <div v-if="identity" class="card p-5 mb-8">
      <h2 class="text-base font-semibold mb-3">Your Identity</h2>
      <div class="space-y-2">
        <DataRow label="Stake address" mono>{{ identity.stake_address }}</DataRow>
        <DataRow label="Payment address" mono>{{ identity.payment_address }}</DataRow>
        <DataRow v-if="identity.display_name" label="Display name">{{ identity.display_name }}</DataRow>
        <DataRow v-if="identity.profile_hash" label="Profile CID" mono>{{ identity.profile_hash }}</DataRow>
      </div>
    </div>

    <!-- Quick actions -->
    <div>
      <h2 class="text-base font-semibold mb-3">Quick Actions</h2>
      <div class="grid grid-cols-2 lg:grid-cols-4 gap-4">
        <router-link
          to="/courses"
          class="card card-interactive p-4 flex items-center gap-3 cursor-pointer"
        >
          <div class="w-9 h-9 rounded-lg bg-[rgb(var(--color-primary)/0.1)] flex items-center justify-center">
            <svg class="w-4.5 h-4.5 text-[rgb(var(--color-primary))]" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
              <path stroke-linecap="round" stroke-linejoin="round" d="M12 6.253v13m0-13C10.832 5.477 9.246 5 7.5 5S4.168 5.477 3 6.253v13C4.168 18.477 5.754 18 7.5 18s3.332.477 4.5 1.253m0-13C13.168 5.477 14.754 5 16.5 5c1.747 0 3.332.477 4.5 1.253v13C19.832 18.477 18.247 18 16.5 18c-1.746 0-3.332.477-4.5 1.253" />
            </svg>
          </div>
          <div>
            <div class="text-sm font-medium">Browse Courses</div>
            <div class="text-xs text-[rgb(var(--color-muted-foreground))]">Explore the catalog</div>
          </div>
        </router-link>

        <router-link
          to="/skills"
          class="card card-interactive p-4 flex items-center gap-3 cursor-pointer"
        >
          <div class="w-9 h-9 rounded-lg bg-[rgb(var(--color-success)/0.1)] flex items-center justify-center">
            <svg class="w-4.5 h-4.5 text-[rgb(var(--color-success))]" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
              <path stroke-linecap="round" stroke-linejoin="round" d="M9 12l2 2 4-4M7.835 4.697a3.42 3.42 0 001.946-.806 3.42 3.42 0 014.438 0 3.42 3.42 0 001.946.806 3.42 3.42 0 013.138 3.138 3.42 3.42 0 00.806 1.946 3.42 3.42 0 010 4.438 3.42 3.42 0 00-.806 1.946 3.42 3.42 0 01-3.138 3.138 3.42 3.42 0 00-1.946.806 3.42 3.42 0 01-4.438 0 3.42 3.42 0 00-1.946-.806 3.42 3.42 0 01-3.138-3.138 3.42 3.42 0 00-.806-1.946 3.42 3.42 0 010-4.438 3.42 3.42 0 00.806-1.946 3.42 3.42 0 013.138-3.138z" />
            </svg>
          </div>
          <div>
            <div class="text-sm font-medium">Skills</div>
            <div class="text-xs text-[rgb(var(--color-muted-foreground))]">Taxonomy & proofs</div>
          </div>
        </router-link>

        <router-link
          to="/governance"
          class="card card-interactive p-4 flex items-center gap-3 cursor-pointer"
        >
          <div class="w-9 h-9 rounded-lg bg-[rgb(var(--color-governance)/0.1)] flex items-center justify-center">
            <svg class="w-4.5 h-4.5 text-[rgb(var(--color-governance))]" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
              <path stroke-linecap="round" stroke-linejoin="round" d="M3 6l3 1m0 0l-3 9a5.002 5.002 0 006.001 0M6 7l3 9M6 7l6-2m6 2l3-1m-3 1l-3 9a5.002 5.002 0 006.001 0M18 7l3 9m-3-9l-6-2m0-2v2m0 16V5m0 16H9m3 0h3" />
            </svg>
          </div>
          <div>
            <div class="text-sm font-medium">Governance</div>
            <div class="text-xs text-[rgb(var(--color-muted-foreground))]">DAOs & proposals</div>
          </div>
        </router-link>

        <router-link
          to="/dashboard/settings"
          class="card card-interactive p-4 flex items-center gap-3 cursor-pointer"
        >
          <div class="w-9 h-9 rounded-lg bg-[rgb(var(--color-muted))] flex items-center justify-center">
            <svg class="w-4.5 h-4.5 text-[rgb(var(--color-muted-foreground))]" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
              <path stroke-linecap="round" stroke-linejoin="round" d="M10.325 4.317c.426-1.756 2.924-1.756 3.35 0a1.724 1.724 0 002.573 1.066c1.543-.94 3.31.826 2.37 2.37a1.724 1.724 0 001.066 2.573c1.756.426 1.756 2.924 0 3.35a1.724 1.724 0 00-1.066 2.573c.94 1.543-.826 3.31-2.37 2.37a1.724 1.724 0 00-2.573 1.066c-.426 1.756-2.924 1.756-3.35 0a1.724 1.724 0 00-2.573-1.066c-1.543.94-3.31-.826-2.37-2.37a1.724 1.724 0 00-1.066-2.573c-1.756-.426-1.756-2.924 0-3.35a1.724 1.724 0 001.066-2.573c-.94-1.543.826-3.31 2.37-2.37.996.608 2.296.07 2.572-1.065z" />
              <path stroke-linecap="round" stroke-linejoin="round" d="M15 12a3 3 0 11-6 0 3 3 0 016 0z" />
            </svg>
          </div>
          <div>
            <div class="text-sm font-medium">Settings</div>
            <div class="text-xs text-[rgb(var(--color-muted-foreground))]">Profile & node config</div>
          </div>
        </router-link>
      </div>
    </div>
  </div>
</template>
