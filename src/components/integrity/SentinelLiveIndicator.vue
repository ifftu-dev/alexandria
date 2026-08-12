<script setup lang="ts">
/**
 * A quiet, always-honest signal of what Sentinel can currently see.
 *
 * Shown only while a session is active — which, since monitoring follows the
 * element, means only while an assessment is on screen.
 *
 * The reason this exists is fairness rather than transparency theatre. Most
 * integrity flags are caused by things a learner would fix instantly if they
 * knew: a sibling walking behind them, a lamp putting their face in shadow,
 * another window stealing focus. Detecting those silently and raising them
 * afterwards produces flags nobody needed to earn, and an appeals process to
 * handle flags that should never have existed.
 *
 * It is deliberately not an alarm. No red, no counts, no score. Telling someone
 * mid-assessment that they look suspicious would harm the performance being
 * measured, which is its own kind of unfair. It states what the camera sees,
 * once, calmly, and says plainly that nothing has been recorded — which is true:
 * only derived scores are stored, and evidence is retained only if the learner
 * later opts in.
 */

import { computed, ref } from 'vue'
import { useI18n } from 'vue-i18n'

import { useSentinel } from '@/composables/useSentinel'

const { t } = useI18n()
const { debug } = useSentinel()

const expanded = ref(false)

/** Ratio above which "looking away" is worth mentioning. Matches the scorer's
 *  own tolerance closely enough that we don't warn about behaviour that would
 *  never be flagged. */
const OFFSCREEN_NOTICE = 0.35

const notices = computed<string[]>(() => {
  const out: string[] = []
  if (!debug.active) return out

  if (debug.cameraOptedIn) {
    if (debug.faceCount > 1) out.push(t('sentinel.evidence.liveTwoPeople'))
    else if (!debug.facePresent) out.push(t('sentinel.evidence.liveNoFace'))

    const ratio = debug.gazeOffscreenRatio
    if (ratio !== null && ratio > OFFSCREEN_NOTICE) {
      out.push(t('sentinel.evidence.liveLookingAway'))
    }
  }

  if (debug.appFocusLostCount > 0) out.push(t('sentinel.evidence.liveOtherApp'))

  return out
})

const visible = computed(() => debug.active)
const calm = computed(() => notices.value.length === 0)
</script>

<template>
  <div v-if="visible" class="sli" :class="{ 'sli-calm': calm }">
    <button
      type="button"
      class="sli-chip"
      :aria-expanded="expanded"
      @click="expanded = !expanded"
    >
      <span class="sli-dot" aria-hidden="true" />
      <span class="sli-label">
        {{ calm ? t('sentinel.evidence.liveOk') : notices[0] }}
      </span>
    </button>

    <div v-if="expanded" class="sli-panel">
      <p class="sli-h">{{ t('sentinel.evidence.liveWhat') }}</p>
      <ul v-if="!calm" class="sli-list">
        <li v-for="n in notices" :key="n">{{ n }}</li>
      </ul>
      <p v-else class="sli-muted">{{ t('sentinel.evidence.liveOk') }}</p>
      <p class="sli-muted">{{ t('sentinel.evidence.liveExplain') }}</p>
    </div>
  </div>
</template>

<style scoped>
.sli {
  position: fixed;
  right: 1rem;
  bottom: 1rem;
  z-index: 40;
  max-width: 18rem;
}
.sli-chip {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  padding: 0.4rem 0.7rem;
  border-radius: 999px;
  border: 1px solid var(--color-border, rgba(128, 128, 128, 0.35));
  background: var(--color-surface, rgba(255, 255, 255, 0.9));
  color: inherit;
  font: inherit;
  font-size: 0.8rem;
  cursor: pointer;
  backdrop-filter: blur(6px);
}
.sli-dot {
  width: 7px;
  height: 7px;
  border-radius: 50%;
  /* Amber, never red: this is information, not an accusation. */
  background: #d08700;
  flex: none;
}
.sli-calm .sli-dot {
  background: #6b7280;
}
.sli-label {
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.sli-panel {
  margin-top: 0.5rem;
  padding: 0.7rem 0.8rem;
  border-radius: 10px;
  border: 1px solid var(--color-border, rgba(128, 128, 128, 0.35));
  background: var(--color-surface, rgba(255, 255, 255, 0.96));
  font-size: 0.8rem;
  backdrop-filter: blur(6px);
}
.sli-h {
  margin: 0 0 0.35rem;
  font-weight: 600;
}
.sli-list {
  margin: 0 0 0.5rem;
  padding-left: 1rem;
}
.sli-muted {
  margin: 0;
  opacity: 0.72;
}
</style>
