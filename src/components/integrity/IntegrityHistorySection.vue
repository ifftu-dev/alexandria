<script setup lang="ts">
/**
 * Settings → Integrity: the sessions Sentinel monitored, and anything the
 * learner chose to keep.
 *
 * This closes a promise made in `docs/sentinel.md`: evidence a learner consented
 * to retain is theirs to inspect and delete at any time. Without a surface, that
 * promise held only in the sense that a command existed to satisfy it.
 *
 * Deletion here is unconditional and needs no justification. It cannot cost the
 * learner their appeal, because an adjudication rests on the scores and absent
 * evidence may not be held against them — so the button carries no warning about
 * weakening their case, which would be a way of discouraging its use.
 *
 * Releasing is the other half, and is deliberately not automatic. Nothing here
 * guesses which remote assessment a local session belongs to: the learner picks,
 * from the assessments those services say are theirs. An automatic match that
 * got it wrong would send camera frames of somebody to an organisation that was
 * never assessing them, which is the worst thing this screen could do.
 */

import { ref, computed, onMounted } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { useI18n } from 'vue-i18n'

import { AppButton } from '@/components/ui'

interface IntegritySession {
  id: string
  status: string
  integrity_score: number | null
  critical_count: number
  warning_count: number
  started_at: string
  ended_at: string | null
}

interface EvidenceSummary {
  sessionId: string
  cameraFrames: number
  keystrokeWindows: number
  mouseWindows: number
  gazeWindows: number
  totalBytes: number
}

interface EvidencePreview {
  snapshotId: string
  kind: string
  capturedAt: string
  dataUrl: string | null
}

interface ContestableRun {
  directory: string
  directoryUrl: string
  organisation: string
  runId: string
  role: string
  status: string
  integrityFlagged: boolean
  evidenceReleased: number
}

/** How long a service holds released evidence. Mirrors APPEAL_WINDOW_DAYS. */
const APPEAL_DAYS = 14

const { t } = useI18n()

const sessions = ref<IntegritySession[]>([])
const retained = ref<Record<string, EvidenceSummary>>({})
const frames = ref<Record<string, EvidencePreview[]>>({})
const expanded = ref<string | null>(null)
const loading = ref(true)
const error = ref('')

const runs = ref<ContestableRun[]>([])
/** Which remote assessment the learner has picked, per local session. */
const target = ref<Record<string, string>>({})
const sending = ref<string | null>(null)
const sent = ref<Record<string, number>>({})
const withdrawn = ref<Record<string, boolean>>({})
const owed = ref(0)

/** Only flagged assessments can be contested, so only those are offered. */
const flaggedRuns = computed(() => runs.value.filter((r) => r.integrityFlagged))

function runByKey(key: string): ContestableRun | undefined {
  return runs.value.find((r) => `${r.directoryUrl}|${r.runId}` === key)
}

onMounted(async () => {
  try {
    sessions.value = await invoke<IntegritySession[]>('integrity_list_sessions', {})
    // Only flagged sessions can hold evidence, so only those are queried.
    for (const s of sessions.value.filter((x) => x.status === 'flagged')) {
      try {
        retained.value[s.id] = await invoke<EvidenceSummary>('sentinel_evidence_stored', {
          sessionId: s.id,
        })
      } catch {
        /* a session with no consent row simply has nothing retained */
      }
    }
  } catch (e) {
    error.value = String(e)
  } finally {
    loading.value = false
  }

  // Anything this device still owes a service is retried whenever this screen
  // opens. A withdrawal asked for while offline is owed until it is sent, and
  // the person who asked for it is exactly the person looking at this page.
  try {
    owed.value = await invoke<number>('holder_retry_withdrawals', {})
  } catch {
    /* the vault may be locked, or every directory unreachable; still owed */
  }

  // Best effort and separate from the list above: a person with no directories
  // configured, or no network, still gets their own retained evidence.
  try {
    const res = await invoke<{ items: ContestableRun[] }>('holder_contestable_runs', {})
    runs.value = res.items
  } catch {
    /* nothing to contest is not an error worth showing here */
  }
})

