<script setup lang="ts">
/**
 * Course-completion celebration — a "new achievement" reveal in the spirit of a
 * AAA game (emblem pop + glow + confetti + shine), styled with our own tokens.
 * Mounted once globally (AppLayout); driven by useCourseCompletion so it
 * survives the Player navigating away, and shows live credential-mint progress.
 */
import { computed, watch, ref, onBeforeUnmount } from 'vue'
import { useRouter } from 'vue-router'
import { useI18n } from 'vue-i18n'
import { useCourseCompletion } from '@/composables/useCourseCompletion'
import { CREDENTIAL_KINDS, type CredentialClass } from '@/components/credential/credentialKind'

const { t } = useI18n()
const router = useRouter()
const {
  isOpen, courseTitle, isTutorial, txHash, mintStage, items, primaryCredentialId,
  unmetElements, elapsedMs, etaMs, progressPct, close,
} = useCourseCompletion()

const pct = (v: number) => `${Math.round(v * 100)}%`

// Gradeable elements the learner still needs to pass (shown when no
// credential could be earned), most-relevant first: attempted-but-below the
// bar, then never-attempted.
const unmet = computed(() =>
  [...unmetElements.value].sort((a, b) => (b.best_score ?? -1) - (a.best_score ?? -1)),
)
const anyAttempted = computed(() => unmet.value.some((u) => u.best_score !== null))

function fmtSecs(ms: number): string {
  const s = ms / 1000
  return s < 10 ? s.toFixed(1) : `${Math.round(s)}`
}

// Right-hand readout on the progress bar: a live ETA while minting/anchoring,
// then the final elapsed time once the batch is done.
const etaLabel = computed(() => {
  if (mintStage.value === 'issued') return t('courses.completion.etaDone', { seconds: fmtSecs(elapsedMs.value) })
  if (mintStage.value === 'unavailable') return ''
  if (mintStage.value === 'anchoring') {
    return etaMs.value > 0 ? t('courses.completion.etaSecuring', { seconds: fmtSecs(etaMs.value) }) : t('courses.completion.etaSecuringConfirming')
  }
  return etaMs.value > 0 ? t('courses.completion.etaLeft', { seconds: fmtSecs(etaMs.value) }) : t('courses.completion.etaFinishing')
})

function kindMeta(kind: string) {
  return kind in CREDENTIAL_KINDS
    ? CREDENTIAL_KINDS[kind as CredentialClass]
    : CREDENTIAL_KINDS.SelfAssertion
}

const mintedCount = computed(() => items.value.filter((i) => i.status === 'minted').length)

// Confetti burst — generated once per open. Each piece radiates from the emblem
// then falls, with a randomized angle/distance/spin/colour/delay.
interface Piece {
  tx: string
  ty: string
  rot: string
  delay: string
  dur: string
  color: string
  left: string
  size: string
}
const COLORS = ['#34d399', '#fbbf24', '#60a5fa', '#f472b6', '#a78bfa', '#f87171']
const pieces = ref<Piece[]>([])

function rand(min: number, max: number) {
  return min + Math.random() * (max - min)
}

function spawnConfetti() {
  const n = 70
  const out: Piece[] = []
  for (let i = 0; i < n; i++) {
    const angle = rand(0, Math.PI * 2)
    const dist = rand(120, 340)
    out.push({
      tx: `${Math.cos(angle) * dist}px`,
      ty: `${Math.sin(angle) * dist - rand(40, 140)}px`,
      rot: `${rand(-720, 720)}deg`,
      delay: `${rand(0, 120)}ms`,
      dur: `${rand(900, 1700)}ms`,
      color: COLORS[i % COLORS.length] as string,
      left: `${rand(44, 56)}%`,
      size: `${rand(6, 11)}px`,
    })
  }
  pieces.value = out
}

watch(isOpen, (open) => {
  if (open) {
    document.body.style.overflow = 'hidden'
    document.addEventListener('keydown', onKey)
    spawnConfetti()
  } else {
    document.body.style.overflow = ''
    document.removeEventListener('keydown', onKey)
    pieces.value = []
  }
})

