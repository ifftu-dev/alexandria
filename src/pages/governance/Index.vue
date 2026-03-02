<script setup lang="ts">
import { ref, onMounted, computed } from 'vue'
import { useLocalApi } from '@/composables/useLocalApi'
import { StatusBadge } from '@/components/ui'
import type { DaoInfo } from '@/types'

const { invoke } = useLocalApi()

const daos = ref<DaoInfo[]>([])
const loading = ref(true)
const search = ref('')

const filtered = computed(() => {
  if (!search.value.trim()) return daos.value
  const q = search.value.toLowerCase()
  return daos.value.filter(d =>
    d.name.toLowerCase().includes(q) ||
    d.scope_type.toLowerCase().includes(q) ||
    (d.description || '').toLowerCase().includes(q)
  )
})

// Stats
const totalDaos = computed(() => daos.value.length)
const activeDaos = computed(() => daos.value.filter(d => d.status === 'active').length)
const totalSeats = computed(() => daos.value.reduce((s, d) => s + d.committee_size, 0))

onMounted(async () => {
  try {
    daos.value = await invoke<DaoInfo[]>('list_daos')
  } catch (e) {
    console.error('Failed to load DAOs:', e)
  } finally {
    loading.value = false
  }
})
</script>

<template>
  <div>
    <!-- Header -->
    <div class="mb-8 flex items-start justify-between">
      <div class="flex items-center gap-3">
        <div class="flex h-10 w-10 items-center justify-center rounded-lg bg-amber-500/10">
          <svg class="h-5 w-5 text-amber-600 dark:text-amber-400" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
            <path stroke-linecap="round" stroke-linejoin="round" d="M12 21v-8.25M15.75 21v-8.25M8.25 21v-8.25M3 9l9-6 9 6m-1.5 12V10.332A48.36 48.36 0 0012 9.75c-2.551 0-5.056.2-7.5.582V21M3 21h18M12 6.75h.008v.008H12V6.75z" />
          </svg>
        </div>
        <div>
          <h1 class="text-2xl font-bold text-foreground sm:text-3xl">Governance</h1>
          <p class="text-sm text-muted-foreground">
            DAOs governing the knowledge taxonomy
          </p>
        </div>
      </div>
    </div>

    <!-- Skeleton -->
    <div v-if="loading" class="space-y-6">
      <div class="grid grid-cols-1 sm:grid-cols-3 gap-4">
        <div v-for="i in 3" :key="i" class="animate-pulse rounded-xl border border-border bg-card p-6">
          <div class="h-3 w-20 rounded bg-muted-foreground/15 mb-3" />
          <div class="h-8 w-10 rounded bg-muted-foreground/20" />
        </div>
      </div>
      <div class="h-10 w-72 animate-pulse rounded-lg bg-muted-foreground/10" />
      <div class="grid grid-cols-1 sm:grid-cols-2 gap-4">
        <div v-for="i in 4" :key="i" class="animate-pulse rounded-xl border border-border bg-card p-5">
          <div class="flex items-start gap-3">
            <div class="h-10 w-10 rounded-lg bg-muted-foreground/15" />
            <div class="flex-1 space-y-2">
              <div class="h-4 w-40 rounded bg-muted-foreground/15" />
              <div class="h-3 w-full rounded bg-muted-foreground/10" />
            </div>
          </div>
        </div>
      </div>
    </div>

    <template v-else>
      <!-- Stats -->
      <div class="mb-8 grid grid-cols-1 sm:grid-cols-3 gap-4">
        <div class="rounded-xl border border-border bg-card p-6">
          <p class="text-sm text-muted-foreground">Total DAOs</p>
          <p class="mt-2 text-3xl font-bold text-foreground">{{ totalDaos }}</p>
        </div>
        <div class="rounded-xl border border-border bg-card p-6">
          <p class="text-sm text-muted-foreground">Active</p>
          <p class="mt-2 text-3xl font-bold text-emerald-500">{{ activeDaos }}</p>
        </div>
        <div class="rounded-xl border border-border bg-card p-6">
          <p class="text-sm text-muted-foreground">Total Committee Seats</p>
          <p class="mt-2 text-3xl font-bold text-amber-500">{{ totalSeats }}</p>
        </div>
      </div>

      <!-- Search -->
      <div class="mb-6">
        <input
          v-model="search"
          class="w-full max-w-sm rounded-lg border border-border bg-background px-4 py-2.5 text-sm text-foreground placeholder-muted-foreground/50 transition-colors focus:border-primary focus:outline-none focus:ring-1 focus:ring-primary"
          placeholder="Search DAOs..."
        >
      </div>

      <!-- Empty state -->
      <div v-if="filtered.length === 0" class="py-16 text-center">
        <div class="mx-auto mb-4 flex h-16 w-16 items-center justify-center rounded-full bg-amber-500/10">
          <svg class="h-8 w-8 text-amber-500" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.5">
            <path stroke-linecap="round" stroke-linejoin="round" d="M12 21v-8.25M15.75 21v-8.25M8.25 21v-8.25M3 9l9-6 9 6m-1.5 12V10.332A48.36 48.36 0 0012 9.75c-2.551 0-5.056.2-7.5.582V21M3 21h18M12 6.75h.008v.008H12V6.75z" />
          </svg>
        </div>
        <h3 class="text-lg font-semibold text-foreground">
          {{ search ? 'No DAOs match your search' : 'No DAOs yet' }}
        </h3>
        <p class="mt-1 text-sm text-muted-foreground max-w-sm mx-auto">
          {{ search ? 'Try a different search term.' : 'DAOs are created automatically when subject fields or subjects are added to the taxonomy.' }}
        </p>
      </div>

      <!-- DAO grid -->
      <div v-else class="grid grid-cols-1 sm:grid-cols-2 gap-4">
        <router-link
          v-for="dao in filtered"
          :key="dao.id"
          :to="`/governance/${dao.id}`"
          class="group rounded-xl border border-border bg-card p-5 transition-all hover:border-amber-500/30 hover:shadow-sm"
        >
          <div class="flex items-start gap-3">
            <div class="flex h-10 w-10 flex-shrink-0 items-center justify-center rounded-lg bg-amber-500/10 text-lg">
              <span v-if="dao.icon_emoji">{{ dao.icon_emoji }}</span>
              <span v-else class="text-sm font-bold text-amber-600 dark:text-amber-400">{{ dao.scope_type === 'subject_field' ? 'SF' : 'SU' }}</span>
            </div>
            <div class="flex-1 min-w-0">
              <div class="flex items-center gap-2 mb-1">
                <h3 class="text-sm font-semibold text-foreground truncate group-hover:text-amber-600 dark:group-hover:text-amber-400 transition-colors">
                  {{ dao.name }}
                </h3>
                <StatusBadge :status="dao.status" />
              </div>
              <p v-if="dao.description" class="text-xs text-muted-foreground line-clamp-2 mb-2">
                {{ dao.description }}
              </p>
              <div class="flex items-center gap-3 text-xs text-muted-foreground">
                <span class="flex items-center gap-1">
                  <svg class="h-3.5 w-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                    <path stroke-linecap="round" stroke-linejoin="round" d="M15 19.128a9.38 9.38 0 002.625.372 9.337 9.337 0 004.121-.952 4.125 4.125 0 00-7.533-2.493M15 19.128v-.003c0-1.113-.285-2.16-.786-3.07M15 19.128v.106A12.318 12.318 0 018.624 21c-2.331 0-4.512-.645-6.374-1.766l-.001-.109a6.375 6.375 0 0111.964-3.07M12 6.375a3.375 3.375 0 11-6.75 0 3.375 3.375 0 016.75 0zm8.25 2.25a2.625 2.625 0 11-5.25 0 2.625 2.625 0 015.25 0z" />
                  </svg>
                  {{ dao.committee_size }} seats
                </span>
                <span>{{ dao.scope_type === 'subject_field' ? 'Subject Field' : 'Subject' }}</span>
              </div>
            </div>
          </div>
        </router-link>
      </div>
    </template>

  </div>
</template>
