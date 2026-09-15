<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { useLocalApi } from '@/composables/useLocalApi'
import { AppButton, AppInput, AppModal, AppTextarea } from '@/components/ui'
import type {
  StudioConnection,
  StudioDocument,
  TutorPolicy,
  TutorReply,
  TutorThread,
} from '@/types'

const props = defineProps<{ courseId: string; elementId: string }>()
const { invoke } = useLocalApi()
const { t } = useI18n()

const policy = ref<TutorPolicy | null>(null)
const connections = ref<StudioDocument<StudioConnection>[]>([])
const selectedConnection = ref('')
const thread = ref<TutorThread | null>(null)
const open = ref(false)
const loading = ref(true)
const sending = ref(false)
const error = ref('')
const question = ref('')
const addingConnection = ref(false)
const savingConnection = ref(false)
const apiKey = ref('')
const connection = ref<StudioDocument<StudioConnection> | null>(null)

const availableConnections = computed(() => connections.value.filter(item =>
  item.value.enabled && item.value.capability === 'text' && item.value.location !== 'external',
))
const selected = computed(() => availableConnections.value.find(item => item.id === selectedConnection.value))
const providerLocation = computed(() => selected.value?.value.location ?? 'local')

async function load() {
  loading.value = true
  error.value = ''
  try {
    const [nextPolicy, nextConnections] = await Promise.all([
      invoke<TutorPolicy>('studio_get_tutor_policy', { courseId: props.courseId }),
      invoke<StudioDocument<StudioConnection>[]>('studio_list_connections'),
    ])
    policy.value = nextPolicy
    connections.value = nextConnections
    selectedConnection.value = availableConnections.value[0]?.id ?? ''
    if (selectedConnection.value && nextPolicy.enabled) await loadThread()
  } catch (value) {
    error.value = String(value)
  } finally {
    loading.value = false
  }
}

async function loadThread() {
  if (!selectedConnection.value) {
    thread.value = null
    return
  }
  error.value = ''
  try {
    const reply = await invoke<TutorReply>('studio_get_tutor_thread', {
      courseId: props.courseId,
      elementId: props.elementId,
      connectionId: selectedConnection.value,
    })
    thread.value = reply.thread
  } catch (value) {
    thread.value = null
    error.value = String(value)
  }
}

async function ask() {
  if (!question.value.trim() || !selectedConnection.value) return
  sending.value = true
  error.value = ''
  try {
    const reply = await invoke<TutorReply>('studio_ask_tutor', {
      courseId: props.courseId,
      elementId: props.elementId,
      connectionId: selectedConnection.value,
      question: question.value.trim(),
    })
    thread.value = reply.thread
    question.value = ''
  } catch (value) {
    error.value = String(value)
  } finally {
    sending.value = false
  }
}

async function clearThread() {
  await invoke('studio_clear_tutor_thread', {
    courseId: props.courseId,
    elementId: props.elementId,
  })
  thread.value = thread.value ? { ...thread.value, messages: [] } : null
}

function startConnection() {
  const id = crypto.randomUUID()
  connection.value = {
    id,
    revision: 0,
    value: {
      id,
      name: '',
      endpoint: 'http://localhost:11434/v1',
      model: '',
      location: 'local',
      capability: 'text',
      enabled: true,
      has_key: false,
    },
  }
  apiKey.value = ''
  addingConnection.value = true
}

function changeLocation() {
  if (!connection.value) return
  connection.value.value.endpoint = connection.value.value.location === 'local'
    ? 'http://localhost:11434/v1'
    : 'https://api.openai.com/v1'
}

async function saveConnection() {
  if (!connection.value) return
  savingConnection.value = true
  error.value = ''
  try {
    const saved = await invoke<StudioDocument<StudioConnection>>('studio_save_connection', {
      document: connection.value,
      apiKey: apiKey.value || null,
    })
    connections.value = [saved, ...connections.value]
    selectedConnection.value = saved.id
    addingConnection.value = false
    connection.value = null
    apiKey.value = ''
    await loadThread()
  } catch (value) {
    error.value = String(value)
  } finally {
    savingConnection.value = false
  }
}

onMounted(load)
</script>

