<script setup lang="ts">
import { ref, computed } from 'vue'
import ThreadVotes from './ThreadVotes.vue'
import ThreadActions from './ThreadActions.vue'
import ThreadMeta from './ThreadMeta.vue'
import { AppButton } from '@/components/ui'
import type { DiscussionItem, DiscussionAction } from '@/types'
const props = defineProps<{ item: DiscussionItem; items: DiscussionItem[]; actor: string; eligible: boolean; busy: boolean; depth: number; sort: string }>()
const emit = defineEmits<{ act: [item: DiscussionItem, action: DiscussionAction, parent?: string, done?: (success: boolean) => void] }>()
const collapsed = ref(false)
const mode = ref<'reply' | 'edit' | null>(null)
const body = ref('')
const consent = ref(false)
const children = computed(() => props.items.filter(c => c.parent_id === props.item.id).sort((a, b) => props.sort === 'top' ? b.score - a.score || a.created_at - b.created_at : a.created_at - b.created_at))
function edit() { mode.value = 'edit'; body.value = props.item.body; consent.value = false }
function reply() { mode.value = 'reply'; body.value = ''; consent.value = false }
function submit() {
  if (!body.value.trim() || !consent.value) return
  emit('act', props.item, { kind: mode.value === 'edit' ? 'edit_comment' : 'comment', body: body.value.trim() }, mode.value === 'reply' ? props.item.id : undefined, success => { if (success) { mode.value = null; body.value = ''; consent.value = false } })
}
</script>
<template>
  <article class="mt-6 min-w-0 border-s border-border/50" :class="depth > 2 ? 'ps-2 sm:ps-4' : 'ps-3 sm:ps-4'">
    <div class="flex items-center gap-2"><button type="button" :aria-expanded="!collapsed" :aria-label="$t('opinions.threads.collapse')" class="flex h-6 w-6 shrink-0 items-center justify-center rounded text-xs text-muted-foreground hover:bg-muted" @click="collapsed = !collapsed">{{ collapsed ? '+' : '−' }}</button><ThreadMeta :author="item.author_did" :created-at="item.created_at" :own="item.author_did === actor" :edited="item.edited" /></div>
    <template v-if="!collapsed">
      <p class="mt-3 whitespace-pre-wrap break-words text-sm leading-relaxed" :class="item.deleted ? 'italic text-muted-foreground' : ''">{{ item.deleted ? $t('opinions.threads.deletedComment') : item.body }}</p>
      <div v-if="!item.deleted" class="mt-3 flex flex-wrap items-center gap-3"><ThreadVotes :score="item.score" :vote="item.my_vote" :disabled="busy" @vote="emit('act', item, { kind: 'vote', value: $event })" /><button v-if="depth < 8" type="button" :disabled="!eligible || busy" class="text-xs font-medium text-muted-foreground disabled:opacity-40" @click="reply">{{ $t('opinions.threads.reply') }}</button>
      <ThreadActions class="ms-auto" :item="item" :owner="actor === item.author_did" :can-edit="eligible" :busy="busy" @edit="edit" @act="emit('act', item, $event)" /></div>
      <form v-if="mode" class="mt-3 space-y-3" @submit.prevent="submit"><textarea v-model="body" maxlength="10000" required rows="3" :aria-label="$t('opinions.threads.commentLabel')" class="input w-full text-sm" /><label class="flex gap-2 text-xs text-muted-foreground"><input v-model="consent" type="checkbox" />{{ $t('opinions.threads.commentDisclosure') }}</label><div class="flex gap-3"><AppButton type="submit" size="sm" :disabled="busy || !body.trim() || !consent">{{ $t('opinions.threads.submit') }}</AppButton><button type="button" class="text-xs" @click="mode = null">{{ $t('common.actions.cancel') }}</button></div></form>
      <ThreadComment v-for="child in children" :key="child.id" :item="child" :items="items" :actor="actor" :eligible="eligible" :busy="busy" :depth="depth + 1" :sort="sort" @act="(item, action, parent, done) => emit('act', item, action, parent, done)" />
    </template>
  </article>
</template>
