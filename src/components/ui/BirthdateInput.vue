<script setup lang="ts">
/**
 * Birthdate picker as three segmented selects (Day / Month / Year) instead of a
 * native `<input type="date">` — far easier for a birth year, which the native
 * picker makes you step back through decade by decade. Month names are
 * localized to the active UI language. Emits an ISO `YYYY-MM-DD` string (or `''`
 * while incomplete), matching what the caller persists.
 */
import { computed, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'

const props = defineProps<{
  modelValue: string
  /** Oldest allowed age in years (bounds the year list). */
  maxAge?: number
}>()
const emit = defineEmits<{ (e: 'update:modelValue', value: string): void }>()

const { locale } = useI18n()

const day = ref<number | null>(null)
const month = ref<number | null>(null) // 1-12
const year = ref<number | null>(null)

// Hydrate from an incoming ISO value (edit case).
watch(
  () => props.modelValue,
  (v) => {
    const m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(v ?? '')
    if (m) {
      year.value = Number(m[1])
      month.value = Number(m[2])
      day.value = Number(m[3])
    }
  },
  { immediate: true },
)

const now = new Date()
const currentYear = now.getUTCFullYear()

const years = computed(() => {
  const oldest = currentYear - (props.maxAge ?? 120)
  const out: number[] = []
  for (let y = currentYear; y >= oldest; y--) out.push(y)
  return out
})

const months = computed(() => {
  const fmt = new Intl.DateTimeFormat(locale.value, { month: 'long' })
  return Array.from({ length: 12 }, (_, i) => ({
    value: i + 1,
    label: fmt.format(new Date(Date.UTC(2000, i, 1))),
  }))
})

/** Days in the selected month/year (defaults to 31 until both are chosen). */
const daysInMonth = computed(() => {
  if (!month.value) return 31
  const y = year.value ?? 2000 // leap-safe default
  return new Date(Date.UTC(y, month.value, 0)).getUTCDate()
})
const days = computed(() => Array.from({ length: daysInMonth.value }, (_, i) => i + 1))

// Clamp the day if the month/year shrinks the range (e.g. 31 → Feb).
watch(daysInMonth, (max) => {
  if (day.value && day.value > max) day.value = max
})

const pad = (n: number) => String(n).padStart(2, '0')

watch([day, month, year], () => {
  if (day.value && month.value && year.value) {
    emit('update:modelValue', `${year.value}-${pad(month.value)}-${pad(day.value)}`)
  } else if (props.modelValue) {
    emit('update:modelValue', '')
  }
})
</script>

<template>
  <div class="birthdate-grid" dir="ltr">
    <label class="bd-field">
      <span class="bd-label">{{ $t('common.birthdate.day') }}</span>
      <select v-model.number="day" class="bd-select">
        <option :value="null" disabled>{{ $t('common.birthdate.dayPlaceholder') }}</option>
        <option v-for="d in days" :key="d" :value="d">{{ d }}</option>
      </select>
    </label>

    <label class="bd-field bd-field--month">
      <span class="bd-label">{{ $t('common.birthdate.month') }}</span>
      <select v-model.number="month" class="bd-select">
        <option :value="null" disabled>{{ $t('common.birthdate.monthPlaceholder') }}</option>
        <option v-for="m in months" :key="m.value" :value="m.value">{{ m.label }}</option>
      </select>
    </label>

    <label class="bd-field">
      <span class="bd-label">{{ $t('common.birthdate.year') }}</span>
      <select v-model.number="year" class="bd-select">
        <option :value="null" disabled>{{ $t('common.birthdate.yearPlaceholder') }}</option>
        <option v-for="y in years" :key="y" :value="y">{{ y }}</option>
      </select>
    </label>
  </div>
</template>

<style scoped>
.birthdate-grid {
  display: grid;
  grid-template-columns: 1fr 1.6fr 1fr;
  gap: 0.5rem;
}
.bd-field {
  display: flex;
  flex-direction: column;
  gap: 0.25rem;
  min-width: 0;
}
.bd-label {
  font-size: 0.6875rem;
  font-weight: 500;
  text-transform: uppercase;
  letter-spacing: 0.03em;
  color: hsl(var(--muted-foreground));
}
.bd-select {
  appearance: none;
  width: 100%;
  padding: 0.5rem 1.75rem 0.5rem 0.625rem;
  font-size: 0.875rem;
  color: hsl(var(--foreground));
  background-color: hsl(var(--background));
  border: 1px solid hsl(var(--border));
  border-radius: 0.5rem;
  /* chevron */
  background-image: url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='16' height='16' viewBox='0 0 24 24' fill='none' stroke='%23888' stroke-width='2' stroke-linecap='round' stroke-linejoin='round'%3E%3Cpath d='M6 9l6 6 6-6'/%3E%3C/svg%3E");
  background-repeat: no-repeat;
  background-position: right 0.5rem center;
  transition:
    border-color 0.15s,
    box-shadow 0.15s;
}
.bd-select:focus {
  outline: none;
  border-color: hsl(var(--primary));
  box-shadow: 0 0 0 2px hsl(var(--ring) / 0.4);
}
</style>
