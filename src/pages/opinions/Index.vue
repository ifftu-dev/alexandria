<script setup lang="ts">
import { ref, computed, onMounted, onBeforeUnmount, watch } from 'vue'
import { useLocalApi } from '@/composables/useLocalApi'
import ThreadThumbnail from '@/components/opinions/ThreadThumbnail.vue'
import ThreadVotes from '@/components/opinions/ThreadVotes.vue'
import OpinionExamples from '@/components/opinions/OpinionExamples.vue'
import type { DiscussionItem, OpinionExample, OpinionRow, SubjectFieldInfo } from '@/types'
const { invoke } = useLocalApi()
const threads = ref<DiscussionItem[]>([])
const examples = ref<OpinionExample[]>([])
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
    const [f, e, l] = await Promise.all([invoke<SubjectFieldInfo[]>('list_subject_fields'), invoke<OpinionExample[]>('list_demo_opinions'), invoke<OpinionRow[]>('list_opinions')])
    fields.value = f; examples.value = e; legacy.value = l
    await load()
    timer = setInterval(() => { if (!busy.value && threads.value.length <= 200) void load() }, 10000)
  } catch (e) { error.value = String(e); loading.value = false }
})
onBeforeUnmount(() => { generation++; if (timer) clearInterval(timer) })
</script>
<template>
  <div class="mx-auto max-w-6xl">
    <header class="mb-7 flex flex-col items-start justify-between gap-4 sm:flex-row sm:items-center">
      <div><p class="mb-1 text-xs font-semibold uppercase tracking-widest text-primary">{{ $t('opinions.threads.eyebrow') }}</p><h1 class="text-3xl font-bold">{{ $t('opinions.threads.heading') }}</h1><p class="mt-2 text-sm text-muted-foreground">{{ $t('opinions.threads.intro') }}</p></div>
      <router-link to="/opinions/new" class="shrink-0 rounded-full bg-primary px-5 py-2.5 text-sm font-semibold text-primary-foreground">+ {{ $t('opinions.threads.create') }}</router-link>
    </header>
    <div class="grid items-start gap-7 lg:grid-cols-[minmax(0,1fr)_260px]">
      <main class="min-w-0">
        <div class="mb-3 flex flex-wrap items-center gap-3 rounded-xl border border-border bg-card p-3">
          <select v-model="field" :aria-label="$t('opinions.threads.topic')" class="min-w-0 max-w-full rounded-lg bg-muted/50 p-2 text-sm"><option value="">{{ $t('opinions.threads.allTopics') }}</option><option v-for="f in fields" :key="f.id" :value="f.id">{{ f.icon_emoji }} {{ f.name }}</option></select>
          <select v-model="sort" :aria-label="$t('opinions.threads.sort')" class="rounded-lg bg-muted/50 p-2 text-sm"><option value="new">{{ $t('opinions.threads.new') }}</option><option value="top">{{ $t('opinions.threads.top') }}</option><option value="discussed">{{ $t('opinions.threads.discussed') }}</option></select>
          <button type="button" class="ms-auto text-sm text-muted-foreground" @click="load()">{{ $t('opinions.threads.refresh') }}</button>
        </div>
        <p v-if="error" role="alert" class="mb-4 rounded-xl bg-red-500/10 p-4 text-sm text-red-500">{{ error }}</p>
        <p v-if="loading" class="p-6 text-muted-foreground">{{ $t('opinions.threads.loading') }}</p>
        <div class="divide-y divide-border overflow-hidden rounded-xl border border-border bg-card">
          <article v-for="item in threads" :key="item.id" class="p-4 hover:bg-muted/20">
            <div class="mb-2 flex flex-wrap items-center gap-2 text-xs text-muted-foreground"><span class="font-semibold text-foreground">{{ names.get(item.subject_field_id) }}</span><span>·</span><span :title="item.author_did">{{ item.author_did.slice(-10) }}</span><span>· {{ new Date(item.created_at * 1000).toLocaleDateString() }}</span></div>
            <router-link :to="`/opinions/${item.id}`" class="flex items-start justify-between gap-5"><div class="min-w-0"><h2 class="break-words text-lg font-semibold leading-snug">{{ item.content?.title }}</h2><p class="mt-2 line-clamp-2 text-sm text-muted-foreground">{{ item.body }}</p><p v-if="item.content?.url" class="mt-1 truncate text-xs text-primary">{{ item.content.url }}</p></div><ThreadThumbnail :cid="item.content?.thumbnail_cid" :kind="item.content?.post_kind" :topic="item.subject_field_id" /></router-link>
            <div class="mt-3 flex flex-wrap items-center gap-3"><ThreadVotes :score="item.score" :vote="item.my_vote" :disabled="busy" @vote="vote(item, $event)" /><router-link :to="`/opinions/${item.id}`" class="text-xs font-medium text-muted-foreground">{{ $t('opinions.threads.comments', { count: item.comment_count }) }}</router-link><span class="text-xs text-primary">{{ $t('opinions.threads.qualified') }}</span></div>
          </article>
        </div>
        <p v-if="!loading && !threads.length" class="py-5 text-sm text-muted-foreground">{{ $t('opinions.threads.empty') }}</p>
        <button v-if="more" class="my-4 text-sm text-primary" @click="load(true)">{{ $t('opinions.threads.loadMore') }}</button>
        <article v-for="item in filteredLegacy" :key="item.id" class="mb-2 rounded-xl border border-border bg-card p-4"><router-link :to="`/opinions/legacy/${item.id}`" class="flex items-center gap-4"><ThreadThumbnail :cid="item.thumbnail_cid" kind="video" :topic="item.subject_field_id" /><div><p class="text-xs text-muted-foreground">{{ names.get(item.subject_field_id) }}</p><h2 class="font-semibold">{{ item.title }}</h2><p class="mt-1 text-xs text-muted-foreground">{{ $t('opinions.threads.legacy') }}</p></div></router-link></article>
        <OpinionExamples :examples="examples" :subject-field-id="field" />
      </main>
      <aside class="rounded-xl border border-border bg-card p-5 text-sm">
        <h2 class="font-semibold">{{ $t('opinions.threads.about') }}</h2><p class="mt-3 leading-relaxed text-muted-foreground">{{ $t('opinions.threads.aboutBody') }}</p><hr class="my-4 border-border" /><h3 class="font-semibold">{{ $t('opinions.threads.gateTitle') }}</h3><p class="mt-2 leading-relaxed text-muted-foreground">{{ $t('opinions.threads.gateBody') }}</p><p class="mt-4 text-xs leading-relaxed text-muted-foreground">{{ $t('opinions.threads.voteNote') }}</p>
      </aside>
    </div>
  </div>
</template>
