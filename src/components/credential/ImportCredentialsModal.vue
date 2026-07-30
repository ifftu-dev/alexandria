<script setup lang="ts">
/**
 * Import credentials from a file or pasted JSON.
 *
 * The counterpart to "Export all" on the credentials page. Accepts both an
 * exported bundle and a single credential, because an issuer hands you one
 * credential while a backup is a bundle — and an export you cannot re-import
 * is not a backup.
 *
 * Verification happens in Rust before anything is stored, so this component
 * never decides whether a credential is real; it only reports what the backend
 * decided. Failures are listed per credential rather than summarised, since
 * "3 of 10 failed" without saying which is not actionable.
 */

import { ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { useI18n } from 'vue-i18n'

import { AppButton, AppModal, AppTextarea } from '@/components/ui'

interface ImportFailure {
  credentialId: string
  reason: string
}

interface ImportSummary {
  imported: number
  alreadyPresent: number
  failed: ImportFailure[]
}

defineProps<{ open: boolean }>()
const emit = defineEmits<{ close: []; imported: [] }>()

const { t } = useI18n()

const pasted = ref('')
const busy = ref(false)
const summary = ref<ImportSummary | null>(null)
const error = ref('')

function reset() {
  pasted.value = ''
  summary.value = null
  error.value = ''
}

async function runImport(payload: string) {
  busy.value = true
  summary.value = null
  error.value = ''
  try {
    const result = await invoke<ImportSummary>('import_credentials', { payload })
    summary.value = result
    // Refresh the list even on a partial import — the credentials that did
    // land should appear without the user reopening anything.
    if (result.imported > 0) emit('imported')
  } catch (e) {
    error.value = String(e)
  } finally {
    busy.value = false
  }
}

async function onFile(event: Event) {
  const input = event.target as HTMLInputElement
  const file = input.files?.[0]
  if (!file) return
  try {
    await runImport(await file.text())
  } finally {
    // Clear so re-picking the same file fires change again.
    input.value = ''
  }
}

function onClose() {
  reset()
  emit('close')
}
</script>

<template>
  <AppModal
    :open="open"
    :title="t('credentials.import.title')"
    max-width="34rem"
    @close="onClose"
  >
    <div class="space-y-4">
      <p class="text-sm text-muted-foreground">{{ t('credentials.import.intro') }}</p>

      <div>
        <label
          class="inline-flex cursor-pointer items-center rounded-lg border border-border bg-muted/30 px-3 py-2 text-sm text-foreground transition-colors hover:bg-muted/60"
        >
          {{ t('credentials.import.chooseFile') }}
          <input type="file" accept="application/json,.json" class="hidden" @change="onFile" />
        </label>
      </div>

      <div class="space-y-2">
        <label class="block text-xs font-medium text-muted-foreground" for="import-paste">
          {{ t('credentials.import.orPaste') }}
        </label>
        <AppTextarea
          id="import-paste"
          v-model="pasted"
          :rows="5"
          class="font-mono text-xs"
          :placeholder="t('credentials.import.pastePlaceholder')"
        />
        <AppButton :disabled="busy || !pasted.trim()" @click="runImport(pasted)">
          {{ busy ? t('credentials.import.importing') : t('credentials.import.action') }}
        </AppButton>
      </div>

      <p v-if="error" class="text-sm text-destructive">
        {{ t('credentials.import.error', { error }) }}
      </p>

      <div v-if="summary" class="space-y-2 border-t border-border pt-3 text-sm">
        <p v-if="summary.imported > 0" class="text-foreground">
          {{ t('credentials.import.imported', { count: summary.imported }, summary.imported) }}
        </p>
        <p v-if="summary.alreadyPresent > 0" class="text-muted-foreground">
          {{
            t(
              'credentials.import.alreadyPresent',
              { count: summary.alreadyPresent },
              summary.alreadyPresent,
            )
          }}
        </p>
        <p
          v-if="summary.imported === 0 && summary.alreadyPresent === 0 && summary.failed.length === 0"
          class="text-muted-foreground"
        >
          {{ t('credentials.import.nothingImported') }}
        </p>

        <template v-if="summary.failed.length > 0">
          <p class="text-destructive">
            {{
              t('credentials.import.someFailed', { count: summary.failed.length }, summary.failed.length)
            }}
          </p>
          <ul class="space-y-1">
            <li
              v-for="f in summary.failed"
              :key="f.credentialId"
              class="break-words font-mono text-xs text-muted-foreground"
            >
              {{ t('credentials.import.failedRow', { id: f.credentialId, reason: f.reason }) }}
            </li>
          </ul>
        </template>
      </div>

      <p class="border-t border-border pt-3 text-xs text-muted-foreground">
        {{ t('credentials.import.registryNote') }}
      </p>
    </div>
  </AppModal>
</template>
