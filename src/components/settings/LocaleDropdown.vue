<script setup lang="ts">
/**
 * Compact language picker as a pretty custom dropdown (not a native <select>,
 * not the full card grid). A trigger shows the current language in its own
 * script; the menu lists every language with its endonym + English name and a
 * check on the active one. Used on the onboarding welcome step.
 */
import { computed, onBeforeUnmount, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { useLocale } from '@/composables/useLocale'
import type { AppLocale } from '@/locales/meta'

const { t } = useI18n()
const { preference, available, setLocale } = useLocale()

type Choice = 'system' | AppLocale

const open = ref(false)
const root = ref<HTMLElement | null>(null)

const currentLabel = computed(() => {
  if (preference.value === 'system') return t('common.language.system')
  return available.find((l) => l.code === preference.value)?.endonym ?? t('common.language.system')
})

function choose(code: Choice) {
  void setLocale(code)
  open.value = false
}

function toggle() {
  open.value = !open.value
}

// Close on outside click / Escape while open.
function onDocClick(e: MouseEvent) {
  if (root.value && !root.value.contains(e.target as Node)) open.value = false
}
function onKey(e: KeyboardEvent) {
  if (e.key === 'Escape') open.value = false
}
function bind() {
  document.addEventListener('click', onDocClick, true)
  document.addEventListener('keydown', onKey)
}
function unbind() {
  document.removeEventListener('click', onDocClick, true)
  document.removeEventListener('keydown', onKey)
}
function onToggle() {
  toggle()
  if (open.value) bind()
  else unbind()
}
onBeforeUnmount(unbind)
</script>

<template>
  <div ref="root" class="locale-dd">
    <button
      type="button"
      class="locale-dd__trigger"
      :aria-expanded="open"
      :aria-label="t('common.language.label')"
      @click="onToggle"
    >
      <svg class="locale-dd__globe" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" aria-hidden="true">
        <circle cx="12" cy="12" r="9" />
        <path stroke-linecap="round" stroke-linejoin="round" d="M3 12h18M12 3a15 15 0 010 18M12 3a15 15 0 000 18" />
      </svg>
      <span class="locale-dd__current">{{ currentLabel }}</span>
      <svg class="locale-dd__chev" :class="{ 'locale-dd__chev--open': open }" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true">
        <path stroke-linecap="round" stroke-linejoin="round" d="M6 9l6 6 6-6" />
      </svg>
    </button>

    <Transition name="locale-dd-menu">
      <div v-if="open" class="locale-dd__menu" role="listbox">
        <button
          type="button"
          class="locale-dd__item"
          :class="{ 'locale-dd__item--active': preference === 'system' }"
          role="option"
          :aria-selected="preference === 'system'"
          @click="choose('system')"
        >
          <span class="locale-dd__text">
            <span class="locale-dd__endonym">{{ t('common.language.system') }}</span>
            <span class="locale-dd__english">{{ t('common.language.systemHint') }}</span>
          </span>
          <svg v-if="preference === 'system'" class="locale-dd__check" viewBox="0 0 20 20" fill="currentColor" aria-hidden="true">
            <path fill-rule="evenodd" d="M16.7 5.3a1 1 0 010 1.4l-7.5 7.5a1 1 0 01-1.4 0L3.3 9.7a1 1 0 011.4-1.4l3.3 3.3 6.8-6.8a1 1 0 011.4 0z" clip-rule="evenodd" />
          </svg>
        </button>

        <button
          v-for="loc in available"
          :key="loc.code"
          type="button"
          class="locale-dd__item"
          :class="{ 'locale-dd__item--active': preference === loc.code }"
          :lang="loc.code"
          :dir="loc.dir"
          role="option"
          :aria-selected="preference === loc.code"
          @click="choose(loc.code)"
        >
          <span class="locale-dd__text">
            <span class="locale-dd__endonym">{{ loc.endonym }}</span>
            <span class="locale-dd__english" dir="ltr">{{ loc.englishName }}</span>
          </span>
          <svg v-if="preference === loc.code" class="locale-dd__check" viewBox="0 0 20 20" fill="currentColor" aria-hidden="true">
            <path fill-rule="evenodd" d="M16.7 5.3a1 1 0 010 1.4l-7.5 7.5a1 1 0 01-1.4 0L3.3 9.7a1 1 0 011.4-1.4l3.3 3.3 6.8-6.8a1 1 0 011.4 0z" clip-rule="evenodd" />
          </svg>
        </button>
      </div>
    </Transition>
  </div>
</template>

<style scoped>
.locale-dd {
  position: relative;
  display: inline-block;
  text-align: start;
}

.locale-dd__trigger {
  display: inline-flex;
  align-items: center;
  gap: 0.5rem;
  padding: 0.5rem 0.75rem;
  border: 1px solid hsl(var(--border));
  border-radius: 0.625rem;
  background: hsl(var(--background));
  color: hsl(var(--foreground));
  font-size: 0.875rem;
  transition: border-color 0.15s, background-color 0.15s;
}
.locale-dd__trigger:hover {
  border-color: hsl(var(--primary) / 0.5);
}
.locale-dd__globe {
  width: 1rem;
  height: 1rem;
  color: hsl(var(--muted-foreground));
  flex-shrink: 0;
}
.locale-dd__current {
  font-weight: 500;
}
.locale-dd__chev {
  width: 1rem;
  height: 1rem;
  color: hsl(var(--muted-foreground));
  transition: transform 0.15s;
}
.locale-dd__chev--open {
  transform: rotate(180deg);
}

.locale-dd__menu {
  position: absolute;
  z-index: 50;
  top: calc(100% + 0.375rem);
  inset-inline-start: 50%;
  transform: translateX(-50%);
  min-width: 15rem;
  max-height: 18rem;
  overflow-y: auto;
  padding: 0.25rem;
  border: 1px solid hsl(var(--border));
  border-radius: 0.75rem;
  background: hsl(var(--popover, var(--card)));
  box-shadow: 0 10px 30px -12px rgb(0 0 0 / 0.35);
}
:global([dir='rtl']) .locale-dd__menu {
  transform: translateX(50%);
}

.locale-dd__item {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 0.5rem;
  width: 100%;
  padding: 0.5rem 0.625rem;
  border-radius: 0.5rem;
  background: transparent;
  text-align: start;
  transition: background-color 0.12s;
}
.locale-dd__item:hover {
  background: hsl(var(--muted) / 0.5);
}
.locale-dd__item--active {
  background: hsl(var(--primary) / 0.1);
}
.locale-dd__text {
  display: flex;
  flex-direction: column;
  gap: 0.1rem;
  min-width: 0;
}
.locale-dd__endonym {
  font-size: 0.875rem;
  font-weight: 500;
  color: hsl(var(--foreground));
  line-height: 1.2;
}
.locale-dd__english {
  font-size: 0.6875rem;
  color: hsl(var(--muted-foreground));
}
.locale-dd__check {
  width: 1rem;
  height: 1rem;
  flex-shrink: 0;
  color: hsl(var(--primary));
}

.locale-dd-menu-enter-active,
.locale-dd-menu-leave-active {
  transition: opacity 0.14s ease, transform 0.14s ease;
}
.locale-dd-menu-enter-from,
.locale-dd-menu-leave-to {
  opacity: 0;
  transform: translateX(-50%) translateY(-0.25rem);
}
</style>
