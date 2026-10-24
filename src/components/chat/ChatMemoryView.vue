<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { AppButton } from '@/components/ui'
import { chatErrorCode, useChat } from '@/composables/useChat'
import ChatMemoryProposal from './ChatMemoryProposal.vue'
import type { ChatMemoryItem } from '@/types'

/** Everything chats remember, with on/off, edit, and forget. */
const { t } = useI18n()
const chat = useChat()
const adding = ref('')
const editingId = ref<string | null>(null)
const editText = ref('')
const busy = ref(false)

const saved = computed(() => chat.memoryItems.value.filter((m) => m.status === 'approved'))
const suggested = computed(() => chat.memoryItems.value.filter((m) => m.status === 'proposed'))

onMounted(() => {
  void chat.refreshMemory().catch((e) => (chat.error.value = chatErrorCode(e)))
})

async function run(action: () => Promise<void>) {
  busy.value = true
  try {
    await action()
  } catch (e) {
    const code = chatErrorCode(e)
    chat.error.value = code === 'invalid_input' ? 'memory' : code
  } finally {
    busy.value = false
  }
}

function startEdit(item: ChatMemoryItem) {
  editingId.value = item.id
  editText.value = item.content
}

async function finishEdit(item: ChatMemoryItem) {
  const text = editText.value.trim()
  editingId.value = null
  if (text && text !== item.content) await run(() => chat.updateMemory(item.id, { content: text }))
}

async function add() {
  const text = adding.value.trim()
  if (!text) return
  await run(async () => {
    await chat.addMemory('preference', text)
    adding.value = ''
  })
}
</script>

<template>
  <div class="min-h-0 flex-1 space-y-3 overflow-y-auto p-3 text-sm">
    <p class="text-xs text-muted-foreground">{{ t('chat.memory.intro') }}</p>

    <form class="flex gap-1.5" @submit.prevent="add">
      <input v-model="adding" class="input flex-1 py-1.5 text-sm" maxlength="300" :placeholder="t('chat.memory.addPlaceholder')" :aria-label="t('chat.memory.add')">
      <AppButton size="sm" type="submit" :disabled="!adding.trim() || busy">{{ t('chat.memory.add') }}</AppButton>
    </form>

    <section>
      <h3 class="mb-1.5 text-xs font-semibold uppercase tracking-wide text-muted-foreground">{{ t('chat.memory.saved') }}</h3>
      <p v-if="!saved.length" class="text-xs text-muted-foreground">{{ t('chat.memory.noneSaved') }}</p>
      <ul class="space-y-1.5">
        <li v-for="item in saved" :key="item.id" class="rounded-lg border border-border px-2.5 py-2">
          <input
            v-if="editingId === item.id"
            v-model="editText"
            class="input py-1 text-sm"
            maxlength="300"
            :aria-label="t('chat.memory.edit')"
            @keydown.enter.prevent="finishEdit(item)"
            @keydown.esc="editingId = null"
            @blur="finishEdit(item)"
          >
          <p v-else :class="{ 'text-muted-foreground line-through': !item.active }">
            <span class="me-1 text-xs text-muted-foreground">{{ item.kind === 'observation' ? t('chat.memory.observation') : t('chat.memory.preference') }} ·</span>{{ item.content }}
          </p>
          <div class="mt-1.5 flex flex-wrap items-center gap-1.5 text-xs">
            <button
              type="button"
              role="switch"
              class="chat-switch"
              :data-on="item.active"
              :aria-checked="item.active"
              :aria-label="item.active ? t('chat.memory.on') : t('chat.memory.off')"
              :disabled="busy"
              @click="run(() => chat.updateMemory(item.id, { active: !item.active }))"
            />
            <span class="text-muted-foreground">{{ item.active ? t('chat.memory.on') : t('chat.memory.off') }}</span>
            <span class="flex-1" />
            <AppButton variant="ghost" size="xs" type="button" :disabled="busy" @click="startEdit(item)">{{ t('chat.memory.edit') }}</AppButton>
            <AppButton variant="ghost" size="xs" type="button" :disabled="busy" @click="run(() => chat.forgetMemory(item.id))">{{ t('chat.memory.forget') }}</AppButton>
          </div>
        </li>
      </ul>
    </section>

    <section v-if="suggested.length">
      <h3 class="mb-1.5 text-xs font-semibold uppercase tracking-wide text-muted-foreground">{{ t('chat.memory.suggested') }}</h3>
      <div class="space-y-1.5">
        <ChatMemoryProposal v-for="item in suggested" :key="item.id" :item="item" />
      </div>
    </section>

    <p class="text-xs text-muted-foreground">{{ t('chat.memory.forgetNote') }}</p>
  </div>
</template>

<style scoped>
.chat-switch { position: relative; width: 1.75rem; height: 1rem; flex: none; border-radius: 999px; background: var(--app-border); }
.chat-switch[data-on='true'] { background: var(--app-primary); }
.chat-switch::after { content: ''; position: absolute; top: 2px; inset-inline-start: 2px; width: 12px; height: 12px; border-radius: 999px; background: #fff; transition: inset-inline-start 150ms; }
.chat-switch[data-on='true']::after { inset-inline-start: 14px; }
.chat-switch:focus-visible { outline: 2px solid var(--app-ring); outline-offset: 2px; }
</style>