function onKey(e: KeyboardEvent) {
  if (e.key === 'Escape') close()
}

onBeforeUnmount(() => {
  document.body.style.overflow = ''
  document.removeEventListener('keydown', onKey)
})

const shortTx = computed(() =>
  txHash.value ? `${txHash.value.slice(0, 8)}…${txHash.value.slice(-6)}` : '',
)

const mint = computed(() => {
  const total = items.value.length
  switch (mintStage.value) {
    case 'issued':
      return {
        label: t('courses.completion.credentialsReady', { count: total }, total),
        note: t('courses.completion.credentialsReadyNote'),
      }
    case 'minting':
      return {
        label: t('courses.completion.creatingLabel'),
        note: t('courses.completion.creatingNote', { done: mintedCount.value, total }),
      }
    case 'anchoring':
      return {
        label: t('courses.completion.securingLabel'),
        note: t('courses.completion.securingNote'),
      }
    default:
      return {
        label: t('courses.completion.notEarnedLabel'),
        note: unmet.value.length
          ? t('courses.completion.notEarnedNote', { count: unmet.value.length }, unmet.value.length)
          : t('courses.completion.noAssessmentNote'),
      }
  }
})

const hasCredential = computed(() => mintStage.value !== 'unavailable' && items.value.length > 0)

function viewCredential() {
  const id = primaryCredentialId.value
  close()
  // A single issued credential → its detail; otherwise the credentials list
  // (derived view) where the whole batch + its evidence live.
  if (id && items.value.length === 1) {
    router.push({ name: 'credential-detail', params: { id } })
  } else {
    router.push({ name: 'credentials' })
  }
}

function continueToDashboard() {
  close()
  router.push('/')
}
</script>

