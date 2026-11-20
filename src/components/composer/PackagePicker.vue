<script setup lang="ts">
// Choosing a standards package for a course: pick a .zip, have the host
// import (check and keep) it, and show what it is before it joins the
// course — title, standard, edition, size and lesson structure — or why it
// was refused, in words an instructor can act on. A course that is or will
// be public also needs the instructor to confirm they may pass the content
// on, because every device that takes the course redistributes it.
import { computed, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { open as openFileDialog } from '@tauri-apps/plugin-dialog'
import { useLocalApi } from '@/composables/useLocalApi'
import { AppButton, AppSpinner } from '@/components/ui'
import type { Package, PackageActivity } from '@/types'
import { packageErrorMessage, type PackageErrorMessage } from './packageMessages'

const props = defineProps<{
  /** Whether the course is or will be public; asks for the redistribution confirmation. */
  willBePublic: boolean
  /** `add` for a new element, `replace` for an existing one. */
  purpose: 'add' | 'replace'
  busy?: boolean
}>()

const emit = defineEmits<{ confirm: [pkg: Package]; cancel: [] }>()

const { t } = useI18n()
const { invoke } = useLocalApi()

const checking = ref(false)
const chosen = ref<Package | null>(null)
const refusal = ref<PackageErrorMessage | null>(null)
const mayRedistribute = ref(false)

interface Row { activity: PackageActivity, depth: number }

const rows = computed<Row[]>(() => {
  const out: Row[] = []
  const walk = (activity: PackageActivity, depth: number) => {
    if (!activity.visible) return
    out.push({ activity, depth })
    for (const child of activity.children ?? []) walk(child, depth + 1)
  }
  for (const child of chosen.value?.description.tree.children ?? []) walk(child, 0)
  return out
})

const sizeValue = computed(() => {
  const description = chosen.value?.description
  if (!description) return ''
  const megabytes = (description.uncompressed_bytes / (1024 * 1024)).toFixed(1)
  return t('instructor.package.sizeValue', { files: description.files, megabytes })
})

const canConfirm = computed(() => Boolean(chosen.value) && (!props.willBePublic || mayRedistribute.value) && !props.busy)

async function choose() {
  const picked = await openFileDialog({
    multiple: false,
    directory: false,
    filters: [{ name: t('instructor.package.fileFilter'), extensions: ['zip'] }],
  })
  const path = Array.isArray(picked) ? picked[0] : picked
  if (typeof path !== 'string' || !path) return
  checking.value = true
  chosen.value = null
  refusal.value = null
  mayRedistribute.value = false
  try {
    chosen.value = await invoke<Package>('package_import', { path })
  } catch (error) {
    refusal.value = packageErrorMessage(error)
  } finally {
    checking.value = false
  }
}

function confirm() {
  if (chosen.value && canConfirm.value) emit('confirm', chosen.value)
}
</script>

<template>
  <div data-package-picker class="space-y-4 rounded-lg border border-border bg-card p-4">
    <div class="space-y-1">
      <h3 class="text-sm font-semibold text-foreground">{{ $t('instructor.package.pickerTitle') }}</h3>
      <p class="text-xs text-muted-foreground">{{ $t('instructor.package.pickerIntro') }}</p>
    </div>

    <div class="flex flex-wrap items-center gap-2">
      <AppButton size="sm" variant="secondary" :disabled="checking || busy" data-package-choose @click="choose">
        {{ chosen || refusal ? $t('instructor.package.chooseAnother') : $t('instructor.package.choose') }}
      </AppButton>
      <span v-if="checking" class="inline-flex items-center gap-2 text-sm text-muted-foreground">
        <AppSpinner size="sm" />{{ $t('instructor.package.checking') }}
      </span>
    </div>

    <div v-if="refusal" data-package-refusal :data-code="refusal.code ?? 'other'" role="alert" class="space-y-1 rounded-md border border-error/40 bg-error/5 p-3 text-sm">
      <p class="text-foreground">{{ $t(refusal.key, refusal.params) }}</p>
      <p v-if="refusal.detail" class="text-xs text-muted-foreground">{{ $t('instructor.package.detail', { detail: refusal.detail }) }}</p>
    </div>

    <section v-if="chosen" data-package-summary class="space-y-3">
      <h4 class="text-xs font-semibold uppercase tracking-wide text-muted-foreground">{{ $t('instructor.package.summaryHeading') }}</h4>
      <dl class="grid grid-cols-[max-content_1fr] gap-x-4 gap-y-1 text-sm">
        <dt class="text-muted-foreground">{{ $t('instructor.package.title') }}</dt>
        <dd data-summary-title class="font-medium text-foreground">{{ chosen.title }}</dd>
        <dt class="text-muted-foreground">{{ $t('instructor.package.standard') }}</dt>
        <dd data-summary-standard>{{ $t(`instructor.package.standards.${chosen.standard}`) }}</dd>
        <dt class="text-muted-foreground">{{ $t('instructor.package.edition') }}</dt>
        <dd data-summary-edition>{{ $t(`instructor.package.editions.${chosen.edition ?? 'none'}`) }}</dd>
        <dt class="text-muted-foreground">{{ $t('instructor.package.size') }}</dt>
        <dd data-summary-size>{{ sizeValue }}</dd>
      </dl>
      <div>
        <p class="mb-1 text-xs font-semibold text-muted-foreground">{{ $t('instructor.package.lessons') }}</p>
        <ul data-summary-lessons class="space-y-0.5 text-sm">
          <li
            v-for="row in rows"
            :key="row.activity.id"
            data-summary-activity
            :data-launchable="row.activity.launch ? 'true' : 'false'"
            :style="{ paddingInlineStart: `${row.depth * 1}rem` }"
            :class="row.activity.launch ? 'text-foreground' : 'font-medium text-muted-foreground'"
          >
            {{ row.activity.title }}
            <span v-if="row.activity.launch" class="text-xs text-muted-foreground">· {{ $t(`instructor.package.lessonKind.${row.activity.launch.kind}`) }}</span>
          </li>
        </ul>
      </div>

      <div v-if="willBePublic" data-package-redistribution class="space-y-2 rounded-md border border-amber-500/40 bg-amber-50 p-3 text-sm text-amber-950 dark:bg-amber-950/40 dark:text-amber-100">
        <p class="font-semibold">{{ $t('instructor.package.redistributeHeading') }}</p>
        <p>{{ $t('instructor.package.redistributeBody') }}</p>
        <label class="flex items-start gap-2">
          <input v-model="mayRedistribute" type="checkbox" class="mt-0.5" data-package-redistribution-confirm>
          <span>{{ $t('instructor.package.redistributeConfirm') }}</span>
        </label>
      </div>
    </section>

    <div class="flex gap-2">
      <AppButton v-if="chosen" size="sm" :disabled="!canConfirm" :loading="busy" data-package-confirm @click="confirm">
        {{ purpose === 'add' ? $t('instructor.package.add') : $t('instructor.package.replace') }}
      </AppButton>
      <AppButton size="sm" variant="ghost" :disabled="busy" @click="emit('cancel')">{{ $t('instructor.package.cancel') }}</AppButton>
    </div>
  </div>
</template>
