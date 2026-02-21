<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
import { useRouter } from 'vue-router'
import { useLocalApi } from '@/composables/useLocalApi'
import { AppSpinner, AppBadge, AppInput, AppTabs, EmptyState } from '@/components/ui'
import SkillGraph from '@/components/skills/SkillGraph.vue'
import type { SubjectFieldInfo, SubjectInfo, SkillInfo, SkillGraphEdge, SkillProof } from '@/types'

const { invoke } = useLocalApi()
const router = useRouter()

const loading = ref(true)
const fields = ref<SubjectFieldInfo[]>([])
const subjects = ref<SubjectInfo[]>([])
const skills = ref<SkillInfo[]>([])
const graphEdges = ref<SkillGraphEdge[]>([])
const proofs = ref<SkillProof[]>([])

const search = ref('')
const selectedField = ref<string | null>(null)
const selectedSubject = ref<string | null>(null)

const activeTab = ref('browse')
const tabs = [
  { key: 'browse', label: 'Browse' },
  { key: 'graph', label: 'Graph' },
  { key: 'proofs', label: 'My Proofs' },
]

onMounted(async () => {
  try {
    const [f, s, sk, edges, p] = await Promise.all([
      invoke<SubjectFieldInfo[]>('list_subject_fields'),
      invoke<SubjectInfo[]>('list_subjects'),
      invoke<SkillInfo[]>('list_skills'),
      invoke<SkillGraphEdge[]>('list_skill_graph_edges'),
      invoke<SkillProof[]>('list_skill_proofs'),
    ])
    fields.value = f
    subjects.value = s
    skills.value = sk
    graphEdges.value = edges
    proofs.value = p
  } catch (e) {
    console.error('Failed to load taxonomy:', e)
  } finally {
    loading.value = false
  }
})

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

// Proof lookup by skill ID
const proofMap = computed(() => {
  const map = new Map<string, SkillProof>()
  for (const p of proofs.value) {
    map.set(p.skill_id, p)
  }
  return map
})

// Stats
const totalSkills = computed(() => skills.value.length)
const totalSubjects = computed(() => subjects.value.length)
const totalFields = computed(() => fields.value.length)
const totalEdges = computed(() => graphEdges.value.length)

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

const bloomColors: Record<string, string> = {
  remember: 'secondary',
  understand: 'primary',
  apply: 'accent',
  analyze: 'warning',
  evaluate: 'success',
  create: 'governance',
}

const bloomOrder = ['remember', 'understand', 'apply', 'analyze', 'evaluate', 'create']
</script>

