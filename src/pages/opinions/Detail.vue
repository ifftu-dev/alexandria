<script setup lang="ts">
import { ref, computed, onMounted, onBeforeUnmount, watch } from 'vue'
import { useRoute } from 'vue-router'
import { useLocalApi } from '@/composables/useLocalApi'
import VideoPlayer from '@/components/course/VideoPlayer.vue'
import ThreadVotes from '@/components/opinions/ThreadVotes.vue'
import ThreadActions from '@/components/opinions/ThreadActions.vue'
import ThreadComment from '@/components/opinions/ThreadComment.vue'
import ThreadComposer from '@/components/opinions/ThreadComposer.vue'
import type { DiscussionAccess, DiscussionItem, DiscussionAction, DiscussionContent, SubjectFieldInfo } from '@/types'
const { invoke } = useLocalApi()
const route = useRoute()
const rows = ref<DiscussionItem[]>([])
const access = ref<DiscussionAccess | null>(null)
const fields = ref<SubjectFieldInfo[]>([])
const error = ref('')
const busy = ref(false)
const loading = ref(true)
const editing = ref(false)
const comment = ref('')
const consent = ref(false)
const sort = ref('top')
let timer: ReturnType<typeof setInterval> | undefined
let generation = 0
const post = computed(() => rows.value.find(r => r.id === route.params.id))
const eligible = computed(() => !!post.value && !post.value.deleted && !!access.value?.eligible_fields.includes(post.value.subject_field_id))
const topic = computed(() => fields.value.find(f => f.id === post.value?.subject_field_id)?.name)
const comments = computed(() => rows.value.filter(r => r.parent_id === post.value?.id).sort((a,b) => sort.value === 'top' ? b.score - a.score || a.created_at - b.created_at : a.created_at - b.created_at))
async function load() {
  const ticket = ++generation
  const id = String(route.params.id)
  try {
    const all: DiscussionItem[] = []
    let batch: DiscussionItem[]
    do {
      batch = await invoke<DiscussionItem[]>('list_discussions', { threadId: id, sort: 'new', offset: all.length })
      all.push(...batch)
    } while (batch.length === 200 && ticket === generation)
    if (ticket === generation) { rows.value = all; error.value = '' }
  } catch (e) { if (ticket === generation) error.value = String(e) }
  finally { loading.value = false }
}
async function act(item: DiscussionItem, action: DiscussionAction, parent?: string, done?: (success: boolean) => void) {
  busy.value = true; error.value = ''
  try {
    await invoke('act_on_discussion', { req: { entity_id: item.id, thread_id: item.thread_id, parent_id: parent, subject_field_id: item.subject_field_id, action } })
    await load()
    done?.(true)
    return true
  } catch (e) { error.value = String(e); done?.(false); return false }
  finally { busy.value = false }
}
async function addComment() {
  if (post.value && comment.value.trim() && consent.value && await act(post.value, { kind: 'comment', body: comment.value.trim() }, post.value.id)) { comment.value = ''; consent.value = false }
}
async function edit(content: DiscussionContent) {
  if (post.value && await act(post.value, { kind: 'edit_post', content })) editing.value = false
}
watch(() => route.params.id, () => { rows.value = []; editing.value = false; comment.value = ''; void load() })
onMounted(async () => {
  try {
    const [a, f] = await Promise.all([invoke<DiscussionAccess>('discussion_access'), invoke<SubjectFieldInfo[]>('list_subject_fields')])
    access.value = a; fields.value = f; await load()
    timer = setInterval(() => { if (!busy.value && !editing.value) void load() }, 10000)
  } catch (e) { error.value = String(e); loading.value = false }
})
onBeforeUnmount(() => { generation++; if (timer) clearInterval(timer) })
</script>
<template>
  <div class="mx-auto max-w-3xl space-y-5">
    <router-link to="/opinions" class="text-sm text-muted-foreground">← {{ $t('opinions.threads.back') }}</router-link>
    <p v-if="error" role="alert" class="rounded-xl bg-red-500/10 p-4 text-sm text-red-500">{{ error }}</p>
    <p v-if="loading" class="text-muted-foreground">{{ $t('opinions.threads.loading') }}</p>
    <template v-else-if="post">
      <article class="rounded-2xl border border-border bg-card p-4 sm:p-6">
        <div class="mb-3 flex flex-wrap gap-2 text-xs text-muted-foreground"><span class="font-semibold text-primary">{{ topic }}</span><span>·</span><span :title="post.author_did">{{ post.author_did === access?.actor_did ? $t('opinions.threads.you') : post.author_did.slice(-10) }}</span><span>· {{ new Date(post.created_at * 1000).toLocaleString() }}</span><span v-if="post.edited">{{ $t('opinions.threads.edited') }}</span></div>
        <h1 class="break-words text-2xl font-bold leading-tight">{{ post.deleted ? $t('opinions.threads.deletedPost') : post.content?.title }}</h1>
        <ThreadComposer v-if="editing && post.content" class="mt-5" :initial="post.content" :busy="busy" @submit="edit" @cancel="editing = false" />
        <template v-else-if="!post.deleted">
          <p class="my-5 whitespace-pre-wrap break-words text-sm leading-7">{{ post.body }}</p>
          <a v-if="post.content?.url" :href="post.content.url" target="_blank" rel="noopener noreferrer" class="mb-5 block break-all text-sm text-primary underline">{{ post.content.url }} ↗</a>
          <VideoPlayer v-if="post.content?.video_cid" :content-cid="post.content.video_cid" :title="post.content.title" />
          <div class="mt-5 flex items-center gap-4"><ThreadVotes :score="post.score" :vote="post.my_vote" :disabled="busy" @vote="act(post, { kind: 'vote', value: $event })" /><span class="text-xs text-muted-foreground">{{ $t('opinions.threads.comments', { count: post.comment_count }) }}</span></div>
        </template>
        <ThreadActions class="mt-4" :item="post" :owner="post.author_did === access?.actor_did" :can-edit="eligible" :busy="busy" @edit="editing = true" @act="act(post, $event)" />
        <details v-if="!post.deleted" class="mt-5 text-xs text-muted-foreground"><summary class="cursor-pointer">{{ $t('opinions.threads.proof') }}</summary><p class="mt-2">{{ $t('opinions.threads.proofBody') }}</p><p v-for="id in post.credential_proof_ids" :key="id" class="mt-1 break-all font-mono">{{ id }}</p></details>
      </article>
      <section v-if="!post.deleted" class="rounded-xl border border-border bg-card p-5">
        <form v-if="eligible" class="space-y-3" @submit.prevent="addComment"><label class="block font-medium">{{ $t('opinions.threads.join') }}<textarea v-model="comment" maxlength="10000" required rows="3" class="mt-3 w-full rounded-xl border border-border bg-background p-3 text-sm font-normal" /></label><label class="flex gap-2 text-xs text-muted-foreground"><input v-model="consent" type="checkbox" />{{ $t('opinions.threads.commentDisclosure') }}</label><button type="submit" :disabled="busy || !comment.trim() || !consent" class="rounded-full bg-primary px-5 py-2 text-sm text-primary-foreground disabled:opacity-40">{{ $t('opinions.threads.comment') }}</button></form>
        <p v-else class="text-sm text-muted-foreground">{{ $t(access?.governed_fields.includes(post.subject_field_id) ? 'opinions.threads.gated' : 'opinions.threads.noPolicy') }}</p>
      </section>
      <section><div class="flex items-center justify-between"><h2 class="font-semibold">{{ $t('opinions.threads.discussion') }}</h2><select v-model="sort" :aria-label="$t('opinions.threads.sort')" class="rounded-lg border border-border bg-card p-2 text-sm"><option value="top">{{ $t('opinions.threads.top') }}</option><option value="old">{{ $t('opinions.threads.oldest') }}</option></select></div><ThreadComment v-for="item in comments" :key="item.id" :item="item" :items="rows" :actor="access?.actor_did ?? ''" :eligible="eligible" :busy="busy" :depth="1" :sort="sort" @act="act" /><p v-if="!comments.length" class="py-8 text-center text-sm text-muted-foreground">{{ $t('opinions.threads.noComments') }}</p></section>
    </template>
    <p v-else class="text-muted-foreground">{{ $t('opinions.threads.notFound') }}</p>
  </div>
</template>
