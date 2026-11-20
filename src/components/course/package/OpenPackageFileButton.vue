<script setup lang="ts">
// Opens a package file from this device to learn from: the host makes a
// draft course around it and enrols the learner locally, then the player
// takes over. The draft is never announced or served.
import { ref } from 'vue'
import { useRouter } from 'vue-router'
import { useI18n } from 'vue-i18n'
import { open as openFileDialog } from '@tauri-apps/plugin-dialog'
import { useLocalApi } from '@/composables/useLocalApi'
import { AppButton } from '@/components/ui'
import { packageErrorMessage } from '@/components/composer/packageMessages'
import type { OpenedPackageCourse } from '@/types'

const { t } = useI18n()
const { invoke } = useLocalApi()
const router = useRouter()

const opening = ref(false)
const error = ref('')

async function openFile() {
  error.value = ''
  const picked = await openFileDialog({
    multiple: false,
    directory: false,
    filters: [{ name: t('learn.packageFile.fileFilter'), extensions: ['zip'] }],
  })
  const path = Array.isArray(picked) ? picked[0] : picked
  if (typeof path !== 'string' || !path) return
  opening.value = true
  try {
    const opened = await invoke<OpenedPackageCourse>('package_open_file', { path })
    await router.push(`/learn/${opened.course_id}`)
  } catch (e) {
    const message = packageErrorMessage(e)
    error.value = message.code ? t(message.key, message.params) : t('learn.packageFile.failed', { error: String(e) })
  } finally {
    opening.value = false
  }
}
</script>

<template>
  <div class="flex flex-col items-end gap-1">
    <AppButton variant="ghost" size="sm" :loading="opening" data-open-package-file @click="openFile">
      {{ opening ? $t('learn.packageFile.opening') : $t('learn.packageFile.open') }}
    </AppButton>
    <p v-if="error" role="alert" class="max-w-xs text-end text-xs text-error">{{ error }}</p>
  </div>
</template>
