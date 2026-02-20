<script setup lang="ts">
import { ref, onMounted, computed } from 'vue'
import { useLocalApi } from '@/composables/useLocalApi'
import { AppSpinner, EmptyState, StatusBadge } from '@/components/ui'
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
    d.scope_type.toLowerCase().includes(q)
  )
})

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
    <div class="flex items-center justify-between mb-6">
      <div>
        <h1 class="text-xl font-bold">Governance</h1>
        <p class="text-sm text-[rgb(var(--color-muted-foreground))]">
          DAOs governing the knowledge taxonomy. Each subject field and subject has a DAO.
        </p>
      </div>
    </div>

    <!-- Search -->
    <div class="mb-6">
      <input
        v-model="search"
        class="input max-w-sm"
        placeholder="Search DAOs..."
      >
    </div>

    <AppSpinner v-if="loading" label="Loading DAOs..." />

    <EmptyState
      v-else-if="filtered.length === 0"
      title="No DAOs found"
      :description="search ? 'Try a different search term.' : 'DAOs are created via governance proposals or seed data.'"
    />

    <div v-else class="grid grid-cols-1 sm:grid-cols-2 gap-4">
      <router-link
        v-for="dao in filtered"
        :key="dao.id"
        :to="`/governance/${dao.id}`"
        class="card dao-card p-4"
      >
        <div class="flex items-start gap-3">
          <div class="governance-seal text-sm font-bold">
            {{ dao.scope_type === 'subject_field' ? 'SF' : 'SU' }}
          </div>
          <div class="flex-1 min-w-0">
            <div class="text-sm font-semibold truncate">{{ dao.name }}</div>
            <p v-if="dao.description" class="text-xs text-[rgb(var(--color-muted-foreground))] line-clamp-2 mt-0.5">
              {{ dao.description }}
            </p>
            <div class="flex items-center gap-2 mt-2">
              <StatusBadge :status="dao.status" />
              <span class="text-xs text-[rgb(var(--color-muted-foreground))]">
                {{ dao.committee_size }} seats
              </span>
            </div>
          </div>
        </div>
      </router-link>
    </div>
  </div>
</template>
