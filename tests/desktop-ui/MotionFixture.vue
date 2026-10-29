<script setup lang="ts">
import { onMounted, onBeforeUnmount, ref } from 'vue'
import AppOpening from '@/components/auth/AppOpening.vue'
import ProfileSelect from '@/pages/ProfileSelect.vue'
import Onboarding from '@/pages/Onboarding.vue'
import BlankLayout from '@/layouts/BlankLayout.vue'
import MotionSettings from '@/components/ui/MotionSettings.vue'
import { useMotion } from '@/composables/useMotion'
const params = new URLSearchParams(location.search)
const ready = ref(false)
const { mode } = useMotion()
let boot: ReturnType<typeof setTimeout>
onMounted(() => { boot = setTimeout(() => { ready.value = true }, Number(params.get('boot') ?? 0)) })
onBeforeUnmount(() => clearTimeout(boot))
</script>
<template>
  <AppOpening :ready="ready" />
  <BlankLayout v-if="ready">
    <div class="p-4 border-b border-border"><MotionSettings /></div>
    <ProfileSelect v-if="params.get('existing') === '1'" />
    <Onboarding v-else />
  </BlankLayout>
  <p v-else role="status">Initializing</p>
  <span class="sr-only" data-motion-mode>{{ mode }}</span>
</template>
