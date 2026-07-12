<script setup lang="ts">
// Bloom-coded skill tagging for one element — extracted from the old
// CourseEdit page so the composer and any future surface share it.
import { computed, onMounted, ref } from 'vue'
import { useLocalApi } from '@/composables/useLocalApi'
import { AppBadge } from '@/components/ui'
import type { ElementSkillTag, SkillInfo } from '@/types'

const props = defineProps<{ elementId: string }>()

const { invoke } = useLocalApi()

const tags = ref<ElementSkillTag[]>([])
const allSkills = ref<SkillInfo[]>([])
const picking = ref(false)
const search = ref('')
const error = ref('')

const bloomColors: Record<string, string> = {
  remember: 'var(--color-muted-foreground)',
  understand: '59 130 246',
  apply: '16 185 129',
  analyze: '245 158 11',
  evaluate: '239 68 68',
  create: '139 92 246',
}

const results = computed<SkillInfo[]>(() => {
  const q = search.value.toLowerCase().trim()
  if (!q) return allSkills.value.slice(0, 20)
  return allSkills.value
    .filter(s => s.name.toLowerCase().includes(q) || (s.subject_name || '').toLowerCase().includes(q))
    .slice(0, 20)
})

const isTagged = (skillId: string) => tags.value.some(t => t.skill_id === skillId)

onMounted(async () => {
  tags.value = await invoke<ElementSkillTag[]>('list_element_skill_tags', { elementId: props.elementId }).catch(() => [])
  allSkills.value = await invoke<SkillInfo[]>('list_skills', {}).catch(() => [])
})

async function tag(skill: SkillInfo) {
  if (isTagged(skill.id)) return
  try {
    await invoke('tag_element_skill', { elementId: props.elementId, skillId: skill.id })
    tags.value.push({
      skill_id: skill.id,
      skill_name: skill.name,
      bloom_level: skill.bloom_level,
      weight: 1.0,
    })
    search.value = ''
    picking.value = false
  } catch (e) {
    error.value = String(e)
  }
}

async function untag(skillId: string) {
  try {
    await invoke('untag_element_skill', { elementId: props.elementId, skillId })
    tags.value = tags.value.filter(t => t.skill_id !== skillId)
  } catch (e) {
    error.value = String(e)
  }
}
</script>

<template>
  <div>
    <div class="flex flex-wrap items-center gap-1.5">
      <button
        v-for="t in tags"
        :key="t.skill_id"
        class="inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-xs font-medium transition-colors hover:opacity-80"
        :style="{ backgroundColor: `rgb(${bloomColors[t.bloom_level] || bloomColors.apply} / 0.15)`, color: `rgb(${bloomColors[t.bloom_level] || bloomColors.apply})` }"
        :title="$t('instructor.skillPicker.removeSkill', { name: t.skill_name })"
        @click="untag(t.skill_id)"
      >
        {{ t.skill_name }}
        <svg class="w-3 h-3 opacity-60" viewBox="0 0 20 20" fill="currentColor"><path d="M6.28 5.22a.75.75 0 00-1.06 1.06L8.94 10l-3.72 3.72a.75.75 0 101.06 1.06L10 11.06l3.72 3.72a.75.75 0 101.06-1.06L11.06 10l3.72-3.72a.75.75 0 00-1.06-1.06L10 8.94 6.28 5.22z"/></svg>
      </button>

      <div v-if="picking" class="relative">
        <input
          v-model="search"
          class="rounded-lg border border-border bg-background px-2 py-1 text-xs w-48"
          :placeholder="$t('instructor.skillPicker.searchPlaceholder')"
          @keydown.escape="picking = false; search = ''"
        >
        <div
          v-if="results.length"
          class="absolute z-20 top-full start-0 mt-1 w-64 max-h-48 overflow-y-auto rounded-lg border border-border bg-card shadow-lg"
        >
          <button
            v-for="skill in results"
            :key="skill.id"
            class="w-full text-start px-3 py-1.5 text-xs hover:bg-muted/30 flex items-center justify-between gap-2 disabled:opacity-40"
            :disabled="isTagged(skill.id)"
            @click="tag(skill)"
          >
            <span class="truncate">{{ skill.name }}</span>
            <AppBadge size="xs">{{ skill.bloom_level }}</AppBadge>
          </button>
        </div>
      </div>
      <button
        v-else
        class="inline-flex items-center gap-0.5 px-1.5 py-0.5 rounded-full text-xs text-muted-foreground border border-dashed border-border hover:border-primary hover:text-primary transition-colors"
        @click="picking = true; search = ''"
      >
        <svg class="w-3 h-3" viewBox="0 0 20 20" fill="currentColor"><path d="M10.75 4.75a.75.75 0 00-1.5 0v4.5h-4.5a.75.75 0 000 1.5h4.5v4.5a.75.75 0 001.5 0v-4.5h4.5a.75.75 0 000-1.5h-4.5v-4.5z"/></svg>
        {{ $t('instructor.skillPicker.addSkill') }}
      </button>
    </div>
    <p v-if="error" class="mt-1 text-xs text-error">{{ error }}</p>
  </div>
</template>
