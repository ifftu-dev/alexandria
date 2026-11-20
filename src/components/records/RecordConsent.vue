<script setup lang="ts">
/**
 * The consent screen for sending learning records to someone.
 *
 * It shows the three things the learner is agreeing to before anything is
 * sent: the scope (which courses, and whether individual responses go too),
 * the identifier the records will carry at this recipient, and what removing
 * it later can and cannot undo. Completion-level records are always in a
 * scope; individual responses are a separate choice, off by default. The
 * learner may approve fewer courses than were offered.
 *
 * Shared by every way of sending: record destinations use it as is; a record
 * share fills the `undo` slot with what revoking a share does, and may offer
 * the account the organisation proposed as a third identifier.
 */
import { computed, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { AppButton } from '@/components/ui'
import type { CourseChoice, IdentifierChoice, RecordAccount, RecordConsentDecision } from '@/types'

const props = withDefaults(
  defineProps<{
    /** What the learner knows the recipient as. */
    recipient: string
    /** Courses the scope may name. */
    courses: CourseChoice[]
    /** Courses ticked when the screen opens (a request's scope). */
    initialCourseIds?: string[]
    /** Whether individual responses may be chosen at all. */
    interactionsOffered?: boolean
    /** The account name records carry with an identifier made for this recipient. */
    pairwiseActor: string
    /** The account name records carry if the learner uses their own DID. */
    realDidActor: string
    /** An account the recipient proposed (a record share request), offered as a choice. */
    proposedAccount?: RecordAccount | null
    /** The recipient is on this device or a private network. */
    privateAddress?: boolean
    busy?: boolean
    error?: string
  }>(),
  {
    initialCourseIds: () => [],
    interactionsOffered: true,
    proposedAccount: null,
    privateAddress: false,
    busy: false,
    error: '',
  },
)

const emit = defineEmits<{ approve: [decision: RecordConsentDecision]; cancel: [] }>()

const { t } = useI18n()

const selected = ref<string[]>(props.courses.filter((c) => props.initialCourseIds.includes(c.id)).map((c) => c.id))
const includeInteractions = ref(false)
const identifier = ref<IdentifierChoice>('pairwise')
const confirmPrivate = ref(false)

const actor = computed(() => {
  if (identifier.value === 'real_did') return props.realDidActor
  if (identifier.value === 'proposed' && props.proposedAccount) return props.proposedAccount.name
  return props.pairwiseActor
})
const canApprove = computed(
  () => selected.value.length > 0 && (!props.privateAddress || confirmPrivate.value) && !props.busy,
)

function toggleCourse(id: string, on: boolean) {
  selected.value = on ? [...new Set([...selected.value, id])] : selected.value.filter((c) => c !== id)
}

function approve() {
  if (!canApprove.value) return
  emit('approve', {
    course_ids: props.courses.filter((c) => selected.value.includes(c.id)).map((c) => c.id),
    include_interactions: props.interactionsOffered && includeInteractions.value,
    identifier: identifier.value,
    confirm_private: props.privateAddress && confirmPrivate.value,
  })
}
</script>

<template>
  <section class="space-y-4 rounded-xl border border-border bg-card p-5" data-testid="record-consent">
    <header>
      <h3 class="text-base font-semibold text-foreground">
        {{ t('organisations.records.consent.title', { recipient }) }}
      </h3>
      <p class="mt-1 break-all text-xs text-muted-foreground">{{ recipient }}</p>
      <p class="mt-1 text-sm text-muted-foreground">{{ t('organisations.records.consent.nothingYet') }}</p>
    </header>

    <fieldset class="space-y-2">
      <legend class="text-sm font-medium text-foreground">{{ t('organisations.records.consent.scopeHeading') }}</legend>
      <p class="text-xs text-muted-foreground">{{ t('organisations.records.consent.coursesLabel') }}</p>
      <p v-if="!courses.length" class="text-sm text-muted-foreground">{{ t('organisations.records.consent.noCourses') }}</p>
      <label v-for="c in courses" :key="c.id" class="flex items-center gap-2 text-sm text-foreground">
        <input
          type="checkbox"
          :checked="selected.includes(c.id)"
          :data-testid="`consent-course-${c.id}`"
          @change="toggleCourse(c.id, ($event.target as HTMLInputElement).checked)"
        />
        <span>{{ c.title }}</span>
      </label>
      <p class="text-sm text-foreground">{{ t('organisations.records.consent.completionAlways') }}</p>
      <label v-if="interactionsOffered" class="flex items-start gap-2 text-sm text-foreground">
        <input v-model="includeInteractions" type="checkbox" class="mt-1" data-testid="consent-interactions" />
        <span>
          <span class="block">{{ t('organisations.records.consent.interactions') }}</span>
          <span class="block text-xs text-muted-foreground">{{ t('organisations.records.consent.interactionsNote') }}</span>
        </span>
      </label>
    </fieldset>

    <fieldset class="space-y-2">
      <legend class="text-sm font-medium text-foreground">{{ t('organisations.records.consent.identifierHeading') }}</legend>
      <label class="flex items-start gap-2 text-sm text-foreground">
        <input v-model="identifier" type="radio" value="pairwise" class="mt-1" data-testid="consent-identifier-pairwise" />
        <span>
          <span class="block">{{ t('organisations.records.consent.pairwise') }}</span>
          <span class="block text-xs text-muted-foreground">{{ t('organisations.records.consent.pairwiseNote') }}</span>
        </span>
      </label>
      <label class="flex items-start gap-2 text-sm text-foreground">
        <input v-model="identifier" type="radio" value="real_did" class="mt-1" data-testid="consent-identifier-real_did" />
        <span>
          <span class="block">{{ t('organisations.records.consent.realDid') }}</span>
          <span v-if="identifier === 'real_did'" class="block text-xs text-muted-foreground">
            {{ t('organisations.records.consent.realDidNote') }}
          </span>
        </span>
      </label>
      <label v-if="proposedAccount" class="flex items-start gap-2 text-sm text-foreground">
        <input v-model="identifier" type="radio" value="proposed" class="mt-1" data-testid="consent-identifier-proposed" />
        <span>
          <span class="block">{{ t('organisations.records.consent.proposed', { recipient }) }}</span>
          <span class="block text-xs text-muted-foreground">
            {{ t('organisations.records.consent.proposedNote', { recipient, homePage: proposedAccount.home_page }) }}
          </span>
        </span>
      </label>
      <code class="block break-all rounded-md bg-muted px-2 py-1 text-xs text-foreground" data-testid="consent-actor">{{ actor }}</code>
    </fieldset>

    <div class="space-y-1">
      <h4 class="text-sm font-medium text-foreground">{{ t('organisations.records.consent.undoHeading') }}</h4>
      <slot name="undo">
        <p class="text-sm text-foreground">{{ t('organisations.records.consent.undoStops') }}</p>
        <p class="text-sm text-foreground">{{ t('organisations.records.consent.undoCannot') }}</p>
      </slot>
    </div>

    <div v-if="privateAddress" class="space-y-1 rounded-lg border border-border p-3">
      <p class="text-sm text-foreground">{{ t('organisations.records.consent.privateAddress') }}</p>
      <label class="flex items-center gap-2 text-sm text-foreground">
        <input v-model="confirmPrivate" type="checkbox" data-testid="consent-confirm-private" />
        <span>{{ t('organisations.records.consent.confirmPrivate') }}</span>
      </label>
    </div>

    <p v-if="error" role="alert" class="text-sm text-destructive">{{ error }}</p>

    <div class="flex justify-end gap-2">
      <AppButton variant="ghost" size="sm" data-testid="consent-cancel" @click="emit('cancel')">
        {{ t('organisations.records.cancel') }}
      </AppButton>
      <AppButton size="sm" :disabled="!canApprove" :loading="busy" data-testid="consent-approve" @click="approve">
        {{ t('organisations.records.consent.approve') }}
      </AppButton>
    </div>
  </section>
</template>
