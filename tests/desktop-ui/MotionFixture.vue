<script setup lang="ts">
import { onMounted, onBeforeUnmount, ref } from 'vue'
import AppOpening from '@/components/auth/AppOpening.vue'
import ProfileSelect from '@/pages/ProfileSelect.vue'
import Onboarding from '@/pages/Onboarding.vue'
import BlankLayout from '@/layouts/BlankLayout.vue'
import MotionSettings from '@/components/ui/MotionSettings.vue'
import { AppButton, AppSpinner } from '@/components/ui'
import ChatMessageItem from '@/components/chat/ChatMessageItem.vue'
import PluginHost from '@/components/plugin/PluginHost.vue'
import type { ChatMessage, Element } from '@/types'
import { useMotion } from '@/composables/useMotion'
const params = new URLSearchParams(location.search)
const ready = ref(false)
const { mode } = useMotion()
const streamingAnswer: ChatMessage = {
  id: 'specimen-answer', thread_id: 'specimen', position: 1, role: 'assistant', reply_to: null, version: 1,
  content: '', status: 'streaming', error: null, connection_id: null, connection_name: null, model_id: null,
  location: null, input_tokens: null, output_tokens: null, sources: [], created_at: '2026-11-05T00:00:00Z', completed_at: null,
}
const pluginElement = { id: 'specimen-element', plugin_cid: 'specimen-plugin', content_inline: null } as unknown as Element
let boot: ReturnType<typeof setTimeout>
onMounted(() => { boot = setTimeout(() => { ready.value = true }, Number(params.get('boot') ?? 0)) })
onBeforeUnmount(() => clearTimeout(boot))
</script>
<template>
  <AppOpening :ready="ready" />
  <BlankLayout v-if="ready">
    <div class="p-4 border-b border-border"><MotionSettings /></div>
    <div v-if="params.get('progress') === '1'" class="p-4 flex gap-4 items-center" data-progress-specimens>
      <AppSpinner label="Saving draft" />
      <AppButton loading>Save draft</AppButton>
      <ChatMessageItem :message="streamingAnswer" live="Thinking it through" />
      <div class="h-40 w-40"><PluginHost :element="pluginElement" /></div>
    </div>
    <ProfileSelect v-if="params.get('existing') === '1'" />
    <Onboarding v-else />
  </BlankLayout>
  <p v-else role="status">Initializing</p>
  <span class="sr-only" data-motion-mode>{{ mode }}</span>
</template>
