<script setup lang="ts">
import { ref } from 'vue'
import { useRouter } from 'vue-router'
import { exit } from '@tauri-apps/plugin-process'
import { useProfiles } from '@/composables/useProfiles'
import { isMobilePlatform } from '@/composables/usePlatform'
import Starfield from '@/components/auth/Starfield.vue'

const router = useRouter()
const { incompatibleData, moveIncompatibleDataAside, initialize } = useProfiles()

const moving = ref(false)
const moveError = ref('')
const movedTo = ref('')

async function moveAside() {
  moving.value = true
  moveError.value = ''
  try {
    movedTo.value = await moveIncompatibleDataAside()
  } catch (e) {
    moveError.value = String(e)
  } finally {
    moving.value = false
  }
}

async function continueToApp() {
  const state = await initialize()
  router.replace(state === 'picker' ? '/profiles' : '/onboarding')
}

// Mobile apps are closed by the OS, not from inside the app.
function quit() {
  void exit(0)
}
</script>

<template>
  <div class="min-h-full bg-background relative overflow-y-auto flex items-center justify-center p-4 sm:p-6 lg:p-8">
    <Starfield />

    <div class="w-full max-w-xl relative z-10">
      <div class="rounded-2xl border border-border/70 bg-card/80 backdrop-blur px-6 py-8 text-center">
        <div v-if="movedTo" class="w-16 h-16 rounded-full bg-success/10 flex items-center justify-center mx-auto mb-4">
          <svg class="w-8 h-8 text-success" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.5">
            <path stroke-linecap="round" stroke-linejoin="round" d="M9 12.75L11.25 15 15 9.75M21 12a9 9 0 11-18 0 9 9 0 0118 0z" />
          </svg>
        </div>
        <div v-else class="w-16 h-16 rounded-full bg-warning/10 flex items-center justify-center mx-auto mb-4">
          <svg class="w-8 h-8 text-warning" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.5">
            <path stroke-linecap="round" stroke-linejoin="round" d="M12 9v3.75m-9.303 3.376c-.866 1.5.217 3.374 1.948 3.374h14.71c1.73 0 2.813-1.874 1.948-3.374L13.949 3.378c-.866-1.5-3.032-1.5-3.898 0L2.697 16.126zM12 15.75h.007v.008H12v-.008z" />
          </svg>
        </div>

        <h1 class="text-2xl font-bold mb-2 text-foreground">
          {{ movedTo ? $t('onboarding.incompatibleData.movedHeading') : $t('onboarding.incompatibleData.heading') }}
        </h1>
        <p class="text-sm text-muted-foreground mb-6">
          {{ movedTo ? $t('onboarding.incompatibleData.movedSubtitle') : $t('onboarding.incompatibleData.subtitle') }}
        </p>

        <template v-if="movedTo">
          <div class="card p-5 mb-5 text-start">
            <p class="text-sm text-success break-all">
              {{ $t('onboarding.incompatibleData.movedTo', { path: movedTo }) }}
            </p>
          </div>

          <button
            class="w-full py-2.5 px-4 rounded-md text-sm font-medium bg-primary text-primary-foreground hover:bg-primary-hover transition-colors"
            @click="continueToApp"
          >
            {{ $t('onboarding.incompatibleData.continue') }}
          </button>
        </template>

        <template v-else>
          <div class="card p-5 mb-5 text-start">
            <h2 class="text-sm font-semibold text-foreground mb-2">{{ $t('onboarding.incompatibleData.optionsHeading') }}</h2>
            <ul class="space-y-2 text-sm text-muted-foreground">
              <li class="flex items-start gap-2">
                <span class="text-primary mt-0.5 font-mono text-xs w-4 text-end shrink-0">01</span>
                {{ $t('onboarding.incompatibleData.moveAsideHint') }}
              </li>
              <li v-if="!isMobilePlatform" class="flex items-start gap-2">
                <span class="text-primary mt-0.5 font-mono text-xs w-4 text-end shrink-0">02</span>
                {{ $t('onboarding.incompatibleData.quitHint') }}
              </li>
            </ul>
          </div>

          <details v-if="incompatibleData" class="card p-5 mb-5 text-start">
            <summary class="text-sm font-semibold text-foreground cursor-pointer">
              {{ $t('onboarding.incompatibleData.details') }}
            </summary>
            <code class="block mt-2 break-words rounded-lg bg-muted/30 p-3 font-mono text-xs text-foreground">{{ incompatibleData }}</code>
          </details>

          <p v-if="moveError" class="mb-3 text-sm text-error">
            {{ $t('onboarding.incompatibleData.moveFailed', { error: moveError }) }}
          </p>

          <button
            class="w-full py-2.5 px-4 rounded-md text-sm font-medium bg-primary text-primary-foreground hover:bg-primary-hover transition-colors disabled:opacity-50"
            :disabled="moving"
            @click="moveAside"
          >
            {{ moving ? $t('onboarding.incompatibleData.moving') : $t('onboarding.incompatibleData.moveAside') }}
          </button>

          <button
            v-if="!isMobilePlatform"
            class="w-full mt-3 py-2 text-sm text-muted-foreground hover:text-foreground transition-colors"
            :disabled="moving"
            @click="quit"
          >
            {{ $t('onboarding.incompatibleData.quit') }}
          </button>
        </template>
      </div>
    </div>
  </div>
</template>
