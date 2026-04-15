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

import type { PluginCapability } from '@/types'
import { AppButton } from '@/components/ui'

defineProps<{
  pluginName: string
  authorDid: string
  capability: PluginCapability
  reason: string
}>()

const emit = defineEmits<{
  (e: 'decide', scope: 'once' | 'session' | 'always' | 'deny'): void
}>()

function capabilityLabel(c: PluginCapability): string {
  switch (c) {
    case 'microphone':
      return 'use your microphone'
    case 'camera':
      return 'use your camera'
    case 'midi':
      return 'access connected MIDI devices'
    case 'fullscreen':
      return 'enter fullscreen mode'
    case 'clipboard':
      return 'read from and write to your clipboard'
    case 'storage':
      return 'save progress locally'
    case 'ml_inference':
      return 'run on-device machine-learning models'
  }
}

function shortDid(did: string): string {
  // did:key:z6Mk... — show first 20 chars plus last 6 for auditability.
  if (did.length <= 32) return did
  return `${did.slice(0, 20)}…${did.slice(-6)}`
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
        {{ pluginName }} wants to {{ capabilityLabel(capability) }}
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
            <p>Plugins are community-authored and run in a sandbox.</p>
            <p>
              Author:
              <code class="font-mono text-[11px]">{{ shortDid(authorDid) }}</code>
            </p>
            <p>The plugin cannot access the network or any other data on this device.</p>
          </div>
        </div>
      </div>

      <div class="mt-5 space-y-2">
        <AppButton variant="primary" class="w-full justify-center" @click="emit('decide', 'always')">
          Always allow
        </AppButton>
        <AppButton variant="secondary" class="w-full justify-center" @click="emit('decide', 'session')">
          Allow for this session
        </AppButton>
        <AppButton variant="secondary" class="w-full justify-center" @click="emit('decide', 'once')">
          Allow once
        </AppButton>
        <AppButton variant="ghost" class="w-full justify-center" @click="emit('decide', 'deny')">
          Deny
        </AppButton>
      </div>
    </div>
  </div>
</template>
