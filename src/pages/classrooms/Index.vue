<template>
  <div>
    <!-- Header -->
    <div class="border-b border-border px-6 py-4 flex items-center justify-between">
      <div>
        <h1 class="text-xl font-bold text-foreground">Classrooms</h1>
        <p class="text-sm text-muted-foreground mt-0.5">Group spaces for learning together</p>
      </div>
      <AppButton variant="primary" size="sm" @click="showCreate = true">
        + Create Classroom
      </AppButton>
    </div>

    <!-- Loading -->
    <div v-if="loading" class="flex items-center justify-center py-20">
      <div class="text-muted-foreground">Loading classrooms...</div>
    </div>

    <!-- Empty state -->
    <div
      v-else-if="classrooms.length === 0"
      class="flex flex-col items-center justify-center py-20 text-center px-4"
    >
      <div class="w-16 h-16 rounded-2xl bg-primary/10 flex items-center justify-center text-3xl mb-4">
        🏫
      </div>
      <h2 class="text-lg font-semibold text-foreground mb-2">No classrooms yet</h2>
      <p class="text-muted-foreground text-sm max-w-sm mb-6">
        Create a classroom to start a group learning space, or join one by requesting access.
      </p>
      <AppButton variant="primary" @click="showCreate = true">
        Create your first classroom
      </AppButton>
    </div>

    <!-- Classroom grid -->
    <div v-else class="p-6 grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-6">
      <RouterLink
        v-for="classroom in classrooms"
        :key="classroom.id"
        :to="{ name: 'classroom', params: { id: classroom.id } }"
        class="card card-interactive block p-4"
      >
        <!-- Icon + Name -->
        <div class="flex items-center gap-3 mb-3">
          <div
            class="w-10 h-10 rounded-lg bg-primary/10 flex items-center justify-center text-xl flex-shrink-0"
          >
            {{ classroom.icon_emoji ?? '🏫' }}
          </div>
          <div class="min-w-0">
            <h3 class="font-semibold text-foreground truncate">{{ classroom.name }}</h3>
            <div class="flex items-center gap-2 text-xs text-muted-foreground mt-0.5">
              <span>{{ classroom.member_count ?? 0 }} member{{ classroom.member_count !== 1 ? 's' : '' }}</span>
              <span class="opacity-40">·</span>
              <span class="capitalize">{{ classroom.my_role }}</span>
            </div>
          </div>
        </div>

        <!-- Description -->
        <p v-if="classroom.description" class="text-sm text-muted-foreground line-clamp-2">
          {{ classroom.description }}
        </p>
      </RouterLink>
    </div>

    <!-- Error -->
    <div v-if="lastError" class="mx-6 mt-4 p-3 bg-destructive/10 border border-destructive/30 rounded-lg text-sm text-destructive">
      {{ lastError }}
    </div>

    <!-- Create classroom modal -->
    <AppModal :open="showCreate" @close="showCreate = false" title="Create Classroom">
      <div class="space-y-4">
        <div>
          <label class="block text-sm text-muted-foreground mb-1">Classroom name *</label>
          <input
            v-model="form.name"
            type="text"
            placeholder="e.g. Advanced Mathematics"
            class="w-full px-3 py-2 bg-muted border border-border rounded-lg text-foreground placeholder-muted-foreground text-sm focus:outline-none focus:border-primary"
          />
        </div>

        <div>
          <label class="block text-sm text-muted-foreground mb-1">Description</label>
          <textarea
            v-model="form.description"
            placeholder="What is this classroom about?"
            rows="3"
            class="w-full px-3 py-2 bg-muted border border-border rounded-lg text-foreground placeholder-muted-foreground text-sm focus:outline-none focus:border-primary resize-none"
          />
        </div>

        <div>
          <label class="block text-sm text-muted-foreground mb-1">Icon emoji</label>
          <input
            v-model="form.iconEmoji"
            type="text"
            placeholder="🏫"
            maxlength="4"
            class="w-24 px-3 py-2 bg-muted border border-border rounded-lg text-foreground placeholder-muted-foreground text-sm focus:outline-none focus:border-primary"
          />
        </div>
      </div>

      <template #footer>
        <div class="flex gap-3">
          <AppButton variant="secondary" class="flex-1" @click="showCreate = false">
            Cancel
          </AppButton>
          <AppButton
            variant="primary"
            class="flex-1"
            :disabled="!form.name.trim() || creating"
            :loading="creating"
            @click="handleCreate"
          >
            Create
          </AppButton>
        </div>
      </template>
    </AppModal>
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { useRouter } from 'vue-router'
import { useClassroom } from '@/composables/useClassroom'
import AppButton from '@/components/ui/AppButton.vue'
import AppModal from '@/components/ui/AppModal.vue'

const router = useRouter()
const { classrooms, loading, lastError, loadClassrooms, createClassroom } = useClassroom()

const showCreate = ref(false)
const creating = ref(false)
const form = ref({ name: '', description: '', iconEmoji: '' })

onMounted(() => {
  loadClassrooms()
})

async function handleCreate() {
  if (!form.value.name.trim()) return
  creating.value = true
  try {
    const classroom = await createClassroom(
      form.value.name.trim(),
      form.value.description.trim() || undefined,
      form.value.iconEmoji.trim() || undefined,
    )
    if (classroom) {
      showCreate.value = false
      form.value = { name: '', description: '', iconEmoji: '' }
      router.push({ name: 'classroom', params: { id: classroom.id } })
    }
  } finally {
    creating.value = false
  }
}
</script>
