<script setup lang="ts">
import { onMounted, onUnmounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { useLocalApi } from '@/composables/useLocalApi'
import { AppButton } from '@/components/ui'
import type { DirectoryInterview, DirectoryOffer, HiringRecord } from '@/types'

const { invoke } = useLocalApi()
const { t } = useI18n()
const interviews = ref<DirectoryInterview[]>([])
const offers = ref<DirectoryOffer[]>([])
const history = ref<HiringRecord[]>([])
const errors = ref<string[]>([])
const busy = ref(false)
const slot = ref<Record<string, number | ''>>({})
const notes = ref<Record<string, string>>({})
const receipts = ref<Record<string, string>>({})
let revision = 0

const modeLabel = (mode: string) => t(mode === 'call' ? 'profile.hiring.modeCall' : mode === 'in_person' ? 'profile.hiring.modeInPerson' : 'profile.hiring.modeVideo')
const when = (unix: number) => new Date(unix * 1000).toLocaleString()

async function refresh() {
  const generation = ++revision
  busy.value = true
  errors.value = []
  try {
    const [i, o, h] = await Promise.all([
      invoke<{ items: DirectoryInterview[]; problems: { directory: string; detail: string }[] }>('hiring_interviews'),
      invoke<{ items: DirectoryOffer[]; problems: { directory: string; detail: string }[] }>('hiring_offers'),
      invoke<HiringRecord[]>('hiring_history'),
    ])
    if (generation !== revision) return
    interviews.value = i.items
    offers.value = o.items
    history.value = h
    const seen = new Set<string>()
    errors.value = [...i.problems, ...o.problems]
      .map(p => `${p.directory}: ${p.detail}`)
      .filter(e => !seen.has(e) && seen.add(e))
  } catch (error) { if (generation === revision) errors.value = [String(error)] }
  finally { if (generation === revision) busy.value = false }
}

async function answerInterview(item: DirectoryInterview, decision: 'accept' | 'decline') {
  if (busy.value) return
  const chosen = slot.value[item.invite.id]
  if (decision === 'accept' && (chosen === undefined || chosen === '')) return
  busy.value = true
  errors.value = []
  try {
    const result = await invoke<{ received: boolean; status: string }>('hiring_interview_respond', {
      directoryUrl: item.directory_url, invite: item.invite, decision,
      chosenSlot: decision === 'accept' ? chosen : null, note: notes.value[item.invite.id] || null,
    })
    if (result.received) receipts.value[item.invite.id] = result.status
    await refresh()
  } catch (error) { errors.value = [String(error)] }
  finally { busy.value = false }
}

async function answerOffer(item: DirectoryOffer, decision: 'accept' | 'decline') {
  if (busy.value) return
  busy.value = true
  errors.value = []
  try {
    const result = await invoke<{ received: boolean; status: string }>('hiring_offer_respond', {
      directoryUrl: item.directory_url, offer: item.offer, decision, note: notes.value[item.offer.id] || null,
    })
    if (result.received) receipts.value[item.offer.id] = result.status
    await refresh()
  } catch (error) { errors.value = [String(error)] }
  finally { busy.value = false }
}

onMounted(refresh)
onUnmounted(() => { revision++ })
</script>

<template>
  <section class="space-y-4 rounded-xl border border-border p-5" data-testid="hiring-inbox">
    <div class="flex items-center justify-between gap-3">
      <h2 class="text-lg font-semibold">{{ t('profile.hiring.title') }}</h2>
      <AppButton variant="outline" :disabled="busy" @click="refresh">{{ t('credentials.page.refresh') }}</AppButton>
    </div>
    <p class="text-sm text-muted-foreground">{{ t('profile.hiring.note') }}</p>
    <p v-for="error in errors" :key="error" role="alert" class="text-sm text-error">{{ error }}</p>
    <p v-if="!busy && !interviews.length && !offers.length" class="text-sm">{{ t('profile.hiring.empty') }}</p>

    <template v-if="interviews.length">
      <h3 class="font-medium">{{ t('profile.hiring.interviews') }}</h3>
      <article v-for="item in interviews" :key="`${item.directory_url}:${item.invite.id}`" class="space-y-3 rounded-lg border border-border p-4" data-testid="interview">
        <p class="font-medium">{{ item.invite.organization }} · {{ item.invite.role_label }} · {{ modeLabel(item.invite.mode) }}</p>
        <p class="text-sm">{{ item.invite.message }}</p>
        <p class="text-sm text-muted-foreground">
          <span>{{ t('profile.hiring.meeting') }}: </span>
          <a v-if="item.invite.meeting_url" :href="item.invite.meeting_url" target="_blank" rel="noopener" class="text-primary underline">{{ item.invite.meeting_url }}</a>
          <span v-else>{{ t('profile.hiring.toBeArranged') }}</span>
          <span v-if="item.invite.run_id"> · {{ t('profile.hiring.followsRun') }}</span>
          · {{ t('profile.hiring.expires') }} {{ when(item.invite.expires_at) }}
        </p>
        <template v-if="!receipts[item.invite.id]">
          <label class="block text-sm">{{ t('profile.hiring.chooseTime') }}
            <select v-model="slot[item.invite.id]" class="mt-1 w-full rounded border border-border bg-background p-2" :disabled="busy" :aria-label="t('profile.hiring.chooseTime')">
              <option value="">{{ t('profile.hiring.chooseTime') }}</option>
              <option v-for="s in item.invite.proposed_slots" :key="s" :value="s">{{ when(s) }}</option>
            </select>
          </label>
          <label class="block text-sm">{{ t('profile.hiring.noteLabel') }}
            <input v-model="notes[item.invite.id]" class="mt-1 w-full rounded border border-border bg-background p-2" maxlength="2000" :disabled="busy">
          </label>
          <div class="flex gap-2">
            <AppButton :disabled="busy || slot[item.invite.id] === undefined || slot[item.invite.id] === ''" @click="answerInterview(item, 'accept')">{{ t('profile.hiring.accept') }}</AppButton>
            <AppButton variant="outline" :disabled="busy" @click="answerInterview(item, 'decline')">{{ t('profile.hiring.decline') }}</AppButton>
          </div>
        </template>
        <p v-else role="status" class="text-sm">{{ t('profile.hiring.answered') }}: {{ receipts[item.invite.id] }}</p>
      </article>
    </template>

    <template v-if="offers.length">
      <h3 class="font-medium">{{ t('profile.hiring.offers') }}</h3>
      <article v-for="item in offers" :key="`${item.directory_url}:${item.offer.id}`" class="space-y-3 rounded-lg border border-border p-4" data-testid="offer">
        <p class="font-medium">{{ item.offer.organization }} · {{ item.offer.role_label }}</p>
        <p class="whitespace-pre-wrap text-sm">{{ item.offer.terms }}</p>
        <p class="text-sm text-muted-foreground">
          <span v-if="item.offer.start_date">{{ t('profile.hiring.starts') }} {{ item.offer.start_date }} · </span>{{ t('profile.hiring.expires') }} {{ when(item.offer.expires_at) }}
        </p>
        <template v-if="!receipts[item.offer.id]">
          <label class="block text-sm">{{ t('profile.hiring.noteLabel') }}
            <input v-model="notes[item.offer.id]" class="mt-1 w-full rounded border border-border bg-background p-2" maxlength="2000" :disabled="busy">
          </label>
          <div class="flex gap-2">
            <AppButton :disabled="busy" @click="answerOffer(item, 'accept')">{{ t('profile.hiring.acceptOffer') }}</AppButton>
            <AppButton variant="outline" :disabled="busy" @click="answerOffer(item, 'decline')">{{ t('profile.hiring.declineOffer') }}</AppButton>
          </div>
        </template>
        <p v-else role="status" class="text-sm">{{ t('profile.hiring.answered') }}: {{ receipts[item.offer.id] }}</p>
      </article>
    </template>

    <details v-if="history.length" data-testid="history">
      <summary class="cursor-pointer text-sm font-medium">{{ t('profile.hiring.history') }} ({{ history.length }})</summary>
      <ul class="mt-2 space-y-2 text-sm">
        <li v-for="record in history" :key="`${record.kind}:${record.id}`" class="rounded border border-border p-3">
          <span class="font-medium">{{ record.organization }} · {{ record.role_label }}</span>
          · {{ record.kind === 'interview' ? t('profile.hiring.interviews') : t('profile.hiring.offers') }}
          · {{ record.decision === 'accept' ? t('profile.hiring.accepted') : t('profile.hiring.declined') }}
          <span v-if="record.chosen_slot"> · {{ when(record.chosen_slot) }}</span>
          <a v-if="record.meeting_url && record.decision === 'accept'" :href="record.meeting_url" target="_blank" rel="noopener" class="ms-1 text-primary underline">{{ t('profile.hiring.meeting') }}</a>
          <span class="text-muted-foreground"> · {{ new Date(record.responded_at).toLocaleString() }}</span>
        </li>
      </ul>
    </details>
  </section>
</template>
