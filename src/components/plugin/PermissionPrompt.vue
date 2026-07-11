<script setup lang="ts">
/**
 * Browser-style capability consent modal. The host shows this when a plugin
 * requests a capability the user hasn't yet granted. Decisions flow back
 * through the `decide` event.
 *
 * Phase 1 scope: one capability at a time, three scopes (once / session /
 * always) + deny. Future phases can add scope refinement (e.g. specific
 * microphone device) without breaking this contract.
 */

import { onMounted } from 'vue'
import { useI18n } from 'vue-i18n'
import type { PluginCapability } from '@/types'
import { AppButton } from '@/components/ui'
import { useDisplayNames } from '@/composables/useDisplayNames'

const props = defineProps<{
  pluginName: string
  authorDid: string
  capability: PluginCapability
  reason: string
}>()

const { t } = useI18n()
const { displayName, ensureNames } = useDisplayNames()
onMounted(() => void ensureNames([props.authorDid]))

const emit = defineEmits<{
  (e: 'decide', scope: 'once' | 'session' | 'always' | 'deny'): void
}>()

function capabilityLabel(c: PluginCapability): string {
  switch (c) {
    case 'microphone':
      return t('plugins.permission.capabilities.microphone')
    case 'camera':
      return t('plugins.permission.capabilities.camera')
    case 'midi':
      return t('plugins.permission.capabilities.midi')
    case 'fullscreen':
      return t('plugins.permission.capabilities.fullscreen')
    case 'clipboard':
      return t('plugins.permission.capabilities.clipboard')
    case 'storage':
      return t('plugins.permission.capabilities.storage')
    case 'ml_inference':
      return t('plugins.permission.capabilities.mlInference')
  }
}

</script>

<template>
  <div
    class="fixed inset-0 z-[60] flex items-center justify-center bg-black/40 backdrop-blur-sm"
    role="dialog"
    aria-modal="true"
  >
    <div
      class="w-full max-w-md rounded-2xl border border-border bg-background p-6 shadow-2xl"
    >
      <h2 class="text-base font-semibold text-foreground">
        {{ $t('plugins.permission.heading', { name: pluginName, action: capabilityLabel(capability) }) }}
      </h2>

      <p v-if="reason" class="mt-2 text-sm text-muted-foreground">
        "{{ reason }}"
      </p>

      <div class="mt-4 rounded-lg border border-border/60 bg-muted/20 p-3">
        <div class="flex items-start gap-2 text-xs text-muted-foreground">
          <svg class="h-4 w-4 flex-shrink-0 text-amber-500" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
            <path stroke-linecap="round" stroke-linejoin="round" d="M12 9v2m0 4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z" />
          </svg>
          <div class="space-y-1">
            <p>{{ $t('plugins.permission.sandbox') }}</p>
            <p>
              {{ $t('plugins.permission.madeBy') }}
              <span class="font-medium">{{ displayName(authorDid) }}</span>
            </p>
            <p>{{ $t('plugins.permission.noAccess') }}</p>
          </div>
        </div>
      </div>

      <div class="mt-5 space-y-2">
        <AppButton variant="primary" class="w-full justify-center" @click="emit('decide', 'always')">
          {{ $t('plugins.permission.always') }}
        </AppButton>
        <AppButton variant="secondary" class="w-full justify-center" @click="emit('decide', 'session')">
          {{ $t('plugins.permission.session') }}
        </AppButton>
        <AppButton variant="secondary" class="w-full justify-center" @click="emit('decide', 'once')">
          {{ $t('plugins.permission.once') }}
        </AppButton>
        <AppButton variant="ghost" class="w-full justify-center" @click="emit('decide', 'deny')">
          {{ $t('plugins.permission.deny') }}
        </AppButton>
      </div>
    </div>
  </div>
</template>
