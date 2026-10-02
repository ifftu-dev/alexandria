<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useRoute } from 'vue-router'
import { useLocalApi } from '@/composables/useLocalApi'
import VideoPlayer from '@/components/course/VideoPlayer.vue'
import type { OpinionExample } from '@/types'

const route = useRoute()
const { invoke } = useLocalApi()
const examples = ref<OpinionExample[]>([])
const loading = ref(true)
const error = ref('')
const example = computed(() => examples.value.find(item => item.id === route.params.id))
onMounted(async () => {
  try { examples.value = await invoke<OpinionExample[]>('list_demo_opinions') }
  catch (e) { error.value = String(e) }
  finally { loading.value = false }
})
</script>

<template>
  <div class="mx-auto max-w-4xl">
    <router-link to="/opinions" class="mb-5 inline-flex text-xs text-muted-foreground hover:text-foreground">← {{ $t('opinions.threads.back') }}</router-link>
    <p v-if="error" role="alert" class="text-sm text-error">{{ error }}</p>
    <p v-else-if="loading" class="py-8 text-sm text-muted-foreground">{{ $t('opinions.threads.loading') }}</p>
    <article v-else-if="example" class="overflow-hidden rounded-xl bg-card shadow-sm">
      <header class="p-5 sm:p-7">
        <p class="text-xs text-muted-foreground">{{ $t('opinions.examples.badge') }} · {{ Math.floor(example.duration_seconds / 60) }}:{{ String(example.duration_seconds % 60).padStart(2, '0') }}</p>
        <h1 class="mt-3 text-xl font-bold leading-snug sm:text-2xl">{{ example.title }}</h1>
        <p class="mt-4 text-sm leading-relaxed text-muted-foreground">{{ example.summary }}</p>
      </header>
      <VideoPlayer :key="example.id" :content-cid="example.video_cid" :title="example.title" />
      <p class="border-t border-border/60 p-5 text-xs leading-relaxed text-muted-foreground sm:p-7">{{ $t('opinions.examples.description') }}</p>
    </article>
    <p v-else class="text-sm text-muted-foreground">{{ $t('opinions.threads.notFound') }}</p>
  </div>
</template>