<template>
  <template v-if="policy?.enabled">
    <AppButton
      type="button"
      class="fixed bottom-20 end-4 z-40 shadow-lg md:bottom-20 md:end-6"
      :aria-label="t('learn.player.tutorOpen')"
      @click="open = true"
    >
      <svg class="me-2 h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
        <path stroke-linecap="round" stroke-linejoin="round" d="M8 10h.01M12 10h.01M16 10h.01M21 12c0 4.418-4.03 8-9 8a10.4 10.4 0 0 1-4-.78L3 20l1.2-3.2A7.4 7.4 0 0 1 3 12c0-4.418 4.03-8 9-8s9 3.582 9 8Z" />
      </svg>
      {{ t('learn.player.tutor') }}
    </AppButton>

    <AppModal :open="open" :title="t('learn.player.tutorTitle')" max-width="44rem" @close="open = false">
      <div class="space-y-4">
        <p class="text-sm text-muted-foreground">{{ t('learn.player.tutorIntro') }}</p>
        <div v-if="loading" class="py-8 text-center text-sm text-muted-foreground">{{ t('learn.player.tutorLoading') }}</div>
        <template v-else>
          <div class="rounded-lg border border-border bg-muted/20 p-3">
            <div class="flex flex-wrap items-end gap-3">
              <label class="min-w-0 flex-1 text-xs text-muted-foreground">
                <span class="mb-1.5 block">{{ t('learn.player.tutorModel') }}</span>
                <select
                  v-model="selectedConnection"
                  class="w-full rounded-lg border border-border bg-background px-3 py-2 text-sm text-foreground"
                  @change="loadThread"
                >
                  <option value="">{{ t('learn.player.tutorChooseModel') }}</option>
                  <option v-for="item in availableConnections" :key="item.id" :value="item.id">
                    {{ item.value.name }} · {{ item.value.location }}
                  </option>
                </select>
              </label>
              <AppButton type="button" variant="secondary" size="sm" @click="startConnection">
                {{ t('learn.player.tutorAddModel') }}
              </AppButton>
            </div>
            <p v-if="selected" class="mt-2 text-xs text-muted-foreground">
              {{ t(providerLocation === 'local' ? 'learn.player.tutorLocalDisclosure' : 'learn.player.tutorCloudDisclosure', { provider: selected.value.name }) }}
            </p>
          </div>

          <div class="max-h-[42vh] space-y-3 overflow-y-auto rounded-lg border border-border p-3" aria-live="polite">
            <p v-if="!thread?.messages.length" class="py-8 text-center text-sm text-muted-foreground">
              {{ t('learn.player.tutorEmpty') }}
            </p>
            <div
              v-for="(message, index) in thread?.messages"
              :key="`${message.created_at}:${index}`"
              class="max-w-[88%] rounded-xl px-3 py-2 text-sm whitespace-pre-wrap"
              :class="message.role === 'learner' ? 'ms-auto bg-primary text-primary-foreground' : 'bg-muted text-foreground'"
            >
              {{ message.text }}
            </div>
          </div>

          <p v-if="error" role="alert" class="text-sm text-error">{{ error }}</p>
          <form class="space-y-3" @submit.prevent="ask">
            <AppTextarea
              v-model="question"
              :label="t('learn.player.tutorQuestion')"
              :placeholder="t('learn.player.tutorQuestionPlaceholder')"
              :maxlength="4000"
              :rows="3"
              :disabled="!selectedConnection || sending"
            />
            <div class="flex items-center justify-between gap-3">
              <AppButton type="button" variant="ghost" size="sm" :disabled="!thread?.messages.length" @click="clearThread">
                {{ t('learn.player.tutorClear') }}
              </AppButton>
              <AppButton type="submit" size="sm" :loading="sending" :disabled="!selectedConnection || !question.trim()">
                {{ t('learn.player.tutorSend') }}
              </AppButton>
            </div>
          </form>
        </template>
      </div>
    </AppModal>

    <AppModal :open="addingConnection" :title="t('learn.player.tutorAddModel')" @close="addingConnection = false">
      <form v-if="connection" class="space-y-4" @submit.prevent="saveConnection">
        <AppInput v-model="connection.value.name" :label="t('learn.player.tutorModelName')" required :maxlength="200" />
        <label class="block text-xs text-muted-foreground">
          <span class="mb-1.5 block">{{ t('learn.player.tutorLocation') }}</span>
          <select v-model="connection.value.location" class="w-full rounded-lg border border-border bg-background px-3 py-2.5 text-sm text-foreground" @change="changeLocation">
            <option value="local">{{ t('learn.player.tutorLocal') }}</option>
            <option value="cloud">{{ t('learn.player.tutorCloud') }}</option>
          </select>
        </label>
        <AppInput v-model="connection.value.endpoint" :label="t('learn.player.tutorEndpoint')" required />
        <AppInput v-model="connection.value.model" :label="t('learn.player.tutorModelId')" required :maxlength="200" />
        <AppInput v-if="connection.value.location === 'cloud'" v-model="apiKey" type="password" autocomplete="new-password" :label="t('learn.player.tutorApiKey')" required />
        <p class="text-xs text-muted-foreground">{{ t('learn.player.tutorKeyPrivacy') }}</p>
        <div class="flex justify-end gap-2">
          <AppButton type="button" variant="ghost" @click="addingConnection = false">{{ t('common.actions.cancel') }}</AppButton>
          <AppButton type="submit" :loading="savingConnection">{{ t('common.actions.save') }}</AppButton>
        </div>
      </form>
    </AppModal>
  </template>
</template>
