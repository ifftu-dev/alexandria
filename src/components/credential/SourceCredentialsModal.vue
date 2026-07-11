<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { AppModal, AppInput } from '@/components/ui'
import { useDisplayNames } from '@/composables/useDisplayNames'
import { extractSkillClaim, extractRoleClaim, type VerifiableCredential } from '@/types'
import {
  kindOfType,
  classNameOf,
  CREDENTIAL_KINDS,
  CREDENTIAL_CLASS_ORDER,
  type CredentialClass,
} from './credentialKind'

const props = defineProps<{
  open: boolean
  /** Skill the derived credential is for (for the modal title). */
  skillName: string
  /** Number of source ids the derived state referenced. */
  sourceCount: number
  /** Resolved input credentials that fed the derived state. */
  sources: VerifiableCredential[]
}>()

const emit = defineEmits<{
  (e: 'close'): void
  (e: 'open-credential', id: string): void
}>()

const { t } = useI18n()
const { displayName, ensureNames } = useDisplayNames()

watch(
  () => props.sources,
  (list) => {
    const dids: string[] = []
    for (const c of list) {
      if (typeof c.issuer === 'string') dids.push(c.issuer)
      if (c.credentialSubject?.id) dids.push(c.credentialSubject.id)
    }
    void ensureNames(dids.filter(Boolean))
  },
  { immediate: true },
)

// Controls -----------------------------------------------------------------
const search = ref('')
const typeFilter = ref<'all' | CredentialClass>('all')
type SortKey = 'date' | 'score' | 'type' | 'issuer'
const sortKey = ref<SortKey>('score')

// Reset the controls whenever a new derived credential is opened.
watch(
  () => props.open,
  (o) => {
    if (o) {
      search.value = ''
      typeFilter.value = 'all'
      sortKey.value = 'score'
    }
  },
)

function issuerOf(c: VerifiableCredential): string {
  return typeof c.issuer === 'string' ? c.issuer : ''
}

function scoreOf(c: VerifiableCredential): number | null {
  const skill = extractSkillClaim(c.credentialSubject)
  return skill ? skill.score : null
}

function summaryOf(c: VerifiableCredential): string {
  const skill = extractSkillClaim(c.credentialSubject)
  if (skill) return `${skill.skillId} · L${skill.level} · ${(skill.score * 100).toFixed(0)}%`
  const role = extractRoleClaim(c.credentialSubject)
  if (role) return role.role + (role.scope ? ` · ${role.scope}` : '')
  return t('credentials.sources.customClaim')
}

// Which classes actually appear — only show those filter chips.
const presentClasses = computed(() => {
  const set = new Set<string>()
  for (const c of props.sources) set.add(classNameOf(c.type))
  return CREDENTIAL_CLASS_ORDER.filter((k) => set.has(k))
})

const filtered = computed(() => {
  const q = search.value.trim().toLowerCase()
  let list = props.sources.filter((c) => {
    if (typeFilter.value !== 'all' && classNameOf(c.type) !== typeFilter.value) return false
    if (!q) return true
    const hay = [
      summaryOf(c),
      issuerOf(c),
      displayName(issuerOf(c)),
      c.id ?? '',
      classNameOf(c.type),
    ]
      .join(' ')
      .toLowerCase()
    return hay.includes(q)
  })

  list = [...list].sort((a, b) => {
    switch (sortKey.value) {
      case 'date':
        return (b.validFrom ?? '').localeCompare(a.validFrom ?? '')
      case 'score':
        return (scoreOf(b) ?? -1) - (scoreOf(a) ?? -1)
      case 'type':
        return classNameOf(a.type).localeCompare(classNameOf(b.type))
      case 'issuer':
        return displayName(issuerOf(a)).localeCompare(displayName(issuerOf(b)))
      default:
        return 0
    }
  })
  return list
})

function pickCredential(c: VerifiableCredential) {
  if (c.id) emit('open-credential', c.id)
}
</script>

