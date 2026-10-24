import { computed, onBeforeUnmount, ref, watch, type Ref } from 'vue'
import type { ChatOffer } from '@/types'

/**
 * What the current page offers to chat, and whether the page forbids chat.
 *
 * A page registers an offer such as "this lesson"; registering attaches and
 * sends nothing. The chat panel shows the offer, and only an explicit click
 * attaches it. A page can also mark itself restricted (an assessment element
 * in the course player): the chat host is then unmounted, not hidden. The
 * backend refuses chat during assessments independently; this is the
 * presentation half.
 */

let nextId = 0
const offers = ref(new Map<number, ChatOffer[]>())
const restrictions = ref(new Set<number>())

export const chatOffers = computed(() => [...offers.value.values()].flat())
export const chatRestrictedByPage = computed(() => restrictions.value.size > 0)

/** Offer `source` to chat while the calling component is mounted. */
export function useChatOffer(source: Ref<ChatOffer | null>) {
  useChatOfferList(computed(() => (source.value ? [source.value] : [])))
}

/** Offer several items, such as each of the learner's goals. */
export function useChatOfferList(source: Ref<ChatOffer[]>) {
  const id = ++nextId
  const stop = watch(
    source,
    (list) => {
      const next = new Map(offers.value)
      if (list.length) next.set(id, list.map((o) => ({ ...o })))
      else next.delete(id)
      offers.value = next
    },
    { immediate: true, deep: true },
  )
  onBeforeUnmount(() => {
    stop()
    const next = new Map(offers.value)
    next.delete(id)
    offers.value = next
  })
}

/** Unmount chat while `restricted` is true and the caller is mounted. */
export function useChatRestriction(restricted: Ref<boolean>) {
  const id = ++nextId
  const stop = watch(
    restricted,
    (value) => {
      const next = new Set(restrictions.value)
      if (value) next.add(id)
      else next.delete(id)
      restrictions.value = next
    },
    { immediate: true },
  )
  onBeforeUnmount(() => {
    stop()
    const next = new Set(restrictions.value)
    next.delete(id)
    restrictions.value = next
  })
}

/**
 * Routes where chat never mounts: assessment attempts and interview calls.
 * The backend refuses chat during any live integrity session regardless.
 */
export const CHAT_RESTRICTED_ROUTES = new Set(['assessment', 'interview-call', 'interview-room'])
