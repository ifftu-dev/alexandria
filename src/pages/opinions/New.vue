<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { useRouter, useRoute } from 'vue-router'
import { useLocalApi } from '@/composables/useLocalApi'
import ThreadComposer from '@/components/opinions/ThreadComposer.vue'
import type { DiscussionAccess, DiscussionContent, SubjectFieldInfo, SeedDraft } from '@/types'
const { invoke } = useLocalApi()
const router = useRouter()
const route = useRoute()
const seedContent = ref<DiscussionContent | undefined>(undefined)
const ready = ref(false)
const fields = ref<SubjectFieldInfo[]>([])
const access = ref<DiscussionAccess | null>(null)
const field = ref('')
const error = ref('')
const busy = ref(false)
onMounted(async () => {
  try {
    const [all, a] = await Promise.all([invoke<SubjectFieldInfo[]>('list_subject_fields'), invoke<DiscussionAccess>('discussion_access')])
    access.value = a
    fields.value = all.filter(f => a.eligible_fields.includes(f.id))
    field.value = fields.value[0]?.id ?? ''
    if (typeof route.query.seedDraft === 'string') {
      const drafts = await invoke<SeedDraft[]>('dev_seed_drafts')
      const draft = drafts.find(d => d.id === route.query.seedDraft)
      if (draft) seedContent.value = { title: draft.title, body: draft.body, post_kind: 'text', url: null, video_cid: null, thumbnail_cid: null }
    }
  } catch (e) { error.value = String(e) }
  finally { ready.value = true }
})
async function publish(content: DiscussionContent) {
  busy.value = true; error.value = ''
  try {
    const id = await invoke<string>('act_on_discussion', { req: { subject_field_id: field.value, action: { kind: 'post', content } } })
    await router.push(`/discussions/${id}`)
  } catch (e) { error.value = String(e) }
  finally { busy.value = false }
}
</script>
<template>
  <div class="mx-auto max-w-3xl space-y-6">
    <router-link to="/discussions" class="text-sm text-muted-foreground">← {{ $t('opinions.threads.back') }}</router-link>
    <h1 class="text-xl font-bold">{{ $t('opinions.threads.create') }}</h1>
    <p v-if="error" role="alert" class="text-red-500">{{ error }}</p>
    <p v-if="access && !fields.length" class="rounded-xl bg-card shadow-sm p-5 text-muted-foreground">{{ $t(access.governed_fields.length ? 'opinions.threads.gated' : 'opinions.threads.noPolicy') }}</p>
    <template v-if="fields.length">
      <label class="block text-sm">{{ $t('opinions.threads.topic') }}<select v-model="field" class="mt-2 w-full rounded-xl bg-card shadow-sm p-3"><option v-for="f in fields" :key="f.id" :value="f.id">{{ f.icon_emoji }} {{ f.name }}</option></select></label>
      <ThreadComposer v-if="ready" :initial="seedContent" :submit-label="$t('opinions.threads.publish')" :busy="busy" @submit="publish" @cancel="router.push('/discussions')" />
    </template>
  </div>
</template>
