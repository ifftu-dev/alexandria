<script setup lang="ts">
import { ref } from 'vue'
import GoalPicker from '@/components/goals/GoalPicker.vue'
import AppInput from '@/components/ui/AppInput.vue'
import AppButton from '@/components/ui/AppButton.vue'
import AppModal from '@/components/ui/AppModal.vue'
import MobileTabBar from '@/components/layout/MobileTabBar.vue'

const name = ref('Desktop UI Audit')
const open = ref(false)
const nestedOpen = ref(false)
</script>

<template>
  <main class="fixture">
    <h1>Desktop UI regression fixture</h1>
    <section aria-label="Goal picker">
      <h2>What are you working toward?</h2>
      <GoalPicker />
    </section>
    <section aria-label="Profile form">
      <AppInput v-model="name" label="Display name" />
      <AppInput model-value="" label="Invalid name" error="Enter a display name" />
      <AppButton @click="open = true">Open long dialog</AppButton>
    </section>
    <AppModal :open="open" title="Long settings dialog" @close="open = false">
      <AppInput v-model="name" label="Dialog display name" />
      <AppButton @click="nestedOpen = true">Open nested dialog</AppButton>
      <p v-for="n in 30" :key="n" class="fixture-paragraph">Setting {{ n }}: explanatory text that must remain scrollable in a small desktop window.</p>
      <template #footer>
        <AppButton @click="open = false">Save settings</AppButton>
      </template>
    </AppModal>
    <AppModal :open="nestedOpen" title="Nested dialog" @close="nestedOpen = false">
      <AppButton @click="nestedOpen = false">Close nested dialog</AppButton>
    </AppModal>
    <MobileTabBar />
  </main>
</template>

<style scoped>
:global(body) { overflow: auto; }
.fixture { max-width: 60rem; margin: 0 auto; padding: 1.5rem; }
h1 { font-size: 1.5rem; margin-bottom: 1.5rem; }
h2 { font-size: 1.125rem; margin-bottom: 1rem; }
section { display: grid; gap: 1rem; margin-bottom: 2rem; }
.fixture-paragraph { margin-top: 1rem; }
</style>
