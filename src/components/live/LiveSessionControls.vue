<script setup lang="ts">
interface Props {
  audioEnabled: boolean
  videoEnabled: boolean
  screenSharing?: boolean
  micLevel?: number
  incomingLevel?: number
  canShareScreen?: boolean
  endLabel?: string
}

withDefaults(defineProps<Props>(), {
  screenSharing: false,
  micLevel: 0,
  incomingLevel: 0,
  canShareScreen: true,
  endLabel: 'Leave',
})

const emit = defineEmits<{
  toggleAudio: []
  toggleVideo: []
  toggleScreenShare: []
  end: []
}>()
</script>

<template>
  <div class="flex flex-wrap items-center justify-center gap-2 border-t border-border bg-card/95 px-3 py-3 backdrop-blur-sm">
    <button
      class="relative grid h-10 w-10 place-items-center rounded-full border transition-colors"
      :class="audioEnabled ? 'border-border bg-muted text-foreground hover:bg-border/70' : 'border-destructive/40 bg-destructive/10 text-destructive hover:bg-destructive/20'"
      :title="audioEnabled ? 'Mute microphone' : 'Unmute microphone'"
      @click="emit('toggleAudio')"
    >
      <span class="sr-only">{{ audioEnabled ? 'Mute microphone' : 'Unmute microphone' }}</span>
      <svg class="h-5 w-5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
        <path stroke-linecap="round" stroke-linejoin="round" d="M12 18.75a6 6 0 006-6v-1.5m-12 1.5a6 6 0 006 6m6 0v3.75m-3.75 0h7.5M12 15.75a3 3 0 01-3-3V4.5a3 3 0 116 0v8.25a3 3 0 01-3 3z" />
        <path v-if="!audioEnabled" stroke-linecap="round" d="M4 4l16 16" />
      </svg>
      <span
        v-if="audioEnabled && micLevel > 0.02"
        class="pointer-events-none absolute inset-0 rounded-full border-2 border-success transition-transform"
        :style="{ transform: `scale(${1 + Math.min(micLevel, 1) * 0.12})`, opacity: `${0.35 + micLevel * 0.65}` }"
      />
    </button>

    <button
      class="relative grid h-10 w-10 place-items-center rounded-full border transition-colors"
      :class="videoEnabled ? 'border-border bg-muted text-foreground hover:bg-border/70' : 'border-destructive/40 bg-destructive/10 text-destructive hover:bg-destructive/20'"
      :title="videoEnabled ? 'Turn camera off' : 'Turn camera on'"
      @click="emit('toggleVideo')"
    >
      <span class="sr-only">{{ videoEnabled ? 'Turn camera off' : 'Turn camera on' }}</span>
      <svg class="h-5 w-5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
        <path stroke-linecap="round" stroke-linejoin="round" d="M15 10l4.55-2.28A1 1 0 0121 8.62v6.76a1 1 0 01-1.45.9L15 14M5 18h8a2 2 0 002-2V8a2 2 0 00-2-2H5a2 2 0 00-2 2v8a2 2 0 002 2z" />
        <path v-if="!videoEnabled" stroke-linecap="round" d="M3 3l18 18" />
      </svg>
    </button>

    <button
      v-if="canShareScreen"
      class="relative grid h-10 w-10 place-items-center rounded-full border transition-colors"
      :class="screenSharing ? 'border-primary bg-primary/10 text-primary' : 'border-border bg-muted text-foreground hover:bg-border/70'"
      :title="screenSharing ? 'Stop sharing' : 'Share screen'"
      @click="emit('toggleScreenShare')"
    >
      <span class="sr-only">{{ screenSharing ? 'Stop sharing' : 'Share screen' }}</span>
      <svg class="h-5 w-5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
        <path stroke-linecap="round" stroke-linejoin="round" d="M9 17.25v1a3 3 0 01-.88 2.13L7.5 21h9l-.62-.62A3 3 0 0115 18.25v-1m6-12V15a2.25 2.25 0 01-2.25 2.25H5.25A2.25 2.25 0 013 15V5.25A2.25 2.25 0 015.25 3h13.5A2.25 2.25 0 0121 5.25z" />
      </svg>
    </button>

    <div v-if="incomingLevel > 0.02" class="flex h-10 items-center gap-1.5 rounded-full border border-border bg-muted px-3" :title="$t('interviews.live.incomingLevel')">
      <svg class="h-4 w-4 text-primary" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
        <path stroke-linecap="round" stroke-linejoin="round" d="M19.1 5.64a9 9 0 010 12.72M16.46 8.3a5.25 5.25 0 010 7.4M6.75 8.25l4.72-4.72a.75.75 0 011.28.53v15.88a.75.75 0 01-1.28.53l-4.72-4.72H4.5a2 2 0 01-1.93-1.35A9 9 0 012.25 12c0-.83.11-1.63.32-2.4A2 2 0 014.5 8.25h2.25z" />
      </svg>
      <div class="h-1.5 w-12 overflow-hidden rounded-full bg-border">
        <div class="h-full rounded-full bg-primary transition-[width]" :style="{ width: `${Math.min(incomingLevel, 1) * 100}%` }" />
      </div>
    </div>

    <div class="mx-1 h-6 w-px bg-border" />
    <button class="btn btn-danger h-10 rounded-full px-5 text-sm" @click="emit('end')">{{ endLabel }}</button>
  </div>
</template>
