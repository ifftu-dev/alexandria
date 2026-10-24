<script setup lang="ts">
import { computed, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { chatErrorCode, useChat } from '@/composables/useChat'
import type { ChatArtifact } from '@/types'
import ChatGoalProposal from './ChatGoalProposal.vue'
import ChatGoalChange from './ChatGoalChange.vue'

/** A note or flashcard set under the answer it came from. A proposal is
 * saved only when the learner saves it. */
const props = defineProps<{ item: ChatArtifact }>()
const { t } = useI18n()
const chat = useChat()
const busy = ref(false)
const open = ref(false)

const cards = computed(() => (props.item.body.kind === 'flashcards' ? props.item.body.cards : []))
const preview = computed(() => (props.item.body.kind === 'note' ? props.item.body.text : ''))

async function run(action: () => Promise<void>) {
  busy.value = true
  try {
    await action()
  } catch (e) {
    chat.error.value = chatErrorCode(e)
  } finally {
    busy.value = false
  }
}
</script>

<template>
  <ChatGoalProposal v-if="item.body.kind === 'goal'" :item="item" />
  <ChatGoalChange v-else-if="item.body.kind === 'goal_change'" :item="item" />
  <p v-else-if="item.status === 'saved'" class="flex items-center gap-1 text-xs text-success">
    <svg class="h-3 w-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M5 12l5 5L20 7" /></svg>
    {{ t('chat.artifacts.saved') }} · {{ item.title }}
  </p>
  <div v-else class="art" :title="t('chat.artifacts.suggestionHint')">
    <div class="art-row">
      <span class="art-ic" aria-hidden="true">
        <svg v-if="item.body.kind === 'note'" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linejoin="round"><path d="M6 3h9l4 4v14H6z" /><path d="M9 12h6M9 16h4" /></svg>
        <svg v-else viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8"><rect x="3" y="6" width="14" height="12" rx="2" /><path d="M7 3h12a2 2 0 0 1 2 2v10" /></svg>
      </span>
      <button type="button" class="art-text" :aria-expanded="open" @click="open = !open">
        <b class="truncate">{{ item.title }}</b>
        <small>{{ item.body.kind === 'note' ? t('chat.artifacts.suggestedNote') : `${t('chat.artifacts.suggestedCards')} · ${t('chat.artifacts.cards', cards.length)}` }}</small>
      </button>
      <button type="button" class="art-save" :disabled="busy" @click="run(() => chat.saveArtifact(item.id))">{{ t('chat.artifacts.save') }}</button>
      <button type="button" class="art-dismiss" :disabled="busy" @click="run(() => chat.deleteArtifact(item.id))">{{ t('chat.artifacts.dismiss') }}</button>
    </div>
    <div v-if="open" class="art-preview">
      <p v-if="preview" class="line-clamp-6 whitespace-pre-wrap">{{ preview }}</p>
      <dl v-else class="grid gap-1.5">
        <div v-for="(card, i) in cards" :key="i">
          <dt class="font-medium text-foreground">{{ card.front }}</dt>
          <dd>{{ card.back }}</dd>
        </div>
      </dl>
      <p class="mt-2 text-xs">{{ t('chat.artifacts.suggestionHint') }}</p>
    </div>
  </div>
</template>

<style scoped>
.art { border-radius: 0.875rem; background: color-mix(in srgb, var(--app-foreground) 6%, transparent); font-size: 0.8125rem; }
.art-row { display: flex; align-items: center; gap: 0.625rem; padding: 0.625rem 0.75rem; }
.art-ic { display: grid; flex: none; color: var(--app-primary); }
.art-ic svg { width: 1rem; height: 1rem; }
.art-text { display: flex; min-width: 0; flex: 1; flex-direction: column; text-align: start; }
.art-text b { font-weight: 500; color: var(--app-foreground); }
.art-text small { font-size: 0.75rem; color: var(--app-muted-foreground); }
.art-save { flex: none; padding: 0.25rem 0.375rem; border-radius: 0.375rem; font-weight: 600; color: var(--app-primary); }
.art-dismiss { flex: none; padding: 0.25rem 0.375rem; border-radius: 0.375rem; color: var(--app-muted-foreground); }
.art-save:hover, .art-dismiss:hover { background: color-mix(in srgb, var(--app-foreground) 7%, transparent); }
.art-save:disabled, .art-dismiss:disabled { opacity: 0.5; }
.art-preview { padding: 0 0.75rem 0.75rem 2.375rem; color: var(--app-muted-foreground); }
</style>
