<script setup lang="ts">
import { ref, computed, onMounted, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useRouter } from 'vue-router'
import { useLocalApi } from '@/composables/useLocalApi'
import { useProfiles } from '@/composables/useProfiles'
import { useSkillAtlas, type AtlasTab } from '@/composables/useSkillAtlas'
import { useSkillAtlasActions } from '@/composables/useSkillAtlasActions'
import { AppBadge, AppTabs } from '@/components/ui'
import SkillAtlasExplorer from '@/components/skills/SkillAtlasExplorer.vue'
import GraphVisibilityEditor from '@/components/skills/GraphVisibilityEditor.vue'
import CredentialsPanel from '@/pages/dashboard/Credentials.vue'
import type { SubjectInfo, SkillInfo } from '@/types'
import { BLOOM_ORDER, bloomBadge } from '@/utils/bloom'

const { invoke } = useLocalApi()
const { t } = useI18n()
const router = useRouter()
const { learnSkill, openSkill, actionError } = useSkillAtlasActions('page')
const atlas = useSkillAtlas()
const { model, pageView, feedback, error } = atlas
const profiles = useProfiles()
const loading = computed(() => atlas.isLoading.value)
const subjects = ref<SubjectInfo[]>([])
const skills = computed(() => model.value?.nodes.map(node => node.skill) ?? [])
const fields = computed(() => model.value?.fields.filter(field => field.id !== null).map(field => ({
  id: field.id!, name: field.name ?? t('skills.atlas.unknownField', { id: field.id }), icon_emoji: null, skill_count: field.total,
})) ?? [])
const search = ref('')
const selectedField = ref<string | null>(null)
const selectedSubject = ref<string | null>(null)
const activeTab = computed({ get: () => atlas.skillsTab.value, set: (tab: string) => atlas.setSkillsTab(tab as AtlasTab) })
const tabs = computed(() => [
  { key: 'credentials', label: t('skills.tabs.credentials') },
  { key: 'graph', label: t('skills.tabs.graph') },
  { key: 'browse', label: t('skills.tabs.browse') },
])
onMounted(() => { void atlas.ensureLoaded() })
watch([profiles.activeProfileId, profiles.isLockBlocked], async ([profile, blocked], _old, onCleanup) => {
  let stale = false
  onCleanup(() => { stale = true })
  subjects.value = []
  search.value = ''
  selectedField.value = selectedSubject.value = null
  if (!profile || blocked) return
  try {
    const result = await invoke<SubjectInfo[]>('list_subjects')
    if (!stale) subjects.value = result
  } catch { /* The shared snapshot exposes load failures and retry. */ }
}, { immediate: true })
watch(() => activeTab.value === 'graph' && model.value !== null, (visible, _old, onCleanup) => {
  if (visible) onCleanup(atlas.activateSurface('page'))
}, { immediate: true })
// Filter subjects when a field is selected
const filteredSubjects = computed(() => {
  if (!selectedField.value) return subjects.value
  return subjects.value.filter(s => s.subject_field_id === selectedField.value)
})

// Filter skills based on selections and search
const filteredSkills = computed(() => {
  let result = skills.value

  if (selectedSubject.value) {
    result = result.filter(sk => sk.subject_id === selectedSubject.value)
  } else if (selectedField.value) {
    result = result.filter(sk => sk.subject_field_id === selectedField.value)
  }

  if (search.value.trim()) {
    const q = search.value.toLowerCase()
    result = result.filter(
      sk =>
        sk.name.toLowerCase().includes(q) ||
        (sk.description && sk.description.toLowerCase().includes(q)) ||
        sk.bloom_level.toLowerCase().includes(q)
    )
  }

  return result
})

// Stats
const totalSkills = computed(() => skills.value.length)

function selectField(id: string | null) {
  selectedField.value = selectedField.value === id ? null : id
  selectedSubject.value = null
}

function selectSubject(id: string | null) {
  selectedSubject.value = selectedSubject.value === id ? null : id
}

