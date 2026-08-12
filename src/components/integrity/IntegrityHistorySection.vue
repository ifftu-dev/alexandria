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
 */

import { ref, onMounted } from 'vue'
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

const { t } = useI18n()

const sessions = ref<IntegritySession[]>([])
const retained = ref<Record<string, EvidenceSummary>>({})
const frames = ref<Record<string, EvidencePreview[]>>({})
const expanded = ref<string | null>(null)
const loading = ref(true)
const error = ref('')

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
})

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
</style>
