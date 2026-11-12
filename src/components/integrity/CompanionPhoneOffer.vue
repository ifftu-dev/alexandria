<script setup lang="ts">
/**
 * Desktop: the one-time code the learner's phone joins a Sentinel session
 * with. Shown while `useCompanionPhone().pairPhone` waits; the learner can
 * continue without the phone at any point.
 */
import { ref } from 'vue'
import { AppButton } from '@/components/ui'
import { useCompanionPhone } from '@/composables/useCompanionPhone'

const companion = useCompanionPhone()
const copied = ref(false)

async function copy(code: string): Promise<void> {
  try {
    await navigator.clipboard.writeText(code)
    copied.value = true
  } catch {
    copied.value = false
  }
}
</script>

<template>
  <div v-if="companion.offer.value" class="space-y-3 rounded-lg border border-border bg-card p-4 text-sm" data-testid="companion-offer">
    <h2 class="font-semibold text-foreground">{{ $t('sentinel.companion.offerTitle') }}</h2>
    <p class="text-muted-foreground">{{ $t('sentinel.companion.offerBody') }}</p>
    <code class="block break-all rounded bg-muted p-2 font-mono text-xs text-foreground" data-testid="companion-code">{{ companion.offer.value.code }}</code>
    <p class="text-muted-foreground">{{ $t('sentinel.companion.waiting') }}</p>
    <div class="flex gap-2">
      <AppButton size="sm" variant="outline" @click="copy(companion.offer.value.code)">
        {{ copied ? $t('sentinel.companion.copied') : $t('sentinel.companion.copy') }}
      </AppButton>
      <AppButton size="sm" variant="secondary" data-testid="companion-skip" @click="companion.skip()">
        {{ $t('sentinel.companion.skip') }}
      </AppButton>
    </div>
  </div>
  <p v-else-if="companion.error.value" class="text-sm text-error" data-testid="companion-error">
    {{ $t('sentinel.companion.failed', { reason: companion.error.value }) }}
  </p>
</template>
