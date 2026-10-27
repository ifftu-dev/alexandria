import { onScopeDispose, readonly, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import { useProfiles } from '@/composables/useProfiles'
import { useSkillAtlas, type AtlasSurface } from '@/composables/useSkillAtlas'

/** Skill-scoped course discovery, with session ownership shared by both atlas surfaces. */
export function useSkillAtlasActions(surface: AtlasSurface) {
  const atlas = useSkillAtlas()
  const router = useRouter()
  const profiles = useProfiles()
  const actionError = ref<string | null>(null)
  let generation = 0
  let busy = false
  const invalidate = () => { generation++; busy = false; actionError.value = null }
  watch([profiles.activeProfileId, profiles.isLockBlocked], invalidate, { flush: 'sync' })
  onScopeDispose(invalidate)

  async function learnSkill(id: string): Promise<void> {
    const skill = atlas.model.value?.nodes.find(node => node.skill.id === id)?.skill
    if (!skill || busy || !profiles.activeProfileId.value || profiles.isLockBlocked.value) return
    const ticket = generation
    busy = true
    actionError.value = null
    try {
      const failure = await router.push({ path: '/courses', query: { skillId: id } })
      if (!failure && ticket === generation && surface === 'modal') atlas.setModalOpen(false)
    } catch (cause) {
      if (ticket === generation) actionError.value = cause instanceof Error ? cause.message : String(cause)
    } finally { if (ticket === generation) busy = false }
  }
  async function openSkill(id: string): Promise<void> {
    actionError.value = null
    const ticket = generation
    try { await atlas.openSkill(router, surface, id) }
    catch (cause) { if (ticket === generation) actionError.value = cause instanceof Error ? cause.message : String(cause) }
  }
  return { learnSkill, openSkill, actionError: readonly(actionError) }
}
