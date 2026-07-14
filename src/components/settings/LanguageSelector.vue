<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'

import { useLocale } from '@/composables/useLocale'
import type { AppLocale } from '@/locales/meta'

const { t } = useI18n()
const { preference, available, setLocale } = useLocale()

type Choice = 'system' | AppLocale

const isActive = (code: Choice) => preference.value === code

const columns = computed(() => (available.length + 1 > 6 ? 3 : 2))

function choose(code: Choice) {
  void setLocale(code)
}
</script>

<template>
  <div
    class="grid gap-2"
    :style="{ gridTemplateColumns: `repeat(${columns}, minmax(0, 1fr))` }"
  >
    <button
      class="locale-card"
      :class="{ 'locale-card--active': isActive('system') }"
      :aria-pressed="isActive('system')"
      @click="choose('system')"
    >
      <span class="locale-text">
        <span class="locale-endonym">{{ t('common.language.system') }}</span>
        <span class="locale-english">{{ t('common.language.systemHint') }}</span>
      </span>
      <svg v-if="isActive('system')" class="locale-check" viewBox="0 0 20 20" fill="currentColor" aria-hidden="true">
        <path fill-rule="evenodd" d="M16.7 5.3a1 1 0 010 1.4l-7.5 7.5a1 1 0 01-1.4 0L3.3 9.7a1 1 0 011.4-1.4l3.3 3.3 6.8-6.8a1 1 0 011.4 0z" clip-rule="evenodd" />
      </svg>
    </button>

    <button
      v-for="loc in available"
      :key="loc.code"
      class="locale-card"
      :class="{ 'locale-card--active': isActive(loc.code) }"
      :aria-pressed="isActive(loc.code)"
      :lang="loc.code"
      :dir="loc.dir"
      @click="choose(loc.code)"
    >
      <span class="locale-text">
        <span class="locale-endonym">{{ loc.endonym }}</span>
        <span class="locale-english" dir="ltr">{{ loc.englishName }}</span>
      </span>
      <svg v-if="isActive(loc.code)" class="locale-check" viewBox="0 0 20 20" fill="currentColor" aria-hidden="true">
        <path fill-rule="evenodd" d="M16.7 5.3a1 1 0 010 1.4l-7.5 7.5a1 1 0 01-1.4 0L3.3 9.7a1 1 0 011.4-1.4l3.3 3.3 6.8-6.8a1 1 0 011.4 0z" clip-rule="evenodd" />
      </svg>
    </button>
  </div>
</template>

<style scoped>
.locale-card {
  display: flex;
  flex-direction: row;
  align-items: center;
  justify-content: space-between;
  gap: 0.5rem;
  padding: 0.625rem 0.75rem;
  border: 1px solid var(--color-border, hsl(var(--border)));
  border-radius: var(--radius, 0.5rem);
  background: transparent;
  text-align: start;
  transition: border-color 0.15s, background-color 0.15s;
}

.locale-text {
  display: flex;
  flex-direction: column;
  gap: 0.15rem;
  min-width: 0;
}

.locale-check {
  width: 1rem;
  height: 1rem;
  flex-shrink: 0;
  color: var(--color-primary, hsl(var(--primary)));
}

.locale-card:hover {
  background: color-mix(in srgb, currentColor 6%, transparent);
}

.locale-card--active {
  border-color: var(--color-primary, hsl(var(--primary)));
  background: color-mix(in srgb, var(--color-primary, hsl(var(--primary))) 10%, transparent);
}

.locale-endonym {
  font-size: 0.9375rem;
  font-weight: 500;
  color: var(--color-foreground, hsl(var(--foreground)));
  line-height: 1.2;
}

.locale-english {
  font-size: 0.75rem;
  color: var(--color-muted-foreground, hsl(var(--muted-foreground)));
}
</style>
