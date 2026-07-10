<script setup lang="ts">
// Standalone "bootstrap your skills" page (also offered during onboarding).
import { ref } from 'vue'
import { useRouter } from 'vue-router'
import SkillBootstrapPanel from '@/components/skills/SkillBootstrapPanel.vue'
import { AppButton, EmptyState } from '@/components/ui'

const router = useRouter()
const claimed = ref<number | null>(null)
</script>

<template>
  <div class="mx-auto max-w-2xl space-y-6 py-6">
    <div>
      <h1 class="text-2xl font-bold text-foreground">Bootstrap your skills</h1>
      <p class="mt-1 text-sm text-muted-foreground">
        Upload a resume, transcript, or credential. We'll suggest skills to
        claim — accredited documents count for more than a self-made resume.
        These are starting points; take an assessment to verify and raise them.
      </p>
    </div>

    <div v-if="claimed !== null">
      <EmptyState
        title="Skills added"
        :description="`Claimed ${claimed} skill${claimed === 1 ? '' : 's'}. Take an assessment to verify them and raise your confidence.`"
      />
      <div class="mt-4 flex gap-2">
        <AppButton @click="router.push('/skills')">View my skills</AppButton>
        <AppButton variant="outline" @click="claimed = null">Add more</AppButton>
      </div>
    </div>

    <SkillBootstrapPanel v-else @claimed="(n) => (claimed = n)" />
  </div>
</template>