<template>
  <AppModal
    :open="open"
    :title="$t('credentials.sources.title', { name: skillName })"
    max-width="44rem"
    @close="emit('close')"
  >
    <div class="space-y-4">
      <p class="text-xs text-muted-foreground">
        {{ $t('credentials.sources.builtFrom', { count: sourceCount }, sourceCount) }}
        <span v-if="sources.length < sourceCount">
          {{ $t('credentials.sources.unresolved', { count: sourceCount - sources.length }, sourceCount - sources.length) }}
        </span>
      </p>

      <!-- Controls -->
      <div class="flex flex-wrap items-center gap-2">
        <div class="min-w-[12rem] flex-1">
          <AppInput v-model="search" :placeholder="$t('credentials.sources.searchPlaceholder')" />
        </div>
        <select v-model="sortKey" class="input w-auto text-sm">
          <option value="score">{{ $t('credentials.sources.sortScore') }}</option>
          <option value="date">{{ $t('credentials.sources.sortNewest') }}</option>
          <option value="type">{{ $t('credentials.sources.sortType') }}</option>
          <option value="issuer">{{ $t('credentials.sources.sortSource') }}</option>
        </select>
      </div>

      <!-- Type filter chips -->
      <div v-if="presentClasses.length > 1" class="flex flex-wrap gap-1.5">
        <button
          class="rounded-full px-2.5 py-1 text-xs font-medium transition-colors"
          :class="typeFilter === 'all'
            ? 'bg-foreground text-background'
            : 'bg-muted text-muted-foreground hover:text-foreground'"
          @click="typeFilter = 'all'"
        >
          {{ $t('credentials.sources.all') }}
        </button>
        <button
          v-for="k in presentClasses"
          :key="k"
          class="inline-flex items-center gap-1.5 rounded-full px-2.5 py-1 text-xs font-medium transition-colors"
          :class="typeFilter === k ? CREDENTIAL_KINDS[k].badge : 'bg-muted text-muted-foreground hover:text-foreground'"
          @click="typeFilter = k"
        >
          <span class="h-1.5 w-1.5 rounded-full" :class="CREDENTIAL_KINDS[k].dot" />
          {{ $t(CREDENTIAL_KINDS[k].label) }}
        </button>
      </div>

      <!-- Source list -->
      <ul v-if="filtered.length" class="divide-y divide-border rounded-lg border border-border">
        <li
          v-for="c in filtered"
          :key="c.id ?? c.issuer + c.validFrom"
          class="flex items-center gap-3 p-3 transition-colors hover:bg-muted/40 cursor-pointer"
          @click="pickCredential(c)"
        >
          <span
            class="flex h-8 w-8 shrink-0 items-center justify-center rounded-lg text-white"
            :class="kindOfType(c.type).dot"
          >
            <svg viewBox="0 0 24 24" class="h-4 w-4" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
              <path :d="kindOfType(c.type).icon" />
            </svg>
          </span>
          <div class="min-w-0 flex-1">
            <p class="truncate text-sm font-medium text-foreground" :title="summaryOf(c)">
              {{ summaryOf(c) }}
            </p>
            <p class="truncate text-xs text-muted-foreground">
              {{ displayName(issuerOf(c)) }} · {{ (c.validFrom ?? '').slice(0, 10) }}
            </p>
          </div>
          <span class="shrink-0 rounded-full px-2 py-0.5 text-[10px] font-semibold" :class="kindOfType(c.type).badge">
            {{ $t(kindOfType(c.type).short) }}
          </span>
        </li>
      </ul>

      <p v-else class="rounded-lg border border-dashed border-border p-6 text-center text-sm text-muted-foreground">
        {{ $t('credentials.sources.empty') }}
      </p>
    </div>

    <template #footer>
      <div class="flex justify-end">
        <button class="btn" @click="emit('close')">{{ $t('common.actions.close') }}</button>
      </div>
    </template>
  </AppModal>
</template>
