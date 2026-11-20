<script setup lang="ts">
// A package element in the course editor: the attached package's summary,
// replacing it, and a preview that runs it as a learner would see it
// without an enrolment and without storing anything. A package element is
// never graded, so this editor offers no grading or skill controls.
import { computed, onMounted, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useLocalApi } from '@/composables/useLocalApi'
import { AppButton } from '@/components/ui'
import PackagePicker from '../PackagePicker.vue'
import PackagePlayer from '@/components/course/package/PackagePlayer.vue'
import { courseWillBePublic, packageErrorMessage, type PackageErrorMessage } from '../packageMessages'
import type { Element, Package } from '@/types'
import { launchables } from '@/components/course/package/packageTree'

const props = defineProps<{ element: Element }>()
const emit = defineEmits<{ updated: [Element] }>()

const { t } = useI18n()
const { invoke } = useLocalApi()

const pkg = ref<Package | null>(null)
const loaded = ref(false)
const picking = ref(false)
const saving = ref(false)
const previewing = ref(false)
const failure = ref<PackageErrorMessage | null>(null)

const lessonCount = computed(() => launchables(pkg.value?.description.tree).length)

async function load() {
  loaded.value = false
  pkg.value = null
  const hash = props.element.content_cid
  if (hash) {
    try {
      pkg.value = await invoke<Package | null>('package_get', { hash })
    } catch (error) {
      failure.value = packageErrorMessage(error)
    }
  }
  loaded.value = true
}

async function attach(chosen: Package) {
  saving.value = true
  failure.value = null
  try {
    const updated = await invoke<Element>('update_element', {
      elementId: props.element.id,
      req: { content_hash: chosen.hash },
    })
    picking.value = false
    previewing.value = false
    emit('updated', updated)
  } catch (error) {
    failure.value = packageErrorMessage(error)
  } finally {
    saving.value = false
  }
}

onMounted(load)
watch(() => props.element.content_cid, load)
</script>

<template>
  <div data-package-editor class="space-y-4">
    <h3 class="text-sm font-semibold text-foreground">{{ $t('instructor.package.attachedHeading') }}</h3>

    <p data-package-self-reported class="rounded-md border border-border bg-muted/40 p-3 text-xs text-muted-foreground">
      {{ $t('instructor.package.selfReported') }}
    </p>

    <template v-if="loaded">
      <div v-if="pkg" data-package-attached class="space-y-1 text-sm">
        <p class="font-medium text-foreground">{{ pkg.title }}</p>
        <p class="text-xs text-muted-foreground">
          {{ $t(`instructor.package.standards.${pkg.standard}`) }}<template v-if="pkg.edition"> · {{ $t(`instructor.package.editions.${pkg.edition}`) }}</template>
          · {{ $t('instructor.package.lessonCount', { count: lessonCount }, lessonCount) }}
        </p>
      </div>
      <p v-else-if="element.content_cid" class="text-sm text-muted-foreground">{{ $t('instructor.package.missingLocally') }}</p>
      <p v-else data-package-not-attached class="text-sm text-muted-foreground">{{ $t('instructor.package.notAttached') }}</p>
    </template>

    <div v-if="!picking" class="flex flex-wrap gap-2">
      <AppButton size="sm" variant="secondary" data-package-replace @click="picking = true">
        {{ pkg ? $t('instructor.package.chooseAnother') : $t('instructor.package.choose') }}
      </AppButton>
      <AppButton v-if="pkg" size="sm" variant="outline" data-package-preview @click="previewing = !previewing">
        {{ previewing ? $t('instructor.package.closePreview') : $t('instructor.package.preview') }}
      </AppButton>
    </div>

    <PackagePicker
      v-else
      purpose="replace"
      :will-be-public="courseWillBePublic()"
      :busy="saving"
      @confirm="attach"
      @cancel="picking = false"
    />

    <div v-if="failure" data-package-error :data-code="failure.code ?? 'other'" role="alert" class="space-y-1 rounded-md border border-error/40 bg-error/5 p-3 text-sm">
      <p class="text-foreground">{{ t(failure.key, failure.params) }}</p>
      <p v-if="failure.detail" class="text-xs text-muted-foreground">{{ t('instructor.package.detail', { detail: failure.detail }) }}</p>
    </div>

    <div v-if="previewing && pkg" data-package-preview-frame class="flex h-[36rem] min-h-0 flex-col overflow-hidden rounded-lg border border-border">
      <PackagePlayer
        :key="`${element.id}:${pkg.hash}`"
        :element="element"
        :enrollment-id="null"
        :publisher="$t('instructor.package.previewPublisher')"
        :progress="null"
        preview
      />
    </div>
  </div>
</template>