<template>
  <div class="space-y-6">
    <!-- Header -->
    <div>
      <h1 class="text-xl font-bold text-[rgb(var(--color-foreground))]">Skill Taxonomy</h1>
      <p class="mt-1 text-sm text-[rgb(var(--color-muted-foreground))]">
        Browse the knowledge graph: subject fields, subjects, and skills with prerequisite relationships.
      </p>
    </div>

    <AppSpinner v-if="loading" label="Loading taxonomy..." />

    <template v-else>
      <!-- Stats bar -->
      <div class="grid grid-cols-4 gap-3">
        <div class="card p-3 text-center">
          <p class="font-mono text-xl font-bold text-[rgb(var(--color-foreground))]">{{ totalFields }}</p>
          <p class="text-xs text-[rgb(var(--color-muted-foreground))]">Fields</p>
        </div>
        <div class="card p-3 text-center">
          <p class="font-mono text-xl font-bold text-[rgb(var(--color-foreground))]">{{ totalSubjects }}</p>
          <p class="text-xs text-[rgb(var(--color-muted-foreground))]">Subjects</p>
        </div>
        <div class="card p-3 text-center">
          <p class="font-mono text-xl font-bold text-[rgb(var(--color-primary))]">{{ totalSkills }}</p>
          <p class="text-xs text-[rgb(var(--color-muted-foreground))]">Skills</p>
        </div>
        <div class="card p-3 text-center">
          <p class="font-mono text-xl font-bold text-[rgb(var(--color-foreground))]">{{ totalEdges }}</p>
          <p class="text-xs text-[rgb(var(--color-muted-foreground))]">Prerequisites</p>
        </div>
      </div>

      <!-- Tabs -->
      <AppTabs :tabs="tabs" v-model="activeTab" />

      <!-- ============ BROWSE TAB ============ -->
      <div v-if="activeTab === 'browse'" class="space-y-4">
        <!-- Search -->
        <AppInput
          v-model="search"
          placeholder="Search skills by name, description, or level..."
        />

        <div v-if="totalSkills === 0">
          <EmptyState
            title="No skills in the taxonomy"
            description="Skills are added through the governance taxonomy proposal workflow. Create a DAO, propose a taxonomy change, and ratify it to populate the skill graph."
          />
        </div>

        <div v-else class="flex gap-4">
          <!-- Left: Hierarchy panel -->
          <div class="w-64 flex-shrink-0 space-y-2">
            <!-- Subject Fields -->
            <div class="card p-3">
              <p class="text-xs font-semibold text-[rgb(var(--color-muted-foreground))] mb-2 tracking-wider uppercase">Subject Fields</p>
              <button
                v-if="selectedField"
                class="w-full text-left text-xs px-2 py-1 mb-1 rounded text-[rgb(var(--color-primary))] hover:bg-[rgb(var(--color-primary)/0.1)]"
                @click="selectField(null)"
              >
                Show all
              </button>
              <div
                v-for="field in fields"
                :key="field.id"
                class="rounded-md px-2 py-1.5 text-sm cursor-pointer transition-colors"
                :class="selectedField === field.id
                  ? 'bg-[rgb(var(--color-primary)/0.1)] text-[rgb(var(--color-primary))] font-medium'
                  : 'text-[rgb(var(--color-foreground))] hover:bg-[rgb(var(--color-muted)/0.5)]'"
                @click="selectField(field.id)"
              >
                <div class="flex items-center justify-between">
                  <span class="truncate">{{ field.name }}</span>
                  <span class="text-xs text-[rgb(var(--color-muted-foreground))]">{{ field.skill_count }}</span>
                </div>
              </div>
              <p v-if="fields.length === 0" class="text-xs text-[rgb(var(--color-muted-foreground))] italic px-2">
                No subject fields
              </p>
            </div>

            <!-- Subjects (filtered by selected field) -->
            <div class="card p-3">
              <p class="text-xs font-semibold text-[rgb(var(--color-muted-foreground))] mb-2 tracking-wider uppercase">Subjects</p>
              <button
                v-if="selectedSubject"
                class="w-full text-left text-xs px-2 py-1 mb-1 rounded text-[rgb(var(--color-primary))] hover:bg-[rgb(var(--color-primary)/0.1)]"
                @click="selectSubject(null)"
              >
                Show all
              </button>
              <div
                v-for="subj in filteredSubjects"
                :key="subj.id"
                class="rounded-md px-2 py-1.5 text-sm cursor-pointer transition-colors"
                :class="selectedSubject === subj.id
                  ? 'bg-[rgb(var(--color-primary)/0.1)] text-[rgb(var(--color-primary))] font-medium'
                  : 'text-[rgb(var(--color-foreground))] hover:bg-[rgb(var(--color-muted)/0.5)]'"
                @click="selectSubject(subj.id)"
              >
                <div class="flex items-center justify-between">
                  <span class="truncate">{{ subj.name }}</span>
                  <span class="text-xs text-[rgb(var(--color-muted-foreground))]">{{ subj.skill_count }}</span>
                </div>
              </div>
              <p v-if="filteredSubjects.length === 0" class="text-xs text-[rgb(var(--color-muted-foreground))] italic px-2">
                No subjects{{ selectedField ? ' in this field' : '' }}
              </p>
            </div>

            <!-- Bloom level legend -->
            <div class="card p-3">
              <p class="text-xs font-semibold text-[rgb(var(--color-muted-foreground))] mb-2 tracking-wider uppercase">Bloom's Levels</p>
              <div class="space-y-1">
                <div v-for="level in bloomOrder" :key="level" class="flex items-center gap-2 text-xs">
                  <AppBadge :variant="(bloomColors[level] as any) ?? 'secondary'" class="text-[0.6rem] min-w-[5rem] justify-center">
                    {{ level }}
                  </AppBadge>
                </div>
              </div>
            </div>
          </div>

          <!-- Right: Skill list -->
          <div class="flex-1 min-w-0">
            <div v-if="filteredSkills.length === 0" class="card p-8 text-center">
              <p class="text-sm text-[rgb(var(--color-muted-foreground))]">
                No skills match the current filters.
              </p>
            </div>

            <div v-else class="space-y-2">
              <div class="text-xs text-[rgb(var(--color-muted-foreground))] mb-2">
                {{ filteredSkills.length }} skill{{ filteredSkills.length !== 1 ? 's' : '' }}
              </div>

              <div
                v-for="skill in filteredSkills"
                :key="skill.id"
                class="card card-interactive p-4 cursor-pointer"
                @click="goToSkill(skill.id)"
              >
                <div class="flex items-start justify-between gap-3">
                  <div class="min-w-0 flex-1">
                    <div class="flex items-center gap-2 mb-1">
                      <h3 class="text-sm font-medium text-[rgb(var(--color-foreground))] truncate">
                        {{ skill.name }}
                      </h3>
                      <AppBadge :variant="(bloomColors[skill.bloom_level] as any) ?? 'secondary'" class="text-[0.6rem] flex-shrink-0">
                        {{ skill.bloom_level }}
                      </AppBadge>
                    </div>
                    <p v-if="skill.description" class="text-xs text-[rgb(var(--color-muted-foreground))] line-clamp-2">
                      {{ skill.description }}
                    </p>
                    <div class="flex items-center gap-3 mt-2 text-xs text-[rgb(var(--color-muted-foreground))]">
                      <span v-if="skill.subject_name">{{ skill.subject_field_name }} / {{ skill.subject_name }}</span>
                      <span v-if="skill.prerequisite_count > 0">{{ skill.prerequisite_count }} prerequisite{{ skill.prerequisite_count !== 1 ? 's' : '' }}</span>
                      <span v-if="skill.dependent_count > 0">{{ skill.dependent_count }} dependent{{ skill.dependent_count !== 1 ? 's' : '' }}</span>
                    </div>
                  </div>

                  <!-- Proof indicator -->
                  <div v-if="proofMap.get(skill.id)" class="text-right flex-shrink-0">
                    <p class="font-mono text-sm font-bold text-[rgb(var(--color-primary))]">
                      {{ (proofMap.get(skill.id)!.confidence * 100).toFixed(0) }}%
                    </p>
                    <p class="text-[0.6rem] text-[rgb(var(--color-muted-foreground))]">proven</p>
                  </div>
                </div>
              </div>
            </div>
          </div>
        </div>
      </div>

      <!-- ============ GRAPH TAB ============ -->
      <div v-if="activeTab === 'graph'">
        <div v-if="skills.length === 0">
          <EmptyState
            title="No skills to graph"
            description="Add skills through the governance taxonomy proposal workflow to see the prerequisite graph."
          />
        </div>
        <SkillGraph
          v-else
          :skills="skills"
          :edges="graphEdges"
          :proofs="proofMap"
          @select="goToSkill"
        />
      </div>

      <!-- ============ PROOFS TAB ============ -->
      <div v-if="activeTab === 'proofs'">
        <EmptyState
          v-if="proofs.length === 0"
          title="No skill proofs yet"
          description="Complete course assessments to earn skill proofs. Each proof attests proficiency at a specific Bloom's taxonomy level."
        />

        <div v-else class="grid grid-cols-1 sm:grid-cols-2 gap-3">
          <div
            v-for="proof in proofs"
            :key="proof.id"
            class="card card-interactive p-4 cursor-pointer"
            @click="goToSkill(proof.skill_id)"
          >
            <div class="flex items-start justify-between mb-2">
              <div class="min-w-0">
                <div class="text-sm font-medium truncate">
                  {{ skills.find(s => s.id === proof.skill_id)?.name ?? proof.skill_id }}
                </div>
                <AppBadge :variant="(bloomColors[proof.proficiency_level] as any) ?? 'secondary'" class="mt-1">
                  {{ proof.proficiency_level }}
                </AppBadge>
              </div>
              <div class="text-right flex-shrink-0">
                <div class="font-mono text-lg font-bold text-[rgb(var(--color-primary))]">
                  {{ (proof.confidence * 100).toFixed(0) }}%
                </div>
                <div class="text-xs text-[rgb(var(--color-muted-foreground))]">confidence</div>
              </div>
            </div>
            <div class="h-1.5 rounded-full bg-[rgb(var(--color-muted))] overflow-hidden">
              <div
                class="h-full rounded-full bg-[rgb(var(--color-primary))] transition-all duration-500"
                :style="{ width: `${proof.confidence * 100}%` }"
              />
            </div>
            <div class="flex items-center justify-between mt-2 text-xs text-[rgb(var(--color-muted-foreground))]">
              <span>{{ proof.evidence_count }} evidence</span>
              <span>{{ proof.updated_at }}</span>
            </div>
          </div>
        </div>
      </div>
    </template>
  </div>
</template>
