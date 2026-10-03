<script setup lang="ts">
import { ref, computed, onMounted, onBeforeUnmount, watch } from 'vue'
import { useLocalApi } from '@/composables/useLocalApi'
import ThreadThumbnail from '@/components/opinions/ThreadThumbnail.vue'
import ThreadVotes from '@/components/opinions/ThreadVotes.vue'
import ThreadMeta from '@/components/opinions/ThreadMeta.vue'
import { AppButton } from '@/components/ui'
import type { DiscussionItem, OpinionRow, SubjectFieldInfo } from '@/types'
const { invoke } = useLocalApi()
const threads = ref<DiscussionItem[]>([])
const legacy = ref<OpinionRow[]>([])
const fields = ref<SubjectFieldInfo[]>([])
const field = ref('')
const sort = ref('new')
const error = ref('')
const loading = ref(true)
const busy = ref(false)
const more = ref(false)
let generation = 0
let timer: ReturnType<typeof setInterval> | undefined
const names = computed(() => new Map(fields.value.map(f => [f.id, f.name])))
const filteredLegacy = computed(() => legacy.value.filter(p => !field.value || p.subject_field_id === field.value))
async function load(append = false) {
  const ticket = ++generation
  try {
    const rows = await invoke<DiscussionItem[]>('list_discussions', { subjectFieldId: field.value || null, sort: sort.value, offset: append ? threads.value.length : 0 })
    if (ticket !== generation) return
    threads.value = append ? [...threads.value, ...rows] : rows
    more.value = rows.length === 200
    error.value = ''
  } catch (e) { if (ticket === generation) error.value = String(e) }
  finally { loading.value = false }
}
async function vote(item: DiscussionItem, value: number) {
  busy.value = true
  try { await invoke('act_on_discussion', { req: { entity_id: item.id, thread_id: item.thread_id, subject_field_id: item.subject_field_id, action: { kind: 'vote', value } } }); await load() }
  catch (e) { error.value = String(e) }
  finally { busy.value = false }
}
watch([field, sort], () => load())
onMounted(async () => {
  try {
    const [f, l] = await Promise.all([invoke<SubjectFieldInfo[]>('list_subject_fields'), invoke<OpinionRow[]>('list_opinions')])
    fields.value = f; legacy.value = l
    await load()
    timer = setInterval(() => { if (!busy.value && threads.value.length <= 200) void load() }, 10000)
  } catch (e) { error.value = String(e); loading.value = false }
})
onBeforeUnmount(() => { generation++; if (timer) clearInterval(timer) })
</script>
<template>
  <div class="mx-auto max-w-5xl">
    <header class="mb-5 flex items-center justify-between gap-3">
      <div><h1 class="text-xl font-bold">{{ $t('opinions.threads.heading') }}</h1><p class="mt-1 hidden text-sm text-muted-foreground sm:block">{{ $t('opinions.threads.intro') }}</p></div>
      <AppButton size="sm" @click="$router.push('/discussions/new')">+ {{ $t('opinions.threads.create') }}</AppButton>
    </header>
    <div class="mb-5 flex flex-wrap items-center gap-3">
      <select v-model="field" :aria-label="$t('opinions.threads.topic')" class="min-w-0 flex-1 rounded-lg border border-input bg-background px-3 py-2 text-sm sm:flex-none"><option value="">{{ $t('opinions.threads.allTopics') }}</option><option v-for="f in fields" :key="f.id" :value="f.id">{{ f.icon_emoji }} {{ f.name }}</option></select>
      <select v-model="sort" :aria-label="$t('opinions.threads.sort')" class="w-24 rounded-lg border border-input bg-background px-3 py-2 text-sm sm:w-auto"><option value="new">{{ $t('opinions.threads.new') }}</option><option value="top">{{ $t('opinions.threads.top') }}</option><option value="discussed">{{ $t('opinions.threads.discussed') }}</option></select>
      <AppButton variant="ghost" size="sm" :aria-label="$t('opinions.threads.refresh')" class="sm:ms-auto" @click="load()"><svg class="h-4 w-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" aria-hidden="true"><path d="M20 7v5h-5M4 17v-5h5M6 7a7 7 0 0 1 12-1l2 3M4 15l2 3a7 7 0 0 0 12-1" /></svg><span class="hidden sm:inline">{{ $t('opinions.threads.refresh') }}</span></AppButton>
    </div>
    <details class="mb-5 rounded-lg bg-muted/40 px-4 py-3 text-xs text-muted-foreground">
      <summary class="cursor-pointer font-medium text-foreground">{{ $t('opinions.threads.gateTitle') }}</summary>
      <p class="mt-2 leading-relaxed">{{ $t('opinions.threads.gateBody') }}</p><p class="mt-2 leading-relaxed">{{ $t('opinions.threads.voteNote') }}</p>
    </details>
    <div>
        <p v-if="error" role="alert" class="mb-4 rounded-xl bg-red-500/10 p-4 text-sm text-red-500">{{ error }}</p>
        <p v-if="loading" class="p-6 text-muted-foreground">{{ $t('opinions.threads.loading') }}</p>
        <div v-if="threads.length" class="divide-y divide-border/60 overflow-hidden rounded-xl bg-card shadow-sm">
          <article v-for="item in threads" :key="item.id" class="px-4 py-5 transition-colors hover:bg-muted/20 sm:px-5">
            <ThreadMeta class="mb-3" :author="item.author_did" :created-at="item.created_at" :topic="names.get(item.subject_field_id)" />
            <router-link :to="`/discussions/${item.id}`" class="flex items-start justify-between gap-4"><div class="min-w-0"><h2 class="break-words text-base font-semibold leading-snug">{{ item.content?.title }}</h2><p class="mt-2 line-clamp-2 text-sm text-muted-foreground">{{ item.body }}</p><p v-if="item.content?.url" class="mt-1 truncate text-xs text-primary">{{ item.content.url }}</p></div><ThreadThumbnail :cid="item.content?.thumbnail_cid" :kind="item.content?.post_kind" :topic="item.subject_field_id" /></router-link>
            <div class="mt-3 flex flex-wrap items-center gap-3"><ThreadVotes :score="item.score" :vote="item.my_vote" :disabled="busy" @vote="vote(item, $event)" /><router-link :to="`/discussions/${item.id}`" class="text-xs font-medium text-muted-foreground">{{ $t('opinions.threads.comments', { count: item.comment_count }) }}</router-link><span class="ms-auto text-[11px] text-muted-foreground">{{ $t('opinions.threads.qualified') }}</span></div>
          </article>
        </div>
        <p v-if="!loading && !threads.length" class="py-5 text-sm text-muted-foreground">{{ $t('opinions.threads.empty') }}</p>
        <button v-if="more" class="my-4 text-sm text-primary" @click="load(true)">{{ $t('opinions.threads.loadMore') }}</button>
        <article v-for="item in filteredLegacy" :key="item.id" class="mb-2 rounded-xl bg-card p-4 shadow-sm"><router-link :to="`/discussions/legacy/${item.id}`" class="flex items-center gap-4"><ThreadThumbnail :cid="item.thumbnail_cid" kind="video" :topic="item.subject_field_id" /><div><p class="text-xs text-muted-foreground">{{ names.get(item.subject_field_id) }}</p><h2 class="font-semibold">{{ item.title }}</h2><p class="mt-1 text-xs text-muted-foreground">{{ $t('opinions.threads.legacy') }}</p></div></router-link></article>
    </div>
  </div>
</template>
