<script setup lang="ts">
// A sponsor role's integrity minimums: trust profile, verification rung and
// follow-up questions, each with plain language beside it, and below them
// the coverage statement a credential issued under these minimums names.
import { computed, onMounted, ref } from 'vue'
import { useLocalApi } from '@/composables/useLocalApi'
import CoverageStatementView from './CoverageStatementView.vue'
import { PROBE_POLICIES, TRUST_PROFILES, VERIFICATION_RUNGS, type RoleIntegrityChoice } from '@/utils/sentinel/rolePolicy'
import type { CoverageStatement } from '@/types'

const props = defineProps<{ modelValue: RoleIntegrityChoice }>()
const emit = defineEmits<{ 'update:modelValue': [value: RoleIntegrityChoice] }>()

const { invoke } = useLocalApi()
const statements = ref<CoverageStatement[]>([])

const profile = computed(() => props.modelValue.min_trust_profile || 'core')
const rung = computed(() => props.modelValue.min_verification_rung || 'none')
const coverage = computed(() =>
  statements.value.find(s => s.profile === profile.value && s.rung === rung.value) ?? null,
)

function set<K extends keyof RoleIntegrityChoice>(key: K, value: RoleIntegrityChoice[K]) {
  emit('update:modelValue', { ...props.modelValue, [key]: value })
}

function selected(event: Event) {
  return (event.target as HTMLSelectElement).value
}

onMounted(async () => {
  try {
    statements.value = await invoke<CoverageStatement[]>('integrity_coverage_statements')
  } catch {
    statements.value = []
  }
})
</script>

<template>
  <div class="grid gap-3">
    <label class="text-xs text-muted-foreground">{{ $t('dashboard.sponsor.roleModal.minProfile') }}
      <select
        :value="modelValue.min_trust_profile"
        class="mt-1 w-full rounded-md border border-border bg-background p-2 text-sm"
        data-testid="policy-min-profile"
        @change="set('min_trust_profile', selected($event) as RoleIntegrityChoice['min_trust_profile'])"
      >
        <option value="">{{ $t('dashboard.sponsor.roleModal.noMinimum') }}</option>
        <option v-for="p in TRUST_PROFILES" :key="p" :value="p">{{ $t(`sentinel.trust.profile.${p}`) }}</option>
      </select>
      <span class="mt-1 block">{{ $t(`sentinel.trust.profileHint.${profile}`) }}</span>
    </label>
    <label class="text-xs text-muted-foreground">{{ $t('dashboard.sponsor.roleModal.minRung') }}
      <select
        :value="modelValue.min_verification_rung"
        class="mt-1 w-full rounded-md border border-border bg-background p-2 text-sm"
        data-testid="policy-min-rung"
        @change="set('min_verification_rung', selected($event) as RoleIntegrityChoice['min_verification_rung'])"
      >
        <option value="">{{ $t('dashboard.sponsor.roleModal.noMinimum') }}</option>
        <option v-for="r in VERIFICATION_RUNGS" :key="r" :value="r">{{ $t(`sentinel.trust.rung.${r}`) }}</option>
      </select>
      <span class="mt-1 block">{{ $t(`sentinel.trust.rungHint.${rung}`) }}</span>
    </label>
    <label class="text-xs text-muted-foreground">{{ $t('dashboard.sponsor.roleModal.probePolicy') }}
      <select
        :value="modelValue.probe_policy"
        class="mt-1 w-full rounded-md border border-border bg-background p-2 text-sm"
        data-testid="policy-probe"
        @change="set('probe_policy', selected($event) as RoleIntegrityChoice['probe_policy'])"
      >
        <option value="">{{ $t('sentinel.trust.probePolicy.none') }}</option>
        <option v-for="p in PROBE_POLICIES" :key="p" :value="p">{{ $t(`sentinel.trust.probePolicy.${p}`) }}</option>
      </select>
    </label>
    <details class="rounded-md border border-border/70 p-3" open>
      <summary class="cursor-pointer text-xs font-medium text-foreground">{{ $t('dashboard.sponsor.roleModal.coverageTitle') }}</summary>
      <CoverageStatementView class="mt-2" :statement="coverage" />
    </details>
  </div>
</template>
