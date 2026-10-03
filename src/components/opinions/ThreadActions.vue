<script setup lang="ts">
import { ref } from 'vue'
import type { DiscussionAction, DiscussionItem } from '@/types'
const props = defineProps<{ item: DiscussionItem; owner: boolean; canEdit: boolean; busy: boolean }>()
const emit = defineEmits<{ act: [action: DiscussionAction]; edit: [] }>()
const menu = ref<'delete' | 'report' | null>(null)
const reason = ref('spam')
function submit() {
  emit('act', menu.value === 'delete' ? { kind: 'delete' } : { kind: 'report', reason: reason.value })
  menu.value = null
}
</script>
<template>
  <div class="thread-actions text-xs text-muted-foreground">
    <div class="flex gap-4">
      <button v-if="owner && !item.deleted" type="button" :disabled="!canEdit || busy" class="disabled:opacity-40" @click="emit('edit')">{{ $t('opinions.threads.edit') }}</button>
      <button v-if="owner && !item.deleted" type="button" :disabled="busy" @click="menu = menu === 'delete' ? null : 'delete'">{{ $t('opinions.threads.delete') }}</button>
      <button v-if="!item.deleted" type="button" :disabled="busy || item.reported" @click="menu = menu === 'report' ? null : 'report'">{{ $t(item.reported ? 'opinions.threads.reported' : 'opinions.threads.report') }}</button>
    </div>
    <form v-if="menu" class="mt-3 space-y-3 rounded-xl border border-border p-4" @submit.prevent="submit">
      <p>{{ $t(menu === 'delete' ? 'opinions.threads.deleteConfirm' : 'opinions.threads.reportDisclosure') }}</p>
      <select v-if="menu === 'report'" v-model="reason" :aria-label="$t('opinions.threads.reportReason')" class="rounded-lg border border-border bg-background p-2"><option v-for="r in ['spam', 'harassment', 'misinformation', 'off_topic', 'other']" :key="r" :value="r">{{ $t(`opinions.threads.${r}`) }}</option></select>
      <div class="flex gap-4"><button type="submit" :disabled="props.busy" class="font-medium text-primary">{{ $t(menu === 'delete' ? 'opinions.threads.delete' : 'opinions.threads.submitReport') }}</button><button type="button" @click="menu = null">{{ $t('common.actions.cancel') }}</button></div>
    </form>
  </div>
</template>

<style scoped>
@media (pointer: coarse) { .thread-actions button { min-height: 44px; } }
</style>
