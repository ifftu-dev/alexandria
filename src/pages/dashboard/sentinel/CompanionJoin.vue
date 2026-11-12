<script setup lang="ts">
/**
 * Phone: join a desktop Sentinel session as its attested companion. The
 * phone attests itself for that session and the desktop verifies it; the
 * phone only accepts codes for the learner signed in here.
 */
import { ref } from 'vue'
import { useRouter } from 'vue-router'
import { AppButton } from '@/components/ui'
import { useCompanionPhone } from '@/composables/useCompanionPhone'
import { isMobilePlatform } from '@/composables/usePlatform'

const router = useRouter()
const companion = useCompanionPhone()
const code = ref('')
const joining = ref(false)

async function join(): Promise<void> {
  joining.value = true
  try {
    await companion.join(code.value)
  } finally {
    joining.value = false
  }
}
</script>

<template>
  <div class="mx-auto max-w-lg space-y-4 p-4">
    <button class="text-sm text-muted-foreground hover:text-foreground" @click="router.push('/dashboard/sentinel')">
      ← {{ $t('sentinel.title') }}
    </button>
    <h1 class="text-xl font-bold text-foreground">{{ $t('sentinel.companion.joinTitle') }}</h1>
    <p v-if="!isMobilePlatform" class="text-sm text-muted-foreground">{{ $t('sentinel.companion.unsupported') }}</p>
    <template v-else-if="companion.joined.value">
      <p class="text-sm text-success" data-testid="companion-joined">{{ $t('sentinel.companion.joined') }}</p>
      <AppButton variant="outline" size="sm" @click="companion.leave()">{{ $t('sentinel.companion.leave') }}</AppButton>
    </template>
    <template v-else>
      <p class="text-sm text-muted-foreground">{{ $t('sentinel.companion.joinBody') }}</p>
      <textarea
        v-model="code"
        rows="4"
        class="w-full rounded-lg border border-border bg-background p-2 font-mono text-xs text-foreground"
        :placeholder="$t('sentinel.companion.joinPlaceholder')"
        data-testid="companion-code-input"
      />
      <AppButton :disabled="!code.trim()" :loading="joining" data-testid="companion-join" @click="join">
        {{ $t('sentinel.companion.join') }}
      </AppButton>
      <p v-if="companion.error.value" class="text-sm text-error">{{ companion.error.value }}</p>
    </template>
  </div>
</template>
