<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { getProfileSessionToken } from '@/composables/profileSession'
import { useSkillAtlas } from '@/composables/useSkillAtlas'
import { useSkillAtlasActions } from '@/composables/useSkillAtlasActions'
import { useSkillGraphView } from '@/composables/useSkillGraphView'
import { useSettings } from '@/composables/useSettings'
import { AppModal } from '@/components/ui'
import SkillAtlasExplorer from '@/components/skills/SkillAtlasExplorer.vue'
import { errorMessage } from '@/utils/errorMessage'

const props = defineProps<{ visible: boolean }>()
const emit = defineEmits<{ close: [] }>()
const atlas = useSkillAtlas()
const { model, modalView, error } = atlas
const { learnSkill, openSkill, actionError } = useSkillAtlasActions('modal')
const viewMode = useSkillGraphView()
/** Settings that failed to load: the remembered view is unknown until a retry. */
const settingsError = ref<string | null>(null)
const shownError = computed(() => error.value ?? settingsError.value)
/** The remembered view is a profile setting; the modal can open before any page has loaded settings. */
async function loadSettings(): Promise<void> {
  const session = getProfileSessionToken()
  settingsError.value = null
  try {
    await useSettings().initialize()
  } catch (cause) {
    if (session === getProfileSessionToken() && props.visible) settingsError.value = errorMessage(cause)
  }
}
function retry(): void {
  if (settingsError.value) void loadSettings()
  if (error.value) void atlas.refresh('retry')
}
watch(() => props.visible, (visible, _old, onCleanup) => {
  if (!visible) return
  void atlas.ensureLoaded()
  void loadSettings()
  onCleanup(atlas.activateSurface('modal'))
}, { immediate: true })
</script>

<template>
  <AppModal :open="visible" :title="$t('skills.graph.modalTitle')" max-width="90rem" @close="emit('close')">
    <p v-if="actionError" role="alert">{{ actionError }}</p>
    <div v-if="shownError" role="alert"><p>{{ shownError }}</p><button type="button" @click="retry">{{ $t('common.actions.retry') }}</button></div>
    <SkillAtlasExplorer v-if="visible && model" v-model:view-mode="viewMode" :model="model" :view="modalView" :feedback="atlas.feedbackFor('modal')"
      @update:view="atlas.updateView('modal', $event)" @open-skill="openSkill"
      @learn-skill="learnSkill" @feedback-consumed="atlas.acknowledgeFeedback" />
  </AppModal>
</template>
