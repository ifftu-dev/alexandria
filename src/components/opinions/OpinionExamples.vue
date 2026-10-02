<script setup lang="ts">
import { computed, ref } from 'vue'
import VideoPlayer from '@/components/course/VideoPlayer.vue'
import type { OpinionExample } from '@/types'

const props = defineProps<{ examples: OpinionExample[]; subjectFieldId: string }>()
const selectedId = ref<string | null>(null)
const filtered = computed(() => props.examples.filter(e => !props.subjectFieldId || e.subject_field_id === props.subjectFieldId))
const selected = computed(() => filtered.value.find(e => e.id === selectedId.value) ?? null)
</script>

<template>
  <section v-if="filtered.length" class="mt-8 space-y-4">
    <div>
      <h2 class="text-lg font-semibold">{{ $t('opinions.examples.title') }}</h2>
      <p class="text-sm text-muted-foreground">{{ $t('opinions.examples.description') }}</p>
    </div>
    <div v-if="selected" class="rounded-xl border border-border bg-card p-5 space-y-3">
      <div class="flex items-start justify-between gap-4">
        <h3 class="font-semibold text-lg">{{ selected.title }}</h3>
        <button type="button" class="text-sm text-primary" @click="selectedId = null">{{ $t('common.actions.close') }}</button>
      </div>
      <p class="text-sm text-muted-foreground">{{ selected.summary }}</p>
      <VideoPlayer :key="selected.id" :content-cid="selected.video_cid" :title="selected.title" />
    </div>
    <div class="grid gap-4 sm:grid-cols-2 xl:grid-cols-3">
      <button v-for="example in filtered" :key="example.id" type="button" class="rounded-xl border border-border bg-card p-5 text-start hover:border-primary/50" @click="selectedId = example.id">
        <span class="text-xs font-medium text-primary">{{ $t('opinions.examples.badge') }}</span>
        <h3 class="mt-2 font-semibold">{{ example.title }}</h3>
        <p class="mt-2 line-clamp-3 text-sm text-muted-foreground">{{ example.summary }}</p>
        <span class="mt-3 block text-xs text-muted-foreground">{{ $t('opinions.examples.watch') }} · {{ Math.floor(example.duration_seconds / 60) }}:{{ String(example.duration_seconds % 60).padStart(2, '0') }}</span>
      </button>
    </div>
  </section>
</template>
