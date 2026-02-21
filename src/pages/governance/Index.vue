<script setup lang="ts">
import { ref, onMounted, computed } from 'vue'
import { useLocalApi } from '@/composables/useLocalApi'
import { useAuth } from '@/composables/useAuth'
import { AppButton, AppSpinner, AppInput, AppModal, EmptyState, StatusBadge } from '@/components/ui'
import type { DaoInfo, CreateDaoParams, SubjectFieldInfo, SubjectInfo } from '@/types'

const { invoke } = useLocalApi()
const { stakeAddress } = useAuth()

const daos = ref<DaoInfo[]>([])
const loading = ref(true)
const search = ref('')
const error = ref('')

const filtered = computed(() => {
  if (!search.value.trim()) return daos.value
  const q = search.value.toLowerCase()
  return daos.value.filter(d =>
    d.name.toLowerCase().includes(q) ||
    d.scope_type.toLowerCase().includes(q) ||
    (d.description || '').toLowerCase().includes(q)
  )
})

// Create DAO state
const showCreateDao = ref(false)
const createForm = ref<CreateDaoParams>({
  name: '',
  description: '',
  scope_type: 'subject_field',
  scope_id: '',
  committee_size: 5,
  election_interval_days: 365,
})
const creating = ref(false)
const fields = ref<SubjectFieldInfo[]>([])
const subjects = ref<SubjectInfo[]>([])

const scopeOptions = computed(() => {
  if (createForm.value.scope_type === 'subject_field') {
    return fields.value.map(f => ({ id: f.id, name: f.name }))
  }
  return subjects.value.map(s => ({ id: s.id, name: `${s.subject_field_name ? s.subject_field_name + ' / ' : ''}${s.name}` }))
})

onMounted(async () => {
  try {
    const [d, f, s] = await Promise.all([
      invoke<DaoInfo[]>('list_daos'),
      invoke<SubjectFieldInfo[]>('list_subject_fields').catch(() => []),
      invoke<SubjectInfo[]>('list_subjects', {}).catch(() => []),
    ])
    daos.value = d
    fields.value = f
    subjects.value = s
  } catch (e) {
    console.error('Failed to load DAOs:', e)
  } finally {
    loading.value = false
  }
})

function openCreateDialog() {
  createForm.value = {
    name: '',
    description: '',
    scope_type: 'subject_field',
    scope_id: '',
    committee_size: 5,
    election_interval_days: 365,
  }
  error.value = ''
  showCreateDao.value = true
}

async function createDao() {
  if (!createForm.value.name.trim() || !createForm.value.scope_id) {
    error.value = 'Name and scope are required.'
    return
  }
  creating.value = true
  error.value = ''
  try {
    const dao = await invoke<DaoInfo>('create_dao', { params: createForm.value })
    daos.value.unshift(dao)
    showCreateDao.value = false
  } catch (e) {
    error.value = String(e)
  } finally {
    creating.value = false
  }
}
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
      <AppButton v-if="stakeAddress" @click="openCreateDialog">
        + Create DAO
      </AppButton>
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
      :description="search ? 'Try a different search term.' : 'Create a DAO to govern a subject field or subject.'"
    />

    <div v-else class="grid grid-cols-1 sm:grid-cols-2 gap-4">
      <router-link
        v-for="dao in filtered"
        :key="dao.id"
        :to="`/governance/${dao.id}`"
        class="card-interactive p-4"
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

    <!-- Create DAO Modal -->
    <AppModal :open="showCreateDao" title="Create DAO" @close="showCreateDao = false">
      <div class="space-y-4">
        <p v-if="error" class="text-sm text-[rgb(var(--color-error))]">{{ error }}</p>

        <div>
          <label class="block text-xs font-medium mb-1">Name</label>
          <AppInput v-model="createForm.name" placeholder="e.g. Computer Science DAO" />
        </div>

        <div>
          <label class="block text-xs font-medium mb-1">Description</label>
          <input v-model="createForm.description" class="input w-full" placeholder="Purpose of this DAO..." />
        </div>

        <div>
          <label class="block text-xs font-medium mb-1">Scope Type</label>
          <select v-model="createForm.scope_type" class="input w-full" @change="createForm.scope_id = ''">
            <option value="subject_field">Subject Field</option>
            <option value="subject">Subject</option>
          </select>
        </div>

        <div>
          <label class="block text-xs font-medium mb-1">
            {{ createForm.scope_type === 'subject_field' ? 'Subject Field' : 'Subject' }}
          </label>
          <select v-model="createForm.scope_id" class="input w-full">
            <option value="" disabled>Select...</option>
            <option v-for="opt in scopeOptions" :key="opt.id" :value="opt.id">{{ opt.name }}</option>
          </select>
        </div>

        <div class="grid grid-cols-2 gap-3">
          <div>
            <label class="block text-xs font-medium mb-1">Committee Size</label>
            <input v-model.number="createForm.committee_size" type="number" min="1" max="21" class="input w-full" />
          </div>
          <div>
            <label class="block text-xs font-medium mb-1">Election Interval (days)</label>
            <input v-model.number="createForm.election_interval_days" type="number" min="30" max="730" class="input w-full" />
          </div>
        </div>

        <div class="flex justify-end gap-2 pt-2">
          <AppButton variant="ghost" @click="showCreateDao = false">Cancel</AppButton>
          <AppButton :loading="creating" @click="createDao">Create DAO</AppButton>
        </div>
      </div>
    </AppModal>
  </div>
</template>
