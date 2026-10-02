<script setup lang="ts">
import { ref, computed } from 'vue'
import { AppButton } from '@/components/ui'
import { useLocalApi } from '@/composables/useLocalApi'
import type { DiscussionContent } from '@/types'
const props = defineProps<{ initial?: DiscussionContent; busy?: boolean }>()
const emit = defineEmits<{ submit: [content: DiscussionContent]; cancel: [] }>()
const { invoke } = useLocalApi()
const kind = ref<DiscussionContent['post_kind']>(props.initial?.post_kind ?? 'text')
const title = ref(props.initial?.title ?? '')
const body = ref(props.initial?.body ?? '')
const link = ref(props.initial?.url ?? '')
const video = ref(props.initial?.video_cid ?? null)
const thumbnail = ref(props.initial?.thumbnail_cid ?? null)
const uploading = ref(false)
const consent = ref(false)
const error = ref('')
const valid = computed(() => title.value.trim() && (kind.value === 'text' ? body.value.trim() : kind.value === 'link' ? /^https?:\/\//.test(link.value) : video.value) && consent.value && !uploading.value && !props.busy)
async function upload(event: Event, mediaKind: string) {
  const file = (event.target as HTMLInputElement).files?.[0]
  if (!file) return
  uploading.value = true; error.value = ''
  try {
    if (file.size > 25 * 1024 * 1024) throw new Error('Maximum file size is 25 MB')
    const result = await invoke<{ hash: string }>('discussion_add_media', { data: Array.from(new Uint8Array(await file.arrayBuffer())), mediaKind })
    if (mediaKind === 'video') video.value = result.hash
    else thumbnail.value = result.hash
  } catch (e) { error.value = String(e) }
  finally { uploading.value = false }
}
function submit() {
  if (!valid.value) return
  emit('submit', { title: title.value.trim(), body: body.value.trim(), post_kind: kind.value, url: kind.value === 'link' ? link.value.trim() : null, video_cid: kind.value === 'video' ? video.value : null, thumbnail_cid: thumbnail.value })
}
</script>
<template>
  <form class="space-y-4" @submit.prevent="submit">
    <div class="flex gap-2" role="group" :aria-label="$t('opinions.threads.postType')">
      <button v-for="k in (['text', 'link', 'video'] as const)" :key="k" type="button" class="rounded-lg px-3 py-1.5 text-xs font-medium" :class="kind === k ? 'bg-primary/10 text-primary' : 'bg-muted/50 text-muted-foreground'" :aria-pressed="kind === k" @click="kind = k">{{ $t(`opinions.threads.${k}`) }}</button>
    </div>
    <label class="block text-sm">{{ $t('opinions.threads.titleLabel') }}<input v-model="title" required maxlength="300" class="input mt-1 w-full" /></label>
    <label v-if="kind === 'link'" class="block text-sm">{{ $t('opinions.threads.linkLabel') }}<input v-model="link" required type="url" placeholder="https://" class="input mt-1 w-full" /></label>
    <label class="block text-sm">{{ $t('opinions.threads.bodyLabel') }}<textarea v-model="body" maxlength="20000" rows="7" :required="kind === 'text'" class="input mt-1 w-full" /></label>
    <label class="flex items-start gap-2 text-sm text-muted-foreground"><input v-model="consent" type="checkbox" class="mt-1" />{{ $t('opinions.threads.publishDisclosure') }}</label>
    <label v-if="kind === 'video'" class="block text-sm">{{ $t('opinions.threads.videoLabel') }}<input type="file" accept="video/mp4,video/webm" :disabled="!consent || uploading" class="mt-2 block" @change="upload($event, 'video')" /><span v-if="video" class="text-primary">{{ $t('opinions.threads.uploaded') }}</span></label>
    <label class="block text-sm">{{ $t('opinions.threads.thumbnailLabel') }}<input type="file" accept="image/png,image/jpeg,image/webp" :disabled="!consent || uploading" class="mt-2 block" @change="upload($event, 'image')" /><span v-if="thumbnail" class="text-primary">{{ $t('opinions.threads.uploaded') }}</span></label>
    <p v-if="uploading" role="status" class="text-sm text-muted-foreground">{{ $t('opinions.threads.uploading') }}</p>
    <p v-if="error" role="alert" class="text-sm text-red-500">{{ error }}</p>
    <div class="flex gap-3"><AppButton type="submit" size="sm" :disabled="!valid">{{ $t(initial ? 'opinions.threads.save' : 'opinions.threads.publish') }}</AppButton><button type="button" class="px-4 text-sm" @click="emit('cancel')">{{ $t('common.actions.cancel') }}</button></div>
  </form>
</template>
