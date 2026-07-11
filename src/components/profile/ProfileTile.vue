<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'

import type { ProfileSummary } from '@/types'

import ProfileAvatar from './ProfileAvatar.vue'

const { t } = useI18n()

interface Props {
  profile: ProfileSummary
  selected?: boolean
}

const props = withDefaults(defineProps<Props>(), {
  selected: false,
})

defineEmits<{
  select: [id: string]
}>()

const lastSeen = computed(() => {
  const stamp = props.profile.last_unlocked_at
  if (!stamp) return t('profile.tile.neverOpened')
  const then = new Date(stamp).getTime()
  if (Number.isNaN(then)) return ''
  const seconds = (Date.now() - then) / 1000
  if (seconds < 60) return t('profile.tile.justNow')
  if (seconds < 3600) {
    const m = Math.floor(seconds / 60)
    return t('profile.tile.minAgo', { count: m }, m)
  }
  if (seconds < 86400) {
    const h = Math.floor(seconds / 3600)
    return t('profile.tile.hrAgo', { count: h }, h)
  }
  if (seconds < 86400 * 7) {
    const d = Math.floor(seconds / 86400)
    return t('profile.tile.daysAgo', { count: d }, d)
  }
  return new Date(stamp).toLocaleDateString()
})
</script>

<template>
  <button
    type="button"
    class="group cursor-pointer flex flex-col items-center gap-3 p-4 rounded-2xl transition-all focus:outline-none focus-visible:ring-2 focus-visible:ring-offset-2 focus-visible:ring-offset-background"
    :class="
      selected
        ? 'ring-2 ring-offset-2 ring-offset-background scale-105'
        : 'hover:bg-surface/40 hover:scale-105 active:scale-100'
    "
    :style="{
      '--tw-ring-color': profile.color,
    } as Record<string, string>"
    :aria-pressed="selected"
    @click="$emit('select', profile.id)"
  >
    <ProfileAvatar
      :avatar="profile.avatar"
      :fallback-name="profile.display_name"
      :color="profile.color"
      :size="112"
    />
    <div class="text-center">
      <div class="font-medium text-foreground line-clamp-1 max-w-[140px]">
        {{ profile.display_name }}
      </div>
      <div class="text-xs text-muted-foreground mt-0.5">{{ lastSeen }}</div>
    </div>
  </button>
</template>
