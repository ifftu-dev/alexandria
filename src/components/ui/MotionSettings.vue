<script setup lang="ts">
import { useId } from 'vue'
import { useMotion } from '@/composables/useMotion'
const { preference, setPreference } = useMotion()
const id = useId()
</script>

<template>
  <fieldset :aria-describedby="`${id}-hint`">
    <legend class="text-sm font-semibold mb-3">{{ $t('settings.motion.title') }}</legend>
    <div class="flex flex-wrap gap-2">
      <button v-for="option in (['system', 'reduced', 'off'] as const)" :key="option" type="button"
        class="btn btn-sm" :class="preference === option ? 'btn-primary' : 'btn-secondary'"
        :aria-pressed="preference === option" @click="setPreference(option)">
        {{ $t(`settings.motion.${option}`) }}
      </button>
    </div>
    <p :id="`${id}-hint`" class="text-xs text-muted-foreground mt-2">{{ $t('settings.motion.hint') }}</p>
  </fieldset>
</template>
