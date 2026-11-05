<script setup lang="ts">
import ChatPanel from '@/components/chat/ChatPanel.vue'
import { useChat } from '@/composables/useChat'

// The AI chat as the expanded workspace shows it, open on a thread whose
// history fills more than the panel.
const chat = useChat()
chat.setOpen(true)
void chat.openThread('thread-fixture')
</script>

<template>
  <div class="flex h-[32rem] flex-col overflow-hidden rounded-xl border border-border">
    <ChatPanel variant="expanded" />
  </div>
</template>