<template>
  <Teleport to="body">
    <Transition name="celebrate">
      <div v-if="isOpen" class="celebrate-root" @click.self="close">
        <div class="celebrate-backdrop" />

        <!-- Confetti layer -->
        <div class="confetti" aria-hidden="true">
          <span
            v-for="(p, i) in pieces"
            :key="i"
            class="confetti-piece"
            :style="{
              left: p.left,
              width: p.size,
              height: p.size,
              background: p.color,
              '--tx': p.tx,
              '--ty': p.ty,
              '--rot': p.rot,
              animationDelay: p.delay,
              animationDuration: p.dur,
            }"
          />
        </div>

        <div class="celebrate-card">
          <!-- Emblem -->
          <div class="emblem-wrap">
            <div class="emblem-glow" />
            <div class="emblem-ring" />
            <div class="emblem">
              <svg viewBox="0 0 24 24" class="h-9 w-9" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
                <path d="M8 21h8M12 17v4M7 4h10v4a5 5 0 0 1-10 0V4zM7 6H4v1a3 3 0 0 0 3 3M17 6h3v1a3 3 0 0 1-3 3" />
              </svg>
            </div>
            <div class="emblem-shine" />
          </div>

          <p class="eyebrow">{{ $t('courses.completion.achievementUnlocked') }}</p>
          <h2 class="title">{{ isTutorial ? $t('courses.completion.tutorialComplete') : $t('courses.completion.courseComplete') }}</h2>
          <p class="course-name">{{ courseTitle }}</p>

          <!-- Credential mint progress -->
          <div class="mint">
            <div class="mint-row">
              <span class="mint-label">{{ mint.label }}</span>
              <span class="mint-note">{{ mint.note }}</span>
            </div>

            <!-- Realtime progress bar with % + live ETA -->
            <div v-if="mintStage !== 'unavailable'">
              <div class="mint-meter">
                <span>{{ progressPct }}%</span>
                <span>{{ etaLabel }}</span>
              </div>
              <div class="mint-bar">
                <div
                  class="mint-bar-fill"
                  :class="{ anchoring: mintStage === 'anchoring' }"
                  :style="{ width: `${progressPct}%` }"
                />
              </div>
            </div>

            <!-- Per-credential batch -->
            <ul v-if="items.length" class="mint-list">
              <li
                v-for="(it, i) in items"
                :key="it.id"
                class="mint-item"
                :style="{ animationDelay: `${120 + i * 90}ms` }"
              >
                <span class="mint-chip" :class="kindMeta(it.kind).dot">
                  <svg viewBox="0 0 24 24" class="h-3 w-3" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                    <path :d="kindMeta(it.kind).icon" />
                  </svg>
                </span>
                <span class="mint-item-label">{{ it.label }}</span>
                <span class="mint-item-kind">{{ kindMeta(it.kind).label }}</span>
                <span class="mint-status" :class="{ done: it.status === 'minted' }">
                  <svg v-if="it.status === 'minted'" viewBox="0 0 24 24" class="h-3.5 w-3.5" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round" stroke-linejoin="round"><path d="M5 13l4 4L19 7" /></svg>
                  <span v-else class="spinner" />
                </span>
              </li>
            </ul>

            <!-- Why no credential yet: per-element score vs the passing bar -->
            <div v-if="mintStage === 'unavailable' && unmet.length" class="unmet">
              <ul class="unmet-list">
                <li v-for="u in unmet" :key="u.element_id" class="unmet-item">
                  <div class="unmet-head">
                    <span class="unmet-title">{{ u.title }}</span>
                    <span
                      class="unmet-badge"
                      :class="u.best_score === null ? 'is-none' : 'is-fail'"
                    >{{ u.best_score === null ? $t('courses.completion.notAttempted') : $t('courses.completion.belowPassing') }}</span>
                  </div>
                  <div class="unmet-scores">
                    <span>{{ $t('courses.completion.yourScore') }} <strong>{{ u.best_score === null ? '—' : pct(u.best_score) }}</strong></span>
                    <span>{{ $t('courses.completion.passing') }} <strong>{{ pct(u.required_score) }}</strong></span>
                  </div>
                  <div class="unmet-bar">
                    <div class="unmet-need" :style="{ left: `${Math.round(u.required_score * 100)}%` }" />
                    <div class="unmet-got" :style="{ width: `${Math.round((u.best_score ?? 0) * 100)}%` }" />
                  </div>
                </li>
              </ul>
              <p class="unmet-why-title">{{ $t('courses.completion.whyTitle') }}</p>
              <ul class="unmet-why">
                <li v-if="anyAttempted">{{ $t('courses.completion.whyBelow') }}</li>
                <li v-if="unmet.some((u) => u.best_score === null)">{{ $t('courses.completion.whyNotAttempted') }}</li>
                <li>{{ $t('courses.completion.whyWrong') }}</li>
                <li>{{ $t('courses.completion.whyMultiSelect') }}</li>
              </ul>
              <p class="unmet-cta">{{ $t('courses.completion.cta', { pct: pct(unmet[0]?.required_score ?? 0.6) }) }}</p>
            </div>

            <a
              v-if="shortTx"
              class="mint-tx"
              :href="`https://preprod.cardanoscan.io/transaction/${txHash}`"
              target="_blank"
              rel="noopener"
            >{{ $t('courses.completion.viewPublicRecord') }}</a>
          </div>

          <div class="actions">
            <button v-if="hasCredential" class="btn btn-primary" @click="viewCredential">
              {{ $t('courses.completion.viewCredentials', { count: items.length }, items.length) }}
            </button>
            <button class="btn" @click="continueToDashboard">{{ $t('common.actions.continue') }}</button>
          </div>
        </div>
      </div>
    </Transition>
  </Teleport>
</template>

<style scoped>
.celebrate-root {
  position: fixed;
  inset: 0;
  z-index: 70;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 1rem;
  overflow: hidden;
}
.celebrate-backdrop {
  position: absolute;
  inset: 0;
  background: rgba(2, 6, 23, 0.72);
  backdrop-filter: blur(6px);
}

