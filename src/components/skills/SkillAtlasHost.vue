<script setup lang="ts">
import { onUnmounted, watch } from 'vue'
import { useRouter } from 'vue-router'
import { installSkillAtlasRefresh, useSkillAtlas } from '@/composables/useSkillAtlas'
import { useProfiles } from '@/composables/useProfiles'
import SkillGraphModal from '@/components/layout/SkillGraphModal.vue'

const atlas = useSkillAtlas()
const profiles = useProfiles()
onUnmounted(installSkillAtlasRefresh(useRouter()))
watch([profiles.activeProfileId, profiles.isLockBlocked, profiles.initialized, profiles.loading, () => profiles.activeIdentity.value?.stake_address], ([profile, blocked, initialized, loading]) => {
  if (initialized && !loading && profile && !blocked) void atlas.ensureLoaded()
}, { immediate: true })
</script>

<template>
  <SkillGraphModal :visible="atlas.modalOpen.value" @close="atlas.setModalOpen(false)" />
</template>
