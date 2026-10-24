import { describe, expect, it } from 'vitest'
import { defineComponent, h, nextTick, ref } from 'vue'
import { mount } from '@vue/test-utils'
import { chatOffers, chatRestrictedByPage, useChatOffer, useChatRestriction } from '../useChatOffers'
import type { ChatOffer } from '@/types'

describe('chat offers', () => {
  it('registers an offer while mounted and removes it on unmount', async () => {
    const offer = ref<ChatOffer | null>({ kind: 'lesson', course_id: 'c', element_id: 'e', label: 'Lesson' })
    const restricted = ref(false)
    const Page = defineComponent({
      setup() {
        useChatOffer(offer)
        useChatRestriction(restricted)
        return () => h('div')
      },
    })
    const wrapper = mount(Page)
    expect(chatOffers.value).toHaveLength(1)
    expect(chatRestrictedByPage.value).toBe(false)

    offer.value = null
    restricted.value = true
    await nextTick()
    expect(chatOffers.value).toHaveLength(0)
    expect(chatRestrictedByPage.value).toBe(true)

    wrapper.unmount()
    expect(chatRestrictedByPage.value).toBe(false)
  })
})