async function release(sessionId: string) {
  const key = target.value[sessionId] ?? ''
  const run = runByKey(key)
  if (!run || sending.value) return
  sending.value = sessionId
  error.value = ''
  try {
    const res = await invoke<{ items: number }>('holder_release_evidence', {
      directoryUrl: run.directoryUrl,
      runId: run.runId,
      sessionId,
    })
    sent.value[sessionId] = res.items
    run.evidenceReleased += res.items
  } catch (e) {
    error.value = String(e)
  } finally {
    sending.value = null
  }
}

async function withdraw(sessionId: string, run: ContestableRun) {
  error.value = ''
  try {
    await invoke('holder_withdraw_evidence', {
      directoryUrl: run.directoryUrl,
      runId: run.runId,
      sessionId,
    })
    run.evidenceReleased = 0
    delete sent.value[sessionId]
    withdrawn.value[sessionId] = true
  } catch (e) {
    // Asked for regardless: the request is queued on the device and retried,
    // so the honest thing to report is that it has not been sent *yet*.
    error.value = String(e)
    owed.value += 1
  }
}

function retainedCount(id: string): number {
  const r = retained.value[id]
  if (!r) return 0
  return r.cameraFrames + r.keystrokeWindows + r.mouseWindows + r.gazeWindows
}

async function toggle(id: string) {
  if (expanded.value === id) {
    expanded.value = null
    return
  }
  expanded.value = id
  if (!frames.value[id]) {
    try {
      frames.value[id] = await invoke<EvidencePreview[]>('sentinel_evidence_stored_preview', {
        sessionId: id,
      })
    } catch (e) {
      error.value = String(e)
    }
  }
}

async function remove(id: string) {
  try {
    await invoke<number>('sentinel_evidence_delete', { sessionId: id })
    delete retained.value[id]
    delete frames.value[id]
    if (expanded.value === id) expanded.value = null
  } catch (e) {
    error.value = String(e)
  }
}

function fmtDate(iso: string | null): string {
  if (!iso) return '—'
  const d = new Date(iso)
  return Number.isNaN(d.getTime()) ? iso : d.toLocaleString()
}
</script>

<template>
  <section class="ih">
    <p class="ih-intro">{{ t('sentinel.evidence.historyIntro') }}</p>

    <p v-if="owed > 0" class="ih-warn">
      {{ t('sentinel.evidence.releaseOwed', { n: owed }) }}
    </p>

    <p v-if="error" class="ih-error">{{ error }}</p>
    <p v-else-if="loading" class="ih-muted">{{ t('common.actions.loading') }}</p>
    <p v-else-if="sessions.length === 0" class="ih-muted">
      {{ t('sentinel.evidence.historyEmpty') }}
    </p>

    <ul v-else class="ih-list">
      <li v-for="s in sessions" :key="s.id" class="ih-item">
        <div class="ih-row">
          <div>
            <p class="ih-when">{{ fmtDate(s.started_at) }}</p>
            <p class="ih-muted">
              {{ s.status }}
              <template v-if="s.critical_count || s.warning_count">
                · {{ s.critical_count }}/{{ s.warning_count }}
              </template>
            </p>
          </div>

          <div v-if="retainedCount(s.id) > 0" class="ih-actions">
            <AppButton variant="ghost" @click="toggle(s.id)">
              {{ t('sentinel.evidence.retained') }} ({{ retainedCount(s.id) }})
            </AppButton>
            <AppButton variant="danger" @click="remove(s.id)">
              {{ t('sentinel.evidence.deleteNow') }}
            </AppButton>
          </div>
          <p v-else-if="s.status === 'flagged'" class="ih-muted">
            {{ t('sentinel.evidence.retainedNone') }}
          </p>
        </div>

        <div v-if="expanded === s.id" class="ih-frames">
          <figure v-for="(f, i) in frames[s.id] ?? []" :key="`${f.snapshotId}-${i}`">
            <img v-if="f.dataUrl" :src="f.dataUrl" alt="" />
            <figcaption>{{ fmtDate(f.capturedAt) }}</figcaption>
          </figure>
        </div>

        <!--
          Offered only where there is something to send. A control that is
          visible but inert on every unflagged session reads as a thing the
          learner has failed to do.
        -->
        <div v-if="retainedCount(s.id) > 0" class="ih-release">
          <p class="ih-when">{{ t('sentinel.evidence.releaseTitle') }}</p>
          <p class="ih-muted">{{ t('sentinel.evidence.releaseIntro') }}</p>

          <p v-if="flaggedRuns.length === 0" class="ih-muted">
            {{ t('sentinel.evidence.releaseChooseNone') }}
          </p>

          <template v-else>
            <label class="ih-field">
              <span>{{ t('sentinel.evidence.releaseChoose') }}</span>
              <select v-model="target[s.id]">
                <option value="">—</option>
                <option
                  v-for="r in flaggedRuns"
                  :key="`${r.directoryUrl}|${r.runId}`"
                  :value="`${r.directoryUrl}|${r.runId}`"
                >
                  {{ r.organisation }} · {{ r.role }}
                </option>
              </select>
            </label>

            <p class="ih-warn">
              {{ t('sentinel.evidence.releaseWarn', { days: APPEAL_DAYS }) }}
            </p>

            <div class="ih-actions">
              <AppButton
                :disabled="!target[s.id] || sending === s.id"
                @click="release(s.id)"
              >
                {{
                  sending === s.id
                    ? t('sentinel.evidence.releaseSending')
                    : t('sentinel.evidence.releaseSend')
                }}
              </AppButton>
            </div>

            <p v-if="sent[s.id]" class="ih-muted">
              {{ t('sentinel.evidence.releaseSent', { n: sent[s.id] }) }}
            </p>
          </template>

          <!--
            What somebody else is currently holding, and the way to end it.
            Shown per run rather than as a total, because withdrawing is
            addressed to one service and a total cannot be acted on.
          -->
          <div v-for="r in flaggedRuns.filter((x) => x.evidenceReleased > 0)" :key="r.runId" class="ih-held">
            <span>
              {{ t('sentinel.evidence.releaseHeld', { n: r.evidenceReleased, org: r.organisation }) }}
            </span>
            <AppButton variant="ghost" @click="withdraw(s.id, r)">
              {{ t('sentinel.evidence.releaseWithdraw') }}
            </AppButton>
          </div>

          <p v-if="withdrawn[s.id]" class="ih-muted">
            {{ t('sentinel.evidence.releaseWithdrawn') }}
          </p>

          <p class="ih-muted">{{ t('sentinel.evidence.releaseDeleteAlso') }}</p>
        </div>
      </li>
    </ul>
  </section>
