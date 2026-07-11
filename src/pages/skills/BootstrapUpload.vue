<script setup lang="ts">
// Standalone "bootstrap your skills" page (also offered during onboarding).
import { ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { useRouter } from 'vue-router'
import SkillBootstrapPanel from '@/components/skills/SkillBootstrapPanel.vue'
import { AppButton, EmptyState } from '@/components/ui'

const { t } = useI18n()
const router = useRouter()
const claimed = ref<number | null>(null)
</script>

<template>
  <div class="mx-auto max-w-2xl space-y-6 py-6">
    <div>
      <h1 class="text-2xl font-bold text-foreground">{{ $t('skills.bootstrap.title') }}</h1>
      <p class="mt-1 text-sm text-muted-foreground">
        {{ $t('skills.bootstrap.intro') }}
      </p>
    </div>

    <div v-if="claimed !== null">
      <EmptyState
        :title="$t('skills.bootstrap.addedTitle')"
        :description="t('skills.bootstrap.addedBody', { count: claimed }, claimed)"
      />
      <div class="mt-4 flex gap-2">
        <AppButton @click="router.push('/skills')">{{ $t('skills.bootstrap.viewMySkills') }}</AppButton>
        <AppButton variant="outline" @click="claimed = null">{{ $t('skills.bootstrap.addMore') }}</AppButton>
      </div>
    </div>

    <SkillBootstrapPanel v-else @claimed="(n) => (claimed = n)" />
  </div>
</template>