.celebrate-card {
  position: relative;
  z-index: 2;
  width: 100%;
  max-width: 28rem;
  padding: 2rem 1.75rem 1.5rem;
  border-radius: 1.25rem;
  border: 1px solid var(--app-border, rgba(148, 163, 184, 0.2));
  background: var(--app-card, #0b1220);
  box-shadow: 0 20px 60px -15px rgba(0, 0, 0, 0.6), 0 0 0 1px rgba(255, 255, 255, 0.02);
  text-align: center;
  animation: card-pop 0.5s cubic-bezier(0.22, 1, 0.36, 1) both;
}

/* Emblem */
.emblem-wrap {
  position: relative;
  width: 96px;
  height: 96px;
  margin: 0 auto 1rem;
}
.emblem {
  position: absolute;
  inset: 12px;
  border-radius: 9999px;
  display: flex;
  align-items: center;
  justify-content: center;
  color: #0b1220;
  background: linear-gradient(135deg, #fde68a 0%, #34d399 55%, #10b981 100%);
  box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.5);
  animation: emblem-pop 0.6s cubic-bezier(0.34, 1.56, 0.64, 1) 0.05s both;
}
.emblem-ring {
  position: absolute;
  inset: 0;
  border-radius: 9999px;
  border: 2px solid rgba(52, 211, 153, 0.5);
  animation: ring-pulse 1.8s ease-out infinite;
}
.emblem-glow {
  position: absolute;
  inset: -14px;
  border-radius: 9999px;
  background: radial-gradient(circle, rgba(52, 211, 153, 0.45), transparent 65%);
  filter: blur(6px);
  animation: glow-breathe 2.4s ease-in-out infinite;
}
.emblem-shine {
  position: absolute;
  inset: 12px;
  border-radius: 9999px;
  overflow: hidden;
  pointer-events: none;
}
.emblem-shine::after {
  content: '';
  position: absolute;
  top: -60%;
  left: -120%;
  width: 60%;
  height: 220%;
  background: linear-gradient(75deg, transparent, rgba(255, 255, 255, 0.85), transparent);
  transform: rotate(8deg);
  animation: shine 2.6s ease-in-out 0.6s infinite;
}

.eyebrow {
  font-size: 0.7rem;
  letter-spacing: 0.18em;
  text-transform: uppercase;
  font-weight: 700;
  color: #fbbf24;
  margin: 0;
  animation: fade-up 0.5s ease 0.15s both;
}
.title {
  font-size: 1.5rem;
  font-weight: 800;
  margin: 0.15rem 0 0.1rem;
  color: var(--app-foreground, #f8fafc);
  animation: fade-up 0.5s ease 0.22s both;
}
.course-name {
  font-size: 0.95rem;
  color: var(--app-muted-foreground, #94a3b8);
  margin: 0;
  animation: fade-up 0.5s ease 0.28s both;
}

.chips {
  display: flex;
  flex-wrap: wrap;
  gap: 0.4rem;
  justify-content: center;
  margin: 1rem 0 0.25rem;
}
.chip {
  font-size: 0.72rem;
  padding: 0.2rem 0.6rem;
  border-radius: 9999px;
  background: rgba(52, 211, 153, 0.12);
  border: 1px solid rgba(52, 211, 153, 0.3);
  color: #6ee7b7;
  animation: chip-in 0.4s cubic-bezier(0.34, 1.56, 0.64, 1) both;
}
.chip-more {
  background: rgba(148, 163, 184, 0.12);
  border-color: rgba(148, 163, 184, 0.3);
  color: var(--app-muted-foreground, #94a3b8);
}

.mint {
  margin-top: 1.25rem;
  padding: 0.75rem 0.85rem;
  border-radius: 0.75rem;
  border: 1px solid var(--app-border, rgba(148, 163, 184, 0.18));
  background: rgba(148, 163, 184, 0.06);
  text-align: left;
  animation: fade-up 0.5s ease 0.4s both;
}
.mint-row {
  display: flex;
  flex-direction: column;
  gap: 0.05rem;
  margin-bottom: 0.6rem;
}
.spinner {
  width: 0.85rem;
  height: 0.85rem;
  border-radius: 9999px;
  border: 2px solid rgba(52, 211, 153, 0.25);
  border-top-color: #34d399;
  animation: spin 0.8s linear infinite;
}
.mint-meter {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  margin-bottom: 0.3rem;
  font-size: 0.68rem;
  font-variant-numeric: tabular-nums;
  color: var(--app-muted-foreground, #94a3b8);
}
.mint-meter span:first-child {
  font-weight: 700;
  color: var(--app-foreground, #e2e8f0);
}
.mint-bar {
  height: 6px;
  border-radius: 9999px;
  background: rgba(148, 163, 184, 0.18);
  overflow: hidden;
  margin-bottom: 0.7rem;
}
.mint-bar-fill {
  height: 100%;
  border-radius: 9999px;
  background: linear-gradient(90deg, #34d399, #10b981);
  transition: width 0.45s cubic-bezier(0.22, 1, 0.36, 1);
}
.mint-bar-fill.anchoring {
  background-image: linear-gradient(
    115deg,
    #34d399 0%, #34d399 40%, #6ee7b7 50%, #34d399 60%, #34d399 100%
  );
  background-size: 200% 100%;
  animation: bar-shimmer 1.4s linear infinite;
}
@keyframes bar-shimmer {
  to { background-position: -200% 0; }
}
.mint-list {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: 0.4rem;
}
.mint-item {
  display: flex;
  align-items: center;
  gap: 0.55rem;
  animation: chip-in 0.4s cubic-bezier(0.34, 1.56, 0.64, 1) both;
}
.mint-chip {
  flex: none;
  width: 1.4rem;
  height: 1.4rem;
  border-radius: 0.45rem;
  display: flex;
  align-items: center;
  justify-content: center;
  color: #fff;
}
.mint-item-label {
  flex: 1;
  min-width: 0;
  font-size: 0.8rem;
  font-weight: 600;
  color: var(--app-foreground, #e2e8f0);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.mint-item-kind {
  flex: none;
  font-size: 0.65rem;
  color: var(--app-muted-foreground, #94a3b8);
}
.mint-status {
  flex: none;
  width: 1.3rem;
  height: 1.3rem;
  border-radius: 9999px;
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--app-muted-foreground, #94a3b8);
  background: rgba(148, 163, 184, 0.15);
}
.mint-status.done {
  color: #0b1220;
  background: #34d399;
}
.mint-label {
  font-size: 0.82rem;
  font-weight: 600;
  color: var(--app-foreground, #e2e8f0);
}
.mint-note {
  font-size: 0.72rem;
  color: var(--app-muted-foreground, #94a3b8);
}
.mint-tx {
  display: inline-block;
  margin-top: 0.5rem;
  font-size: 0.68rem;
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  color: #60a5fa;
  text-decoration: none;
}
.mint-tx:hover {
  text-decoration: underline;
}

/* "Why no credential yet" panel */
.unmet-list {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: 0.6rem;
}
.unmet-item {
  padding: 0.55rem 0.6rem;
  border-radius: 0.55rem;
  background: rgba(148, 163, 184, 0.08);
  border: 1px solid var(--app-border, rgba(148, 163, 184, 0.18));
}
.unmet-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 0.5rem;
}
.unmet-title {
  font-size: 0.8rem;
  font-weight: 600;
  color: var(--app-foreground, #e2e8f0);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.unmet-badge {
  flex: none;
  font-size: 0.62rem;
  font-weight: 700;
  padding: 0.1rem 0.4rem;
  border-radius: 9999px;
}
.unmet-badge.is-fail {
  background: rgba(239, 68, 68, 0.16);
  color: #f87171;
}
.unmet-badge.is-none {
  background: rgba(148, 163, 184, 0.18);
  color: var(--app-muted-foreground, #94a3b8);
}
.unmet-scores {
  display: flex;
  justify-content: space-between;
  margin-top: 0.3rem;
  font-size: 0.68rem;
  color: var(--app-muted-foreground, #94a3b8);
  font-variant-numeric: tabular-nums;
}
.unmet-scores strong {
  color: var(--app-foreground, #e2e8f0);
}
.unmet-bar {
  position: relative;
  height: 6px;
  margin-top: 0.35rem;
  border-radius: 9999px;
  background: rgba(148, 163, 184, 0.18);
  overflow: hidden;
}
.unmet-got {
  height: 100%;
  border-radius: 9999px;
  background: linear-gradient(90deg, #fbbf24, #f87171);
}
.unmet-need {
  position: absolute;
  top: -2px;
  bottom: -2px;
  width: 2px;
  background: #34d399;
  z-index: 1;
}
.unmet-why-title {
  margin: 0.7rem 0 0.25rem;
  font-size: 0.72rem;
  font-weight: 600;
  color: var(--app-foreground, #e2e8f0);
}
.unmet-why {
  margin: 0;
  padding-left: 1rem;
  font-size: 0.7rem;
  line-height: 1.45;
  color: var(--app-muted-foreground, #94a3b8);
}
.unmet-cta {
  margin: 0.55rem 0 0;
  font-size: 0.72rem;
  font-weight: 600;
  color: #6ee7b7;
}

.actions {
  display: flex;
  gap: 0.5rem;
  margin-top: 1.25rem;
  animation: fade-up 0.5s ease 0.48s both;
}
.btn {
  flex: 1;
  font-size: 0.85rem;
  font-weight: 600;
  padding: 0.55rem 0.75rem;
  border-radius: 0.6rem;
  border: 1px solid var(--app-border, rgba(148, 163, 184, 0.25));
  background: transparent;
  color: var(--app-foreground, #e2e8f0);
  cursor: pointer;
  transition: background-color 120ms ease, transform 80ms ease;
}
.btn:hover {
  background: rgba(148, 163, 184, 0.1);
}
.btn:active {
  transform: translateY(1px);
}
.btn-primary {
  border-color: transparent;
  background: linear-gradient(135deg, #34d399, #10b981);
  color: #04130c;
}
.btn-primary:hover {
  background: linear-gradient(135deg, #6ee7b7, #34d399);
}

/* Confetti */
.confetti {
  position: absolute;
  inset: 0;
  z-index: 3;
  pointer-events: none;
}
.confetti-piece {
  position: absolute;
  top: 38%;
  border-radius: 1px;
  opacity: 0;
  animation-name: confetti-fly;
  animation-timing-function: cubic-bezier(0.18, 0.7, 0.3, 1);
  animation-fill-mode: forwards;
}

@keyframes confetti-fly {
  0% {
    opacity: 1;
    transform: translate(0, 0) rotate(0deg) scale(1);
  }
  70% {
    opacity: 1;
  }
  100% {
    opacity: 0;
    transform: translate(var(--tx), calc(var(--ty) + 240px)) rotate(var(--rot)) scale(0.6);
  }
}
@keyframes card-pop {
  from { opacity: 0; transform: translateY(14px) scale(0.94); }
  to { opacity: 1; transform: translateY(0) scale(1); }
}
@keyframes emblem-pop {
  0% { transform: scale(0); }
  100% { transform: scale(1); }
}
@keyframes ring-pulse {
  0% { transform: scale(1); opacity: 0.6; }
  100% { transform: scale(1.5); opacity: 0; }
}
@keyframes glow-breathe {
  0%, 100% { opacity: 0.5; transform: scale(0.96); }
  50% { opacity: 0.9; transform: scale(1.06); }
}
@keyframes shine {
  0%, 100% { left: -120%; }
  35%, 65% { left: 130%; }
}
@keyframes fade-up {
  from { opacity: 0; transform: translateY(8px); }
  to { opacity: 1; transform: translateY(0); }
}
@keyframes chip-in {
  from { opacity: 0; transform: scale(0.8); }
  to { opacity: 1; transform: scale(1); }
}
@keyframes spin {
  to { transform: rotate(360deg); }
}

/* Modal transition */
.celebrate-enter-active { transition: opacity 0.25s ease; }
.celebrate-leave-active { transition: opacity 0.2s ease; }
.celebrate-enter-from,
.celebrate-leave-to { opacity: 0; }

@media (prefers-reduced-motion: reduce) {
  .celebrate-card,
  .emblem,
  .emblem-ring,
  .emblem-glow,
  .emblem-shine::after,
  .eyebrow,
  .title,
  .course-name,
  .chip,
  .mint,
  .actions,
  .confetti-piece,
  .spinner,
  .mint-bar-fill.anchoring {
    animation: none !important;
  }
  .confetti { display: none; }
}
</style>
