<script setup lang="ts">
import { ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { AppButton } from '@/components/ui'
import { useChat } from '@/composables/useChat'
import { chatErrorCode } from '@/utils/chatErrors'
import type { ChatMemoryItem } from '@/types'

/** A model's suggestion for learning memory. Nothing is remembered unless
 * the learner saves it; they can edit it first or dismiss it. */
const props = defineProps<{ item: ChatMemoryItem }>()
const { t } = useI18n()
const chat = useChat()
const editing = ref(false)
const text = ref(props.item.content)
const busy = ref(false)

async function run(action: () => Promise<void>) {
  busy.value = true
  try {
    await action()
  } catch (e) {
    chat.error.value = chatErrorCode(e) === 'invalid_input' ? 'memory' : chatErrorCode(e)
  } finally {
    busy.value = false
  }
}
</script>

<template>
  <div class="rounded-2xl bg-muted/60 px-3.5 py-3 text-[13px]">
    <p class="font-semibold">{{ t('chat.memory.suggestion') }}</p>
    <p class="mt-0.5 text-muted-foreground">{{ t('chat.memory.suggestionHint') }}</p>
    <input
      v-if="editing"
      v-model="text"
      class="input mt-1.5 py-1 text-sm"
      maxlength="300"
      :aria-label="t('chat.memory.edit')"
      @keydown.enter.prevent="run(() => chat.approveMemory(item.id, text))"
    >
    <p v-else class="mt-1.5 text-sm text-foreground">
      <span class="me-1 text-muted-foreground">{{ item.kind === 'observation' ? t('chat.memory.observation') : t('chat.memory.preference') }} ·</span>{{ item.content }}
    </p>
    <div class="mt-2 flex gap-1.5">
      <AppButton size="xs" type="button" :loading="busy" @click="run(() => chat.approveMemory(item.id, editing ? text : undefined))">{{ t('chat.memory.save') }}</AppButton>
      <AppButton v-if="!editing" variant="ghost" size="xs" type="button" :disabled="busy" @click="editing = true">{{ t('chat.memory.edit') }}</AppButton>
      <AppButton variant="ghost" size="xs" type="button" :disabled="busy" @click="run(() => chat.forgetMemory(item.id))">{{ t('chat.memory.dismiss') }}</AppButton>
    </div>
  </div>
</template>
