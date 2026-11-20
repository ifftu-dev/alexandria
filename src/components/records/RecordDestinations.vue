<script setup lang="ts">
/**
 * Record destinations: learning record stores the learner names and sends
 * records to directly, with the key and secret the store issued to them.
 *
 * Adding one is two steps. The address and credentials are entered first;
 * "Continue" only looks the address up and works out the identifier the
 * store would see. The consent screen then shows scope, identifier and what
 * removal can and cannot undo, and nothing is sent until it is approved.
 *
 * Works whether or not the organisations switch is on: no hosted service is
 * involved. With `showLog` it also lists what this device has sent, for when
 * the organisations access panel (which shows the same log beside the
 * hosted entries) is not on screen.
 */
import { onMounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { useLocalApi } from '@/composables/useLocalApi'
import { AppButton, AppInput, ConfirmDialog } from '@/components/ui'
import RecordConsent from './RecordConsent.vue'
import RecordSendLog from './RecordSendLog.vue'
import { useRecordSendLog } from '@/composables/useRecordSendLog'
import type { DestinationPreview, RecordConsentDecision, RecordDestination, RemovalReport } from '@/types'

withDefaults(defineProps<{ showLog?: boolean }>(), { showLog: false })

const { invoke } = useLocalApi()
const { t, locale } = useI18n()

const destinations = ref<RecordDestination[]>([])
const error = ref('')
const notice = ref('')
const adding = ref(false)
const address = ref('')
const username = ref('')
const secret = ref('')
const preview = ref<DestinationPreview | null>(null)
const busy = ref(false)
const formError = ref('')
const removing = ref<RecordDestination | null>(null)
const removingBusy = ref(false)
const sendLog = useRecordSendLog()

function when(iso: string | null): string {
  if (!iso) return ''
  const parsed = new Date(iso)
  if (Number.isNaN(parsed.getTime())) return iso
  return new Intl.DateTimeFormat(locale.value, { dateStyle: 'medium', timeStyle: 'short' }).format(parsed)
}

async function refresh() {
  error.value = ''
  try {
    destinations.value = (await invoke<RecordDestination[] | null>('record_destinations')) ?? []
  } catch (e) {
    error.value = String(e)
  }
  // Every view of the send log shares it; what this panel sent shows in all of them.
  await sendLog.refresh()
}

function resetForm() {
  adding.value = false
  preview.value = null
  address.value = ''
  username.value = ''
  secret.value = ''
  formError.value = ''
}

/** Looks the address up and shows the consent screen. Sends nothing. */
async function next() {
  busy.value = true
  formError.value = ''
  try {
    preview.value = await invoke<DestinationPreview>('record_destination_preview', {
      address: address.value.trim(),
      username: username.value.trim(),
    })
  } catch (e) {
    formError.value = String(e)
  } finally {
    busy.value = false
  }
}

async function approve(decision: RecordConsentDecision) {
  busy.value = true
  formError.value = ''
  try {
    await invoke<RecordDestination>('record_destination_add', {
      request: {
        address: address.value.trim(),
        username: username.value.trim(),
        secret: secret.value,
        ...decision,
      },
    })
    resetForm()
    await refresh()
  } catch (e) {
    formError.value = String(e)
  } finally {
    busy.value = false
  }
}

async function confirmRemove() {
  const target = removing.value
  if (!target) return
  removingBusy.value = true
  notice.value = ''
  try {
    const report = await invoke<RemovalReport>('record_destination_remove', { id: target.id })
    notice.value = report.void_error
      ? t('organisations.records.removedVoidFailed', { error: report.void_error })
      : t('organisations.records.removed', { count: report.voided })
    await refresh()
  } catch (e) {
    error.value = String(e)
  } finally {
    removingBusy.value = false
    removing.value = null
  }
}

async function sendNow() {
  error.value = ''
  try {
    await invoke('record_destinations_send_now')
  } catch (e) {
    error.value = String(e)
  }
}

defineExpose({ refresh })
onMounted(refresh)
</script>

<template>
  <section class="space-y-4 rounded-xl border border-border bg-card p-5" data-testid="record-destinations">
    <header class="flex items-start justify-between gap-3">
      <div>
        <h2 class="text-base font-semibold text-foreground">{{ t('organisations.records.title') }}</h2>
        <p class="mt-1 text-sm text-muted-foreground">{{ t('organisations.records.note') }}</p>
      </div>
      <AppButton v-if="destinations.length" variant="outline" size="sm" class="shrink-0 whitespace-nowrap" @click="sendNow">
        {{ t('organisations.records.sendNow') }}
      </AppButton>
    </header>

    <p v-if="error" role="alert" class="text-sm text-destructive">{{ error }}</p>
    <p v-if="notice" role="status" class="text-sm text-muted-foreground">{{ notice }}</p>
    <p v-if="!destinations.length && !adding" class="text-sm text-muted-foreground">{{ t('organisations.records.empty') }}</p>

    <article
      v-for="d in destinations"
      :key="d.id"
      class="space-y-2 rounded-lg border border-border p-4"
      data-testid="record-destination"
    >
      <p class="break-all text-sm font-medium text-foreground">{{ d.endpoint }}</p>
      <p class="text-xs text-muted-foreground">
        {{ t('organisations.records.courses', { count: d.scope.course_ids.length }, d.scope.course_ids.length) }}
        <template v-if="d.scope.include_interactions"> · {{ t('organisations.records.withResponses') }}</template>
      </p>
      <p class="break-all text-xs text-muted-foreground">{{ t('organisations.records.carries', { identifier: d.actor_name }) }}</p>
      <p class="text-xs" data-testid="record-destination-status">
        <span v-if="d.status.failed" class="block text-destructive">
          {{ t('organisations.records.status.failed', { count: d.status.failed }, d.status.failed) }}
        </span>
        <span v-if="d.status.queued" class="block text-muted-foreground">
          {{ t('organisations.records.status.waiting', { count: d.status.queued }, d.status.queued) }}
        </span>
        <span v-if="d.status.last_error" class="block break-words text-destructive">
          {{ t('organisations.records.status.error', { error: d.status.last_error }) }}
        </span>
        <span v-if="d.status.next_attempt_at && (d.status.failed || d.status.queued)" class="block text-muted-foreground">
          {{ t('organisations.records.status.retry', { date: when(d.status.next_attempt_at) }) }}
        </span>
        <span v-if="!d.status.failed && !d.status.queued" class="block text-muted-foreground">
          {{ d.status.last_sent_at ? t('organisations.records.status.lastSent', { date: when(d.status.last_sent_at) }) : t('organisations.records.status.upToDate') }}
        </span>
      </p>
      <div class="flex justify-end">
        <AppButton size="sm" variant="danger" :disabled="removingBusy" @click="removing = d">
          {{ t('organisations.records.remove') }}
        </AppButton>
      </div>
    </article>

    <AppButton v-if="!adding" size="sm" variant="outline" data-testid="record-destination-add" @click="adding = true">
      {{ t('organisations.records.add') }}
    </AppButton>

    <form v-else-if="!preview" class="space-y-3 rounded-lg border border-border p-4" data-testid="record-destination-form" @submit.prevent="next">
      <AppInput v-model="address" :label="t('organisations.records.address')" :placeholder="t('organisations.records.addressPlaceholder')" autocomplete="off" />
      <AppInput v-model="username" :label="t('organisations.records.username')" autocomplete="off" />
      <AppInput v-model="secret" type="password" :label="t('organisations.records.secret')" autocomplete="off" />
      <p class="text-xs text-muted-foreground">{{ t('organisations.records.credentialsNote') }}</p>
      <p v-if="formError" role="alert" class="text-sm text-destructive">{{ formError }}</p>
      <div class="flex justify-end gap-2">
        <AppButton type="button" variant="ghost" size="sm" @click="resetForm">{{ t('organisations.records.cancel') }}</AppButton>
        <AppButton type="submit" size="sm" :loading="busy" :disabled="!address.trim() || !username.trim() || !secret">
          {{ t('organisations.records.continue') }}
        </AppButton>
      </div>
    </form>

    <RecordConsent
      v-else
      :recipient="preview.endpoint"
      :courses="preview.courses"
      :pairwise-actor="preview.pairwise_actor"
      :real-did-actor="preview.real_did_actor"
      :private-address="preview.needs_confirmation"
      :busy="busy"
      :error="formError"
      @approve="approve"
      @cancel="resetForm"
    />

    <RecordSendLog v-if="showLog" />

    <ConfirmDialog
      :open="removing !== null"
      :title="t('organisations.records.removeTitle', { store: removing?.endpoint ?? '' })"
      :message="t('organisations.records.removeMessage')"
      :confirm-label="t('organisations.records.remove')"
      confirm-variant="danger"
      :loading="removingBusy"
      @confirm="confirmRemove"
      @cancel="removing = null"
    />
  </section>
</template>
