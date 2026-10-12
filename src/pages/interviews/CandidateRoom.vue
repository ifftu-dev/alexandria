<script setup lang="ts">
// The candidate's door into an interview held in Alexandria. The panel opens
// the room; until then this waits, asking Cloud (with the learner's own proof)
// every few seconds. Joining hands off to the ordinary live room.
import { computed, onMounted, onUnmounted, ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { usePanel } from '@/composables/usePanel'
import { useTutoringRoom } from '@/composables/useTutoringRoom'
import { AppAlert, AppButton } from '@/components/ui'

const POLL_MS = 5000

const route = useRoute()
const router = useRouter()
const panelApi = usePanel()
const tutoring = useTutoringRoom()

const interviewId = computed(() => String(route.params.id))
const cloud = computed(() => String(route.query.cloud ?? ''))
const ticket = ref<string | null>(null)
const error = ref<string | null>(null)
const joining = ref(false)
let timer: number | null = null

async function check() {
  try {
    const room = await panelApi.candidateRoom(cloud.value, interviewId.value)
    error.value = null
    ticket.value = room.ticket
    if (room.ticket && timer) {
      window.clearInterval(timer)
      timer = null
    }
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause)
  }
}

async function join() {
  if (!ticket.value) return
  joining.value = true
  try {
    const room = await tutoring.joinRoom(ticket.value)
    await router.push(`/tutoring/${room.id}`)
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause)
  } finally {
    joining.value = false
  }
}

onMounted(async () => {
  await check()
  if (!ticket.value) timer = window.setInterval(() => void check(), POLL_MS)
})

onUnmounted(() => {
  if (timer) window.clearInterval(timer)
})
</script>

<template>
  <div class="mx-auto max-w-xl space-y-5 py-10">
    <div class="card p-6">
      <p class="text-xs font-semibold uppercase tracking-[0.14em] text-primary">{{ $t('tutoring.interviewRoom.eyebrow') }}</p>
      <h1 class="mt-2 text-xl font-semibold text-foreground">
        {{ ticket ? $t('tutoring.interviewRoom.ready') : $t('tutoring.interviewRoom.waiting') }}
      </h1>
      <p class="mt-2 text-sm leading-relaxed text-muted-foreground">{{ $t('tutoring.interviewRoom.explain') }}</p>
      <ul class="mt-4 space-y-1.5 text-sm text-muted-foreground">
        <li>• {{ $t('tutoring.interviewRoom.noRecording') }}</li>
        <li>• {{ $t('tutoring.interviewRoom.captions') }}</li>
        <li>• {{ $t('tutoring.interviewRoom.leave') }}</li>
      </ul>
      <AppAlert v-if="error" variant="error" class="mt-4">{{ error }}</AppAlert>
      <div class="mt-6 flex items-center gap-3">
        <AppButton :disabled="!ticket" :loading="joining" data-testid="join-interview" @click="join">
          {{ $t('tutoring.interviewRoom.join') }}
        </AppButton>
        <span v-if="!ticket" class="text-xs text-muted-foreground">{{ $t('tutoring.interviewRoom.checking') }}</span>
      </div>
    </div>
  </div>
</template>