</template>

<style scoped>
.ih {
  display: flex;
  flex-direction: column;
  gap: 1rem;
}
.ih-intro {
  margin: 0;
}
.ih-list {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: 0.75rem;
}
.ih-item {
  border: 1px solid var(--color-border, rgba(128, 128, 128, 0.3));
  border-radius: 10px;
  padding: 0.75rem 0.9rem;
}
.ih-row {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 1rem;
  flex-wrap: wrap;
}
.ih-when {
  margin: 0;
  font-weight: 600;
}
.ih-muted {
  margin: 0;
  opacity: 0.72;
  font-size: 0.85rem;
}
.ih-actions {
  display: flex;
  gap: 0.5rem;
}
.ih-frames {
  margin-top: 0.75rem;
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(130px, 1fr));
  gap: 0.6rem;
}
.ih-frames img {
  width: 100%;
  border-radius: 6px;
  display: block;
}
.ih-frames figcaption {
  font-size: 0.72rem;
  opacity: 0.7;
}
.ih-error {
  margin: 0;
  color: var(--color-danger, #b00);
}
.ih-release {
  margin-top: 0.9rem;
  padding-top: 0.75rem;
  border-top: 1px solid var(--color-border, rgba(128, 128, 128, 0.3));
  display: flex;
  flex-direction: column;
  gap: 0.6rem;
}
.ih-field {
  display: flex;
  flex-direction: column;
  gap: 0.25rem;
  font-size: 0.85rem;
}
.ih-field select {
  font: inherit;
  padding: 0.4rem 0.5rem;
  border: 1px solid var(--color-border, rgba(128, 128, 128, 0.3));
  border-radius: 6px;
  background: var(--color-surface, transparent);
  color: inherit;
}
/* Not styled as an error. Nothing has gone wrong — this is what the control
   does, said before it is used rather than after. */
.ih-warn {
  margin: 0;
  font-size: 0.85rem;
  opacity: 0.85;
}
.ih-held {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 0.75rem;
  flex-wrap: wrap;
  font-size: 0.85rem;
}
</style>
