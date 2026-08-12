<script setup lang="ts">
/**
 * Asks a flagged learner whether to keep the evidence behind the flag.
 *
 * This is the only surface that can cause Sentinel to persist behavioural
 * capture or camera frames. Nothing is written until `sentinel_evidence_decide`
 * is called with `granted: true` from here — see `sentinel::evidence` for why
 * the ordering matters.
 *
 * Three things the wording is deliberate about, because this is a consent
 * surface and not a notification:
 *
 * - A flag is stated as something the system could not explain, not as an
 *   accusation. False positives are common enough that leading with "you have
 *   been flagged for cheating" would be both frightening and often wrong.
 * - Declining is offered as a real option with equal weight, not a greyed-out
 *   escape from a primary call to action. A refusal that costs something is not
 *   a refusal.
 * - The frames are viewable *before* the decision. Consent that cannot see what
 *   it authorises is not informed — and the frame is frequently what proves the
 *   flag wrong.
 */

import { ref, computed, watch } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { useI18n } from 'vue-i18n'

import { AppButton, AppModal } from '@/components/ui'

interface EvidencePreview {
  snapshotId: string
  kind: string
  capturedAt: string
  dataUrl: string | null
}

interface EvidenceSummary {
  sessionId: string
  cameraFrames: number
  keystrokeWindows: number
  mouseWindows: number
  gazeWindows: number
  totalBytes: number
}

const props = defineProps<{
  open: boolean
  sessionId: string
  /** Human-readable reasons the session was flagged, from the snapshot flags. */
  reasons: string[]
  retentionDays: number
}>()

const emit = defineEmits<{ close: []; decided: [kept: boolean] }>()

const { t } = useI18n()

const summary = ref<EvidenceSummary | null>(null)
const frames = ref<EvidencePreview[]>([])
const showFrames = ref(false)
const busy = ref(false)
const outcome = ref<'kept' | 'discarded' | null>(null)
const error = ref('')

const frameCount = computed(() => summary.value?.cameraFrames ?? 0)
const hasAnything = computed(
  () =>
    !!summary.value &&
    summary.value.cameraFrames +
      summary.value.keystrokeWindows +
      summary.value.mouseWindows +
      summary.value.gazeWindows >
      0,
)

watch(
  () => props.open,
  async (isOpen) => {
    if (!isOpen) return
    outcome.value = null
    error.value = ''
    showFrames.value = false
    try {
      summary.value = await invoke<EvidenceSummary>('sentinel_evidence_pending', {
        sessionId: props.sessionId,
      })
    } catch (e) {
      error.value = String(e)
    }
  },
  { immediate: true },
)

async function revealFrames() {
  if (frames.value.length === 0) {
    try {
      frames.value = await invoke<EvidencePreview[]>('sentinel_evidence_preview', {
        sessionId: props.sessionId,
      })
    } catch (e) {
      error.value = String(e)
      return
    }
  }
  showFrames.value = true
}

async function decide(granted: boolean) {
  busy.value = true
  error.value = ''
  try {
    await invoke<number>('sentinel_evidence_decide', {
      sessionId: props.sessionId,
      granted,
    })
    outcome.value = granted ? 'kept' : 'discarded'
    emit('decided', granted)
  } catch (e) {
    error.value = String(e)
  } finally {
    busy.value = false
  }
}

function formatTime(iso: string): string {
  const d = new Date(iso)
  return Number.isNaN(d.getTime()) ? iso : d.toLocaleTimeString()
}
</script>

<template>
  <AppModal :open="open" :title="t('sentinel.evidence.flaggedTitle')" @close="emit('close')">
    <div v-if="outcome" class="ec-done">
      <p>
        {{
          outcome === 'kept' ? t('sentinel.evidence.kept') : t('sentinel.evidence.discarded')
        }}
      </p>
      <AppButton @click="emit('close')">{{ t('common.actions.close') }}</AppButton>
    </div>

    <div v-else class="ec">
      <p class="ec-intro">
        {{ t('sentinel.evidence.flaggedIntro', { count: reasons.length }) }}
      </p>

      <section v-if="reasons.length" class="ec-section">
        <h3 class="ec-h">{{ t('sentinel.evidence.whatHappened') }}</h3>
        <ul class="ec-reasons">
          <li v-for="r in reasons" :key="r">{{ r }}</li>
        </ul>
      </section>

      <section class="ec-section">
        <h3 class="ec-h">{{ t('sentinel.evidence.previewTitle') }}</h3>
        <p v-if="!hasAnything" class="ec-muted">{{ t('sentinel.evidence.previewNone') }}</p>
        <template v-else>
          <p class="ec-muted">{{ t('sentinel.evidence.previewFrames', { n: frameCount }) }}</p>
          <AppButton
            v-if="frameCount > 0"
            variant="ghost"
            @click="showFrames ? (showFrames = false) : revealFrames()"
          >
            {{
              showFrames ? t('sentinel.evidence.hideFrames') : t('sentinel.evidence.viewFrames')
            }}
          </AppButton>
          <div v-if="showFrames" class="ec-frames">
            <figure v-for="(f, i) in frames" :key="`${f.snapshotId}-${i}`" class="ec-frame">
              <img v-if="f.dataUrl" :src="f.dataUrl" alt="" />
              <figcaption>
                {{ t('sentinel.evidence.capturedAt', { time: formatTime(f.capturedAt) }) }}
              </figcaption>
            </figure>
          </div>
        </template>
      </section>

      <section class="ec-section">
        <h3 class="ec-h">{{ t('sentinel.evidence.keepQuestion') }}</h3>
        <p>{{ t('sentinel.evidence.keepExplain', { days: retentionDays }) }}</p>
        <p class="ec-note">{{ t('sentinel.evidence.keepNote') }}</p>
      </section>

      <p v-if="error" class="ec-error">{{ error }}</p>

      <!-- Equal visual weight on both choices: a refusal that looks like the
           lesser option is not a free refusal. -->
      <div class="ec-actions">
        <AppButton :disabled="busy" @click="decide(false)">
          {{ t('sentinel.evidence.discard') }}
        </AppButton>
        <AppButton :disabled="busy || !hasAnything" @click="decide(true)">
          {{ t('sentinel.evidence.keep', { days: retentionDays }) }}
        </AppButton>
      </div>
    </div>
  </AppModal>
</template>

<style scoped>
.ec {
  display: flex;
  flex-direction: column;
  gap: 1.25rem;
}
.ec-intro {
  margin: 0;
}
.ec-section {
  display: flex;
  flex-direction: column;
  gap: 0.5rem;
}
.ec-h {
  margin: 0;
  font-size: 0.9rem;
  font-weight: 600;
}
.ec-reasons {
  margin: 0;
  padding-left: 1.1rem;
}
.ec-muted {
  margin: 0;
  opacity: 0.75;
  font-size: 0.9rem;
}
.ec-note {
  margin: 0;
  font-size: 0.85rem;
  opacity: 0.75;
}
.ec-frames {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(140px, 1fr));
  gap: 0.75rem;
}
.ec-frame {
  margin: 0;
}
.ec-frame img {
  width: 100%;
  border-radius: 6px;
  display: block;
}
.ec-frame figcaption {
  font-size: 0.75rem;
  opacity: 0.7;
  margin-top: 0.25rem;
}
.ec-actions {
  display: flex;
  gap: 0.75rem;
  justify-content: flex-end;
}
.ec-error {
  margin: 0;
  color: var(--color-danger, #b00);
  font-size: 0.9rem;
}
.ec-done {
  display: flex;
  flex-direction: column;
  gap: 1rem;
}
</style>