function goToSkill(id: string) {
  router.push(`/skills/${id}`)
}

function goalSkill(skill: SkillInfo) { void learnSkill(skill.id) }
</script>

<template>
  <div>
    <!-- Header -->
    <div class="mb-8 flex items-start justify-between gap-4">
      <div>
        <h1 class="text-3xl font-bold text-foreground">{{ $t('skills.index.title') }}</h1>
        <p class="mt-2 text-muted-foreground">
          {{ $t('skills.index.subtitle') }}
        </p>
      </div>
      <button
        class="shrink-0 rounded-md border border-border px-3 py-2 text-sm font-medium text-foreground transition-colors hover:border-primary/50"
        @click="router.push('/skills/import')"
      >
        {{ $t('skills.index.fromResume') }}
      </button>
    </div>

    <!-- Skeleton -->
    <div v-if="loading" class="space-y-6">
      <div class="grid grid-cols-2 sm:grid-cols-4 gap-4">
        <div v-for="i in 4" :key="i" class="animate-pulse rounded-xl bg-card shadow-sm p-5 text-center">
          <div class="h-7 w-10 mx-auto rounded bg-muted-foreground/20 mb-2" />
          <div class="h-3 w-16 mx-auto rounded bg-muted-foreground/10" />
        </div>
      </div>
      <div class="h-10 w-full animate-pulse rounded-lg bg-muted-foreground/8" />
      <div class="flex gap-4">
        <div class="w-64 space-y-3">
          <div v-for="i in 2" :key="i" class="animate-pulse rounded-xl bg-card shadow-sm p-4">
            <div class="h-3 w-20 rounded bg-muted-foreground/15 mb-3" />
            <div v-for="j in 4" :key="j" class="h-7 w-full rounded bg-muted-foreground/8 mb-1.5" />
          </div>
        </div>
        <div class="flex-1 space-y-2">
          <div v-for="i in 5" :key="i" class="animate-pulse rounded-xl bg-card shadow-sm p-4">
            <div class="flex items-start justify-between">
              <div class="space-y-2 flex-1">
                <div class="h-4 w-48 rounded bg-muted-foreground/15" />
                <div class="h-3 w-full rounded bg-muted-foreground/8" />
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>

    <template v-else>
      <div v-if="activeTab !== 'graph'" class="mb-6 grid grid-cols-3 gap-4">
        <div v-for="stat in [{ count: model?.counts.earned ?? 0, label: $t('skills.atlas.earned') }, { count: model?.counts.ready ?? 0, label: $t('skills.atlas.ready') }, { count: totalSkills, label: $t('skills.stats.total') }]" :key="stat.label" class="rounded-xl bg-card p-4 text-center">
          <p class="font-mono text-2xl font-bold">{{ stat.count }}</p><p class="text-xs text-muted-foreground">{{ stat.label }}</p>
        </div>
      </div>
      <p v-if="actionError" role="alert">{{ actionError }}</p>
      <div v-if="error" role="alert" class="mb-4"><p>{{ error }}</p><button type="button" @click="atlas.refresh('retry')">{{ $t('common.actions.retry') }}</button></div>

      <!-- Tabs -->
      <AppTabs :tabs="tabs" v-model="activeTab" class="mb-6" />

      <!-- ============ BROWSE TAB ============ -->
      <div v-if="activeTab === 'browse'" class="space-y-4">
        <!-- Search -->
        <input
          v-model="search"
          class="w-full rounded-lg border border-border bg-background px-4 py-2.5 text-sm text-foreground placeholder-muted-foreground/50 transition-colors focus:border-primary focus:outline-none focus:ring-1 focus:ring-primary"
          :placeholder="$t('skills.browse.searchPlaceholder')"
        >

        <div v-if="totalSkills === 0" class="py-16 text-center">
          <div class="mx-auto mb-4 flex h-16 w-16 items-center justify-center rounded-full bg-primary/10">
            <svg class="h-8 w-8 text-primary" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.5">
              <path stroke-linecap="round" stroke-linejoin="round" d="M9.813 15.904L9 18.75l-.813-2.846a4.5 4.5 0 00-3.09-3.09L2.25 12l2.846-.813a4.5 4.5 0 003.09-3.09L9 5.25l.813 2.846a4.5 4.5 0 003.09 3.09L15.75 12l-2.846.813a4.5 4.5 0 00-3.09 3.09z" />
            </svg>
          </div>
          <h3 class="text-lg font-semibold text-foreground">{{ $t('skills.browse.emptyTitle') }}</h3>
          <p class="mt-1 text-sm text-muted-foreground max-w-md mx-auto">
            {{ $t('skills.browse.emptyBody') }}
          </p>
        </div>

        <div v-else class="flex flex-col md:flex-row gap-4">
          <!-- Left: Hierarchy panel — collapsible on mobile -->
          <div class="w-full md:w-64 md:flex-shrink-0 space-y-3">
            <!-- Subject Fields -->
            <div class="rounded-xl bg-card shadow-sm p-4">
              <p class="text-[10px] font-semibold text-muted-foreground mb-3 tracking-wider uppercase">{{ $t('skills.browse.areas') }}</p>
              <button
                v-if="selectedField"
                class="w-full text-start text-xs px-2 py-1 mb-1 rounded-lg text-primary hover:bg-primary/10"
                @click="selectField(null)"
              >
                {{ $t('skills.browse.showAll') }}
              </button>
              <div
                v-for="field in fields"
                :key="field.id"
                class="rounded-lg px-2.5 py-2 text-sm cursor-pointer transition-colors"
                :class="selectedField === field.id
                  ? 'bg-primary/10 text-primary font-medium'
                  : 'text-foreground hover:bg-muted/50'"
                @click="selectField(field.id)"
              >
                <div class="flex items-center justify-between">
                  <span class="truncate">
                    <span v-if="field.icon_emoji" class="me-1.5">{{ field.icon_emoji }}</span>{{ field.name }}
                  </span>
                  <span class="text-xs text-muted-foreground tabular-nums">{{ field.skill_count }}</span>
                </div>
              </div>
              <p v-if="fields.length === 0" class="text-xs text-muted-foreground italic px-2">
                {{ $t('skills.browse.noAreas') }}
              </p>
            </div>

            <!-- Subjects -->
            <div class="rounded-xl bg-card shadow-sm p-4">
              <p class="text-[10px] font-semibold text-muted-foreground mb-3 tracking-wider uppercase">{{ $t('skills.browse.topics') }}</p>
              <button
                v-if="selectedSubject"
                class="w-full text-start text-xs px-2 py-1 mb-1 rounded-lg text-primary hover:bg-primary/10"
                @click="selectSubject(null)"
              >
                {{ $t('skills.browse.showAll') }}
              </button>
              <div
                v-for="subj in filteredSubjects"
                :key="subj.id"
                class="rounded-lg px-2.5 py-2 text-sm cursor-pointer transition-colors"
                :class="selectedSubject === subj.id
                  ? 'bg-primary/10 text-primary font-medium'
                  : 'text-foreground hover:bg-muted/50'"
                @click="selectSubject(subj.id)"
              >
                <div class="flex items-center justify-between">
                  <span class="truncate">{{ subj.name }}</span>
                  <span class="text-xs text-muted-foreground tabular-nums">{{ subj.skill_count }}</span>
                </div>
              </div>
              <p v-if="filteredSubjects.length === 0" class="text-xs text-muted-foreground italic px-2">
                {{ selectedField ? $t('skills.browse.noTopicsInArea') : $t('skills.browse.noTopics') }}
              </p>
            </div>

            <!-- Bloom level legend -->
            <div class="rounded-xl bg-card shadow-sm p-4">
              <p class="text-[10px] font-semibold text-muted-foreground mb-3 tracking-wider uppercase">{{ $t('skills.browse.levels') }}</p>
              <div class="space-y-1.5">
                <div v-for="level in BLOOM_ORDER" :key="level" class="flex items-center gap-2 text-xs">
                  <AppBadge :variant="bloomBadge(level)" class="text-[0.6rem] min-w-[5rem] justify-center">
                    {{ level }}
                  </AppBadge>
                </div>
              </div>
            </div>
          </div>

          <!-- Right: Skill list -->
          <div class="flex-1 min-w-0">
            <div v-if="filteredSkills.length === 0" class="rounded-xl bg-card shadow-sm p-8 text-center">
              <p class="text-sm text-muted-foreground">
                {{ $t('skills.browse.noMatches') }}
              </p>
            </div>

            <div v-else class="space-y-2">
              <p class="text-xs text-muted-foreground mb-3">
                {{ $t('skills.browse.skillCount', { count: filteredSkills.length }, filteredSkills.length) }}
              </p>

              <div
                v-for="skill in filteredSkills"
                :key="skill.id"
                class="rounded-xl bg-card shadow-sm p-4 cursor-pointer transition-shadow hover:shadow-md"
                @click="goToSkill(skill.id)"
              >
                <div class="flex items-start justify-between gap-3">
                  <div class="min-w-0 flex-1">
                    <div class="flex items-center gap-2 mb-1">
                      <h3 class="text-sm font-medium text-foreground truncate">
                        {{ skill.name }}
                      </h3>
                      <AppBadge :variant="bloomBadge(skill.bloom_level)" class="text-[0.6rem] flex-shrink-0">
                        {{ skill.bloom_level }}
                      </AppBadge>
                    </div>
                    <p v-if="skill.description" class="text-xs text-muted-foreground line-clamp-2">
                      {{ skill.description }}
                    </p>
                    <div class="flex items-center gap-3 mt-2 text-xs text-muted-foreground">
                      <span v-if="skill.subject_name">{{ skill.subject_field_name }} / {{ skill.subject_name }}</span>
                      <span v-if="skill.prerequisite_count > 0">{{ $t('skills.browse.prerequisitesCount', { count: skill.prerequisite_count }, skill.prerequisite_count) }}</span>
                      <span v-if="skill.dependent_count > 0">{{ $t('skills.browse.unlocksCount', { count: skill.dependent_count }, skill.dependent_count) }}</span>
                    </div>
                  </div>

                  <button
                    class="goal-btn shrink-0"
                    :title="$t('skills.browse.goalTitle', { name: skill.name })"
                    @click.stop="goalSkill(skill)"
                  >
                    {{ $t('skills.browse.goal') }}
                  </button>
                </div>
              </div>
            </div>
          </div>
        </div>
      </div>

      <!-- ============ GRAPH TAB ============ -->
      <div v-if="activeTab === 'graph'" class="space-y-6">
        <SkillAtlasExplorer v-if="model" :model="model" :view="pageView" :feedback="feedback"
          @update:view="atlas.updateView('page', $event)" @open-skill="openSkill" @learn-skill="learnSkill"
          @feedback-consumed="atlas.acknowledgeFeedback" />
        <GraphVisibilityEditor />
      </div>

      <!-- ============ CREDENTIALS TAB ============ -->
      <!-- Combined "Skills and Credentials" view: the full My Credentials
           page is embedded here so both live under one nav entry. -->
      <div v-if="activeTab === 'credentials'">
        <CredentialsPanel />
      </div>
    </template>
  </div>
</template>

<style scoped>
.goal-btn {
  font-size: 0.7rem;
  font-weight: 500;
  padding: 0.25rem 0.6rem;
  border-radius: 999px;
  background: color-mix(in srgb, var(--app-primary) 12%, transparent);
  color: var(--app-primary);
  white-space: nowrap;
  transition: background 0.15s;
}
.goal-btn:hover {
  background: color-mix(in srgb, var(--app-primary) 22%, transparent);
}
</style>
