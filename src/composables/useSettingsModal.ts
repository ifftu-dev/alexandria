import { ref, readonly } from 'vue'

export type SettingsSectionId = 'account' | 'security' | 'personalization' | 'system'

const isOpen = ref(false)
const activeSection = ref<SettingsSectionId>('account')

export function useSettingsModal() {
  function open(section?: SettingsSectionId) {
    if (section) activeSection.value = section
    isOpen.value = true
  }

  function close() {
    isOpen.value = false
  }

  function setSection(section: SettingsSectionId) {
    activeSection.value = section
  }

  return {
    isOpen: readonly(isOpen),
    activeSection: readonly(activeSection),
    open,
    close,
    setSection,
  }
}
