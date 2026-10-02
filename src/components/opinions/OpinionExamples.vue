<script setup lang="ts">
import { computed } from 'vue'
import ThreadThumbnail from './ThreadThumbnail.vue'
import type { OpinionExample } from '@/types'

const props = defineProps<{ examples: OpinionExample[]; subjectFieldId: string }>()
const filtered = computed(() => props.examples.filter(e => !props.subjectFieldId || e.subject_field_id === props.subjectFieldId))
</script>

<template>
  <section v-if="filtered.length" class="mt-6 space-y-3">
    <div>
      <h2 class="text-base font-semibold">{{ $t('opinions.examples.title') }}</h2>
      <p class="mt-1 text-xs text-muted-foreground">{{ $t('opinions.examples.description') }}</p>
    </div>
    <div class="divide-y divide-border overflow-hidden rounded-xl bg-card shadow-sm">
      <router-link v-for="example in filtered" :key="example.id" :to="`/opinions/examples/${example.id}`" class="flex w-full items-start gap-4 p-4 text-start hover:bg-muted/30">
        <ThreadThumbnail :cid="example.thumbnail_cid" kind="video" :topic="example.subject_field_id" />
        <div class="min-w-0">
        <span class="text-[10px] font-medium uppercase tracking-wide text-muted-foreground">{{ $t('opinions.examples.badge') }}</span>
        <h3 class="mt-1 text-sm font-semibold">{{ example.title }}</h3>
        <p class="mt-1 line-clamp-2 text-xs text-muted-foreground">{{ example.summary }}</p>
        <span class="mt-2 block text-xs text-muted-foreground">{{ $t('opinions.examples.watch') }} · {{ Math.floor(example.duration_seconds / 60) }}:{{ String(example.duration_seconds % 60).padStart(2, '0') }}</span>
        </div>
      </router-link>
    </div>
  </section>
</template>
