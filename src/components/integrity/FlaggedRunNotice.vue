<script setup lang="ts">
/**
 * Telling somebody that an assessment of theirs has been flagged.
 *
 * A service can decide it could not explain a session, open a review, and reach
 * a conclusion, without the person it is about being told any of it. The right
 * to contest a flag is worth exactly what the chance of hearing about the flag
 * is worth, so this is the part that makes the rest of it mean something.
 *
 * How it says it is the design:
 *
 * - Once. `told_at` is written when this is shown, so a flag is raised with the
 *   learner a single time rather than every unlock. Repeating an accusation is
 *   its own kind of pressure.
 * - Not as an alarm. No red, no warning icon, no interruption of what they were
 *   doing. Sessions get flagged for ordinary reasons, and a notice that reads
 *   like an emergency teaches people to expect the worst of a false positive.
 * - With the two true things next to it: that this is not a finding, and that
 *   answering it is optional and cannot be held against them.
 * - "Not now" and "See what they have" are equal. There is no primary action
 *   here, because the application has no opinion about whether somebody should
 *   defend themselves.
 */

import { ref, computed, onMounted } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { useRouter } from 'vue-router'
import { useI18n } from 'vue-i18n'

import { AppButton } from '@/components/ui'

interface FlagNotice {
  directoryUrl: string
  runId: string
  organisation: string
  role: string
}

const { t } = useI18n()
const router = useRouter()

const notices = ref<FlagNotice[]>([])
const open = ref(false)

const first = computed(() => notices.value[0])

onMounted(async () => {
  try {
    // Reaches the network, so failure is ordinary: a service that is down, or
    // a person with no directories configured. Neither is worth saying here.
    notices.value = await invoke<FlagNotice[]>('holder_unseen_flags')
    open.value = notices.value.length > 0
  } catch {
    /* nothing to tell them */
  }
})

/**
 * Close it, and record that it was shown.
 *
 * Recorded on either button, because both mean the same thing about what the
 * learner now knows. Nothing here records agreement — see `told_at`.
 */
async function dismiss(thenOpenSettings: boolean) {
  const shown = notices.value
  open.value = false
  try {
    await invoke('holder_mark_flags_seen', { runs: shown })
  } catch {
    /* it will be offered again next unlock, which is the safe direction */
  }
  if (thenOpenSettings) router.push('/settings/integrity')
}
</script>

<template>
  <div v-if="open && first" class="fn" role="status">
    <div class="fn-body">
      <p class="fn-title">{{ t('sentinel.evidence.noticeTitle') }}</p>

      <p class="fn-what">
        <template v-if="notices.length === 1">
          {{ t('sentinel.evidence.noticeOne', { org: first.organisation, role: first.role }) }}
        </template>
        <template v-else>
          {{ t('sentinel.evidence.noticeMany', { n: notices.length }) }}
        </template>
      </p>

      <p class="fn-muted">{{ t('sentinel.evidence.noticeNotAccusation') }}</p>
      <p class="fn-muted">{{ t('sentinel.evidence.noticeWhatYouCanDo') }}</p>
    </div>

    <div class="fn-actions">
      <AppButton variant="ghost" @click="dismiss(false)">
        {{ t('sentinel.evidence.noticeLater') }}
      </AppButton>
      <AppButton variant="ghost" @click="dismiss(true)">
        {{ t('sentinel.evidence.noticeOpen') }}
      </AppButton>
    </div>
  </div>
</template>

<style scoped>
/*
 * Deliberately not the danger palette. This is information about something
 * that happened, not a problem the person has caused, and colouring it as a
 * warning would make every false positive feel like a verdict.
 */
.fn {
  position: fixed;
  right: 1rem;
  bottom: 1rem;
  z-index: 60;
  max-width: 26rem;
  display: flex;
  flex-direction: column;
  gap: 0.75rem;
  padding: 1rem 1.1rem;
  border: 1px solid var(--color-border, rgba(128, 128, 128, 0.3));
  border-radius: 12px;
  background: var(--color-surface, #fff);
  color: inherit;
  box-shadow: 0 10px 30px rgba(0, 0, 0, 0.18);
}
.fn-body {
  display: flex;
  flex-direction: column;
  gap: 0.45rem;
}
.fn-title {
  margin: 0;
  font-weight: 600;
}
.fn-what {
  margin: 0;
}
.fn-muted {
  margin: 0;
  font-size: 0.85rem;
  opacity: 0.78;
}
.fn-actions {
  display: flex;
  gap: 0.5rem;
  justify-content: flex-end;
}

/* On a phone it spans the width rather than floating in a corner. */
@media (max-width: 30rem) {
  .fn {
    left: 1rem;
    right: 1rem;
    max-width: none;
  }
}
</style>
