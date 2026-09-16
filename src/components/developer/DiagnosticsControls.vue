<script setup lang="ts">
import { useRouter } from 'vue-router'
import { useI18n } from 'vue-i18n'

import { useDiagnostics } from '@/composables/useDiagnostics'
import { usePlatform } from '@/composables/usePlatform'
import { AppButton, ConfirmDialog } from '@/components/ui'

const router = useRouter()
const { t } = useI18n()
const diagnostics = useDiagnostics()
const { isMobilePlatform } = usePlatform()

async function enter(): Promise<void> {
  try {
    await diagnostics.confirmEntry()
    await router.replace('/settings')
  } catch {
    // The modal keeps the actionable failure visible for retry.
  }
}

async function exit(): Promise<void> {
  try {
    await diagnostics.exit()
  } catch {
    // The persistent banner shows the failure and keeps Exit available.
  }
}

function action(kind: 'reload' | 'devtools' | 'sentinel' | 'install_cli'): void {
  void diagnostics.runAction(kind).catch(() => undefined)
}
</script>

<template>
  <ConfirmDialog
    :open="diagnostics.promptOpen.value"
    :title="t('common.diagnostics.enterTitle')"
    :message="diagnostics.openAssessmentCount.value > 0
      ? t('common.diagnostics.assessmentWarning')
      : t('common.diagnostics.enterMessage')"
    :confirm-label="t('common.diagnostics.enter')"
    :loading="diagnostics.busy.value"
    @confirm="enter"
    @cancel="diagnostics.cancelEntry"
  >
    <p v-if="diagnostics.error.value" role="alert" class="mb-4 text-sm text-error">
      {{ diagnostics.error.value }}
    </p>
  </ConfirmDialog>

  <div
    v-if="diagnostics.enabled.value"
    role="status"
    class="fixed inset-x-3 bottom-3 z-[90] flex flex-wrap items-center gap-2 rounded-xl border border-warning/50 bg-card p-3 shadow-xl"
  >
    <strong class="me-auto text-sm text-foreground">{{ $t('common.diagnostics.active') }}</strong>
    <AppButton v-if="!isMobilePlatform" size="sm" variant="outline" @click="action('reload')">
      {{ $t('common.diagnostics.reload') }}
    </AppButton>
    <AppButton v-if="!isMobilePlatform" size="sm" variant="outline" @click="action('devtools')">
      {{ $t('common.diagnostics.devtools') }}
    </AppButton>
    <AppButton size="sm" variant="outline" @click="action('sentinel')">
      {{ $t('common.diagnostics.sentinel') }}
    </AppButton>
    <AppButton v-if="!isMobilePlatform" size="sm" variant="outline" @click="action('install_cli')">
      {{ $t('common.diagnostics.installCli') }}
    </AppButton>
    <AppButton size="sm" :loading="diagnostics.busy.value" @click="exit">
      {{ $t('common.diagnostics.exit') }}
    </AppButton>
    <p v-if="diagnostics.error.value" role="alert" class="w-full text-xs text-error">
      {{ diagnostics.error.value }}
    </p>
  </div>
</template>
