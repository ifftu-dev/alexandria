<script setup lang="ts">
import { ref, computed, onMounted, onBeforeUnmount, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useLocalApi } from '@/composables/useLocalApi'
import VideoPlayer from '@/components/course/VideoPlayer.vue'
import ThreadVotes from '@/components/opinions/ThreadVotes.vue'
import ThreadMeta from '@/components/opinions/ThreadMeta.vue'
import { AppButton } from '@/components/ui'
import ThreadActions from '@/components/opinions/ThreadActions.vue'
import ThreadComment from '@/components/opinions/ThreadComment.vue'
import ThreadComposer from '@/components/opinions/ThreadComposer.vue'
import type { DiscussionAccess, DiscussionItem, DiscussionAction, DiscussionContent, OpinionRow, SubjectFieldInfo } from '@/types'
const { invoke } = useLocalApi()
const route = useRoute()
const router = useRouter()
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
    if (ticket === generation && !all.length) {
      const legacy = await invoke<OpinionRow | null>('get_opinion', { opinionId: id })
      if (legacy && ticket === generation) { await router.replace(`/discussions/legacy/${id}`); return }
    }
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
  <div class="mx-auto max-w-4xl">
    <router-link to="/discussions" class="mb-5 inline-flex items-center gap-2 text-xs text-muted-foreground transition-colors hover:text-foreground">← {{ $t('opinions.threads.back') }}</router-link>
    <p v-if="error" role="alert" class="mb-4 rounded-lg bg-error/10 p-3 text-sm text-error">{{ error }}</p>
    <p v-if="loading" class="py-8 text-sm text-muted-foreground">{{ $t('opinions.threads.loading') }}</p>
    <article v-else-if="post" class="overflow-hidden rounded-xl bg-card shadow-sm">
      <div class="p-5 sm:p-7">
        <ThreadMeta :author="post.author_did" :created-at="post.created_at" :topic="topic" :own="post.author_did === access?.actor_did" :edited="post.edited" />
        <h1 class="mt-4 break-words text-xl font-bold leading-snug sm:text-2xl">{{ post.deleted ? $t('opinions.threads.deletedPost') : post.content?.title }}</h1>
        <ThreadComposer v-if="editing && post.content" class="mt-6" :initial="post.content" :busy="busy" @submit="edit" @cancel="editing = false" />
        <template v-else-if="!post.deleted">
          <p class="my-5 whitespace-pre-wrap break-words text-sm leading-7 text-foreground/90">{{ post.body }}</p>
          <a v-if="post.content?.url" :href="post.content.url" target="_blank" rel="noopener noreferrer" class="mb-5 block break-all rounded-lg bg-muted/40 p-3 text-sm text-primary hover:underline">{{ post.content.url }} ↗</a>
          <div v-if="post.content?.video_cid" class="my-5 overflow-hidden rounded-lg"><VideoPlayer :content-cid="post.content.video_cid" :title="post.content.title" /></div>
        </template>
        <div class="mt-5 flex flex-wrap items-center gap-x-5 gap-y-3">
          <ThreadVotes v-if="!post.deleted" :score="post.score" :vote="post.my_vote" :disabled="busy" @vote="act(post, { kind: 'vote', value: $event })" />
          <span class="text-xs text-muted-foreground">{{ $t('opinions.threads.comments', { count: post.comment_count }) }}</span>
          <ThreadActions class="ms-auto" :item="post" :owner="post.author_did === access?.actor_did" :can-edit="eligible" :busy="busy" @edit="editing = true" @act="act(post, $event)" />
        </div>
        <details v-if="!post.deleted" class="mt-5 border-t border-border/50 pt-3 text-xs text-muted-foreground"><summary class="cursor-pointer">{{ $t('opinions.threads.proof') }}</summary><p class="mt-2 leading-relaxed">{{ $t('opinions.threads.proofBody') }}</p><p v-for="id in post.credential_proof_ids" :key="id" class="mt-1 break-all font-mono">{{ id }}</p></details>
      </div>
      <section class="border-t border-border/60 p-5 sm:p-7">
        <div class="mb-5 flex flex-wrap items-center justify-between gap-3"><h2 class="text-sm font-semibold">{{ $t('opinions.threads.discussion') }} <span class="ms-1 font-normal text-muted-foreground">{{ post.comment_count }}</span></h2><select v-model="sort" :aria-label="$t('opinions.threads.sort')" class="rounded-lg border border-input bg-background px-3 py-2 text-xs"><option value="top">{{ $t('opinions.threads.top') }}</option><option value="old">{{ $t('opinions.threads.oldest') }}</option></select></div>
        <form v-if="eligible" class="mb-6 space-y-3" @submit.prevent="addComment">
          <textarea v-model="comment" maxlength="10000" required rows="3" :placeholder="$t('opinions.threads.join')" :aria-label="$t('opinions.threads.commentLabel')" class="input w-full resize-y text-sm" />
          <div class="flex flex-wrap items-start justify-between gap-3"><label class="flex max-w-lg items-start gap-2 text-xs leading-relaxed text-muted-foreground"><input v-model="consent" type="checkbox" class="mt-0.5" />{{ $t('opinions.threads.commentDisclosure') }}</label><AppButton type="submit" size="sm" :disabled="busy || !comment.trim() || !consent">{{ $t('opinions.threads.comment') }}</AppButton></div>
        </form>
        <p v-else-if="!post.deleted" class="mb-5 rounded-lg bg-muted/40 px-3 py-2.5 text-xs leading-relaxed text-muted-foreground">{{ $t(access?.governed_fields.includes(post.subject_field_id) ? 'opinions.threads.gated' : 'opinions.threads.noPolicy') }}</p>
        <ThreadComment v-for="item in comments" :key="item.id" :item="item" :items="rows" :actor="access?.actor_did ?? ''" :eligible="eligible" :busy="busy" :depth="1" :sort="sort" @act="act" />
        <p v-if="!comments.length" class="py-6 text-center text-xs text-muted-foreground">{{ $t('opinions.threads.noComments') }}</p>
      </section>
    </article>
    <p v-else class="text-sm text-muted-foreground">{{ $t('opinions.threads.notFound') }}</p>
  </div>
</template>
