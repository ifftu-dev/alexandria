import { computed } from 'vue'
import { useRoute } from 'vue-router'
import { useChat } from './useChat'
import { CHAT_RESTRICTED_ROUTES, chatRestrictedByPage } from './useChatOffers'

/**
 * Whether chat may be shown right now. False on assessment and interview
 * routes, on pages that mark themselves restricted, and whenever the backend
 * reports an assessment in progress. Call from a component's setup.
 */
export function useChatAvailability() {
  const route = useRoute()
  const chat = useChat()
  const restricted = computed(
    () =>
      CHAT_RESTRICTED_ROUTES.has(String(route.name ?? '')) ||
      chatRestrictedByPage.value ||
      chat.status.value.assessment_active,
  )
  return { restricted }
}
