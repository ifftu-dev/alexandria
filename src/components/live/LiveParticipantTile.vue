<script setup lang="ts">
interface Props {
  name: string
  initials: string
  videoSrc?: string | null
  connected?: boolean
  muted?: boolean
  self?: boolean
  label?: string
  compact?: boolean
}

withDefaults(defineProps<Props>(), {
  videoSrc: null,
  connected: true,
  muted: false,
  self: false,
  label: '',
  compact: false,
})
</script>

<template>
  <article
    class="group relative isolate overflow-hidden border border-border bg-card shadow-sm"
    :class="compact ? 'min-h-32 rounded-xl' : 'min-h-52 rounded-2xl'"
  >
    <slot name="video">
      <img
        v-if="videoSrc"
        :src="videoSrc"
        :alt="name"
        class="absolute inset-0 h-full w-full object-cover"
      />
      <div v-else class="absolute inset-0 grid place-items-center bg-gradient-to-br from-muted/90 to-background">
        <div class="flex h-16 w-16 items-center justify-center rounded-full border border-border bg-card text-xl font-semibold text-foreground shadow-sm">
          {{ initials }}
        </div>
      </div>
    </slot>

    <div class="absolute inset-x-0 bottom-0 flex items-end justify-between gap-2 bg-gradient-to-t from-black/75 via-black/20 to-transparent px-3 pb-3 pt-10">
      <div class="min-w-0">
        <div class="flex items-center gap-1.5">
          <span class="h-1.5 w-1.5 shrink-0 rounded-full" :class="connected ? 'bg-success' : 'bg-warning'" />
          <p class="truncate text-xs font-semibold text-white">{{ name }}</p>
          <span v-if="self" class="text-[0.65rem] text-white/70">{{ $t('interviews.live.you') }}</span>
        </div>
        <p v-if="label" class="mt-0.5 truncate text-[0.65rem] text-white/70">{{ label }}</p>
      </div>
      <div
        class="grid h-7 w-7 shrink-0 place-items-center rounded-full bg-black/45 text-white backdrop-blur-sm"
        :title="muted ? 'Microphone muted' : 'Microphone on'"
      >
        <svg v-if="!muted" class="h-3.5 w-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
          <path stroke-linecap="round" stroke-linejoin="round" d="M12 18.75a6 6 0 006-6v-1.5m-12 1.5a6 6 0 006 6m6 0v3.75m-3.75 0h7.5M12 15.75a3 3 0 01-3-3V4.5a3 3 0 116 0v8.25a3 3 0 01-3 3z" />
        </svg>
        <svg v-else class="h-3.5 w-3.5 text-red-300" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
          <path stroke-linecap="round" d="M4 4l16 16M9 9v3.75a3 3 0 004.4 2.65M15 10.5v-6a3 3 0 00-5.6-1.5M6 11.25v1.5a6 6 0 009.5 4.87M18 11.25v1.5a6 6 0 01-.6 2.62M12 18.75v3.75m-3.75 0h7.5" />
        </svg>
      </div>
    </div>
  </article>
</template>
