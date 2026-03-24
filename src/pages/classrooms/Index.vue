<template>
  <div class="min-h-screen bg-gray-950 text-white">
    <!-- Header -->
    <div class="border-b border-gray-800 px-6 py-4 flex items-center justify-between">
      <div>
        <h1 class="text-xl font-bold text-white">Classrooms</h1>
        <p class="text-sm text-gray-400 mt-0.5">Group spaces for learning together</p>
      </div>
      <button
        @click="showCreate = true"
        class="px-4 py-2 bg-indigo-600 hover:bg-indigo-500 text-white text-sm font-medium rounded-lg transition-colors"
      >
        + Create Classroom
      </button>
    </div>

    <!-- Loading -->
    <div v-if="loading" class="flex items-center justify-center py-20">
      <div class="text-gray-400">Loading classrooms...</div>
    </div>

    <!-- Empty state -->
    <div
      v-else-if="classrooms.length === 0"
      class="flex flex-col items-center justify-center py-20 text-center"
    >
      <div class="text-5xl mb-4">🏫</div>
      <h2 class="text-lg font-semibold text-white mb-2">No classrooms yet</h2>
      <p class="text-gray-400 text-sm max-w-sm mb-6">
        Create a classroom to start a group learning space, or join one by requesting access.
      </p>
      <button
        @click="showCreate = true"
        class="px-4 py-2 bg-indigo-600 hover:bg-indigo-500 text-white text-sm font-medium rounded-lg transition-colors"
      >
        Create your first classroom
      </button>
    </div>

    <!-- Classroom grid -->
    <div v-else class="p-6 grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-4">
      <RouterLink
        v-for="classroom in classrooms"
        :key="classroom.id"
        :to="{ name: 'classroom', params: { id: classroom.id } }"
        class="block bg-gray-900 rounded-xl border border-gray-800 hover:border-indigo-500 transition-colors p-4 cursor-pointer"
      >
        <!-- Icon + Name -->
        <div class="flex items-center gap-3 mb-3">
          <div
            class="w-10 h-10 rounded-lg bg-indigo-900 flex items-center justify-center text-xl flex-shrink-0"
          >
            {{ classroom.icon_emoji ?? '🏫' }}
          </div>
          <div class="min-w-0">
            <h3 class="font-semibold text-white truncate">{{ classroom.name }}</h3>
            <div class="flex items-center gap-2 text-xs text-gray-400 mt-0.5">
              <span>{{ classroom.member_count ?? 0 }} member{{ classroom.member_count !== 1 ? 's' : '' }}</span>
              <span class="text-gray-600">·</span>
              <span class="capitalize">{{ classroom.my_role }}</span>
            </div>
          </div>
        </div>

        <!-- Description -->
        <p v-if="classroom.description" class="text-sm text-gray-400 line-clamp-2">
          {{ classroom.description }}
        </p>
      </RouterLink>
    </div>

    <!-- Error -->
    <div v-if="lastError" class="mx-6 mt-4 p-3 bg-red-900/30 border border-red-700 rounded-lg text-sm text-red-300">
      {{ lastError }}
    </div>

    <!-- Create classroom modal -->
    <Teleport to="body">
      <div
        v-if="showCreate"
        class="fixed inset-0 bg-black/60 flex items-center justify-center z-50 p-4"
        @click.self="showCreate = false"
      >
        <div class="bg-gray-900 rounded-xl border border-gray-700 w-full max-w-md p-6">
          <h2 class="text-lg font-semibold text-white mb-4">Create Classroom</h2>

          <div class="space-y-4">
            <div>
              <label class="block text-sm text-gray-400 mb-1">Classroom name *</label>
              <input
                v-model="form.name"
                type="text"
                placeholder="e.g. Advanced Mathematics"
                class="w-full px-3 py-2 bg-gray-800 border border-gray-700 rounded-lg text-white placeholder-gray-500 text-sm focus:outline-none focus:border-indigo-500"
              />
            </div>

            <div>
              <label class="block text-sm text-gray-400 mb-1">Description</label>
              <textarea
                v-model="form.description"
                placeholder="What is this classroom about?"
                rows="3"
                class="w-full px-3 py-2 bg-gray-800 border border-gray-700 rounded-lg text-white placeholder-gray-500 text-sm focus:outline-none focus:border-indigo-500 resize-none"
              />
            </div>

            <div>
              <label class="block text-sm text-gray-400 mb-1">Icon emoji</label>
              <input
                v-model="form.iconEmoji"
                type="text"
                placeholder="🏫"
                maxlength="4"
                class="w-24 px-3 py-2 bg-gray-800 border border-gray-700 rounded-lg text-white placeholder-gray-500 text-sm focus:outline-none focus:border-indigo-500"
              />
            </div>
          </div>

          <div class="flex gap-3 mt-6">
            <button
              @click="showCreate = false"
              class="flex-1 px-4 py-2 bg-gray-800 hover:bg-gray-700 text-white text-sm font-medium rounded-lg transition-colors"
            >
              Cancel
            </button>
            <button
              @click="handleCreate"
              :disabled="!form.name.trim() || creating"
              class="flex-1 px-4 py-2 bg-indigo-600 hover:bg-indigo-500 disabled:opacity-50 disabled:cursor-not-allowed text-white text-sm font-medium rounded-lg transition-colors"
            >
              {{ creating ? 'Creating...' : 'Create' }}
            </button>
          </div>
        </div>
      </div>
    </Teleport>
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { useRouter } from 'vue-router'
import { useClassroom } from '@/composables/useClassroom'

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
