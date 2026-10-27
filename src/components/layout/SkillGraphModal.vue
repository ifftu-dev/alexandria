<script setup lang="ts">
import { watch } from 'vue'
import { useSkillAtlas } from '@/composables/useSkillAtlas'
import { useSkillAtlasActions } from '@/composables/useSkillAtlasActions'
import { AppModal } from '@/components/ui'
import SkillAtlasExplorer from '@/components/skills/SkillAtlasExplorer.vue'

const props = defineProps<{ visible: boolean }>()
const emit = defineEmits<{ close: [] }>()
const atlas = useSkillAtlas()
const { model, modalView, feedback, error } = atlas
const { learnSkill, openSkill, actionError } = useSkillAtlasActions('modal')
watch(() => props.visible, (visible, _old, onCleanup) => {
  if (!visible) return
  void atlas.ensureLoaded()
  onCleanup(atlas.activateSurface('modal'))
}, { immediate: true })
</script>

<template>
  <AppModal :open="visible" :title="$t('skills.graph.modalTitle')" max-width="90rem" @close="emit('close')">
    <p v-if="actionError" role="alert">{{ actionError }}</p>
    <div v-if="error" role="alert"><p>{{ error }}</p><button type="button" @click="atlas.refresh('retry')">{{ $t('common.actions.retry') }}</button></div>
    <SkillAtlasExplorer v-if="visible && model" :model="model" :view="modalView" :feedback="feedback"
      @update:view="atlas.updateView('modal', $event)" @open-skill="openSkill"
      @learn-skill="learnSkill" @feedback-consumed="atlas.acknowledgeFeedback" />
  </AppModal>
</template>
