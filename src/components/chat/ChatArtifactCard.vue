<script setup lang="ts">
import { computed, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { AppButton } from '@/components/ui'
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
  <p v-else-if="item.status === 'saved'" class="flex items-center gap-1 text-[11px] text-success">
    <svg class="h-3 w-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M5 12l5 5L20 7" /></svg>
    {{ t('chat.artifacts.saved') }} · {{ item.title }}
  </p>
  <div v-else class="rounded-xl border border-dashed border-border bg-background px-3 py-2 text-xs">
    <p class="font-semibold">{{ item.body.kind === 'note' ? t('chat.artifacts.suggestedNote') : t('chat.artifacts.suggestedCards') }}</p>
    <p class="mt-0.5 text-muted-foreground">{{ t('chat.artifacts.suggestionHint') }}</p>
    <p class="mt-1.5 text-sm font-medium text-foreground">{{ item.title }}</p>
    <p v-if="preview" class="mt-0.5 line-clamp-3 whitespace-pre-wrap text-muted-foreground">{{ preview }}</p>
    <template v-else>
      <p class="mt-0.5 text-muted-foreground">{{ t('chat.artifacts.cards', cards.length) }}</p>
      <ul class="mt-1 space-y-0.5">
        <li v-for="(card, i) in cards.slice(0, 3)" :key="i" class="truncate text-foreground">{{ card.front }}</li>
      </ul>
    </template>
    <div class="mt-2 flex gap-1.5">
      <AppButton size="xs" type="button" :loading="busy" @click="run(() => chat.saveArtifact(item.id))">{{ t('chat.artifacts.save') }}</AppButton>
      <AppButton variant="ghost" size="xs" type="button" :disabled="busy" @click="run(() => chat.deleteArtifact(item.id))">{{ t('chat.artifacts.dismiss') }}</AppButton>
    </div>
  </div>
</template>
