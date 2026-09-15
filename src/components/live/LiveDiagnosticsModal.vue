<script setup lang="ts">
import { ref, watch } from 'vue'
import { AppButton, AppModal } from '@/components/ui'

interface Props {
  open: boolean
  diagnostics: Record<string, unknown> | null
}

const props = defineProps<Props>()
const emit = defineEmits<{ close: []; refresh: [] }>()
const copied = ref(false)
const manualCopy = ref(false)

watch(() => props.open, () => {
  copied.value = false
  manualCopy.value = false
})

async function copyJson() {
  if (!props.diagnostics) return
  try {
    await navigator.clipboard.writeText(JSON.stringify(props.diagnostics, null, 2))
    copied.value = true
    window.setTimeout(() => { copied.value = false }, 2000)
  } catch {
    manualCopy.value = true
  }
}
</script>

<template>
  <AppModal :open="open" :title="$t('interviews.live.diagnosticsTitle')" max-width="42rem" @close="emit('close')">
    <p class="text-sm text-muted-foreground">{{ $t('interviews.live.diagnosticsHint') }}</p>
    <pre v-if="diagnostics && !manualCopy" class="mt-4 max-h-[50vh] overflow-auto rounded-lg bg-muted p-3 font-mono text-xs text-foreground whitespace-pre-wrap break-all select-all">{{ JSON.stringify(diagnostics, null, 2) }}</pre>
    <textarea
      v-else-if="diagnostics"
      readonly
      rows="14"
      :value="JSON.stringify(diagnostics, null, 2)"
      class="mt-4 w-full resize-none rounded-lg border border-border bg-muted p-3 font-mono text-xs text-foreground"
      @focus="($event.target as HTMLTextAreaElement).select()"
    />
    <p v-else class="mt-4 rounded-lg border border-border bg-muted/30 p-4 text-sm text-muted-foreground">{{ $t('interviews.live.diagnosticsEmpty') }}</p>

    <template #footer>
      <div class="flex flex-wrap items-center gap-2">
        <AppButton variant="outline" size="sm" @click="emit('refresh')">{{ $t('interviews.common.refresh') }}</AppButton>
        <AppButton v-if="diagnostics" variant="outline" size="sm" @click="copyJson">{{ copied ? $t('interviews.common.copied') : manualCopy ? $t('tutoring.diagnostics.selectManual') : $t('tutoring.diagnostics.copyJson') }}</AppButton>
        <span class="flex-1" />
        <AppButton size="sm" @click="emit('close')">{{ $t('interviews.common.close') }}</AppButton>
      </div>
    </template>
  </AppModal>
</template>
