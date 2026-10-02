<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { useRouter } from 'vue-router'
import { useLocalApi } from '@/composables/useLocalApi'
import ThreadComposer from '@/components/opinions/ThreadComposer.vue'
import type { DiscussionAccess, DiscussionContent, SubjectFieldInfo } from '@/types'
const { invoke } = useLocalApi()
const router = useRouter()
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
  } catch (e) { error.value = String(e) }
})
async function publish(content: DiscussionContent) {
  busy.value = true; error.value = ''
  try {
    const id = await invoke<string>('act_on_discussion', { req: { subject_field_id: field.value, action: { kind: 'post', content } } })
    await router.push(`/opinions/${id}`)
  } catch (e) { error.value = String(e) }
  finally { busy.value = false }
}
</script>
<template>
  <div class="mx-auto max-w-3xl space-y-6">
    <router-link to="/opinions" class="text-sm text-muted-foreground">← {{ $t('opinions.threads.back') }}</router-link>
    <h1 class="text-3xl font-bold">{{ $t('opinions.threads.create') }}</h1>
    <p v-if="error" role="alert" class="text-red-500">{{ error }}</p>
    <p v-if="access && !fields.length" class="rounded-xl border border-border bg-card p-5 text-muted-foreground">{{ $t(access.governed_fields.length ? 'opinions.threads.gated' : 'opinions.threads.noPolicy') }}</p>
    <template v-if="fields.length">
      <label class="block text-sm">{{ $t('opinions.threads.topic') }}<select v-model="field" class="mt-2 w-full rounded-xl border border-border bg-card p-3"><option v-for="f in fields" :key="f.id" :value="f.id">{{ f.icon_emoji }} {{ f.name }}</option></select></label>
      <ThreadComposer :busy="busy" @submit="publish" @cancel="router.push('/opinions')" />
    </template>
  </div>
</template>
