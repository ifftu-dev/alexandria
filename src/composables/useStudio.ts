import { readonly, ref } from 'vue'
import { useLocalApi } from '@/composables/useLocalApi'
import { onProfileLocked } from '@/composables/useProfiles'
import type { StudioConnection, StudioDocument, StudioSettings, StudioWorkflow } from '@/types'

const connections = ref<StudioDocument<StudioConnection>[]>([])
const workflows = ref<StudioDocument<StudioWorkflow>[]>([])
const settings = ref<StudioDocument<StudioSettings> | null>(null)
let generation = 0
onProfileLocked(() => {
  generation++
  connections.value = []
  workflows.value = []
  settings.value = null
})

export function useStudio() {
  const { invoke } = useLocalApi()
  async function refresh() {
    const epoch = generation
    const [nextConnections, nextWorkflows, nextSettings] = await Promise.all([
      invoke<StudioDocument<StudioConnection>[]>('studio_list_connections'),
      invoke<StudioDocument<StudioWorkflow>[]>('studio_list_workflows'),
      invoke<StudioDocument<StudioSettings>>('studio_get_settings'),
    ])
    if (epoch !== generation) return
    connections.value = nextConnections
    workflows.value = nextWorkflows
    settings.value = nextSettings
  }
  return { connections: readonly(connections), workflows: readonly(workflows), settings: readonly(settings), refresh }
}
