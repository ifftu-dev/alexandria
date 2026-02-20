<script setup lang="ts">
import { useRouter, useRoute } from 'vue-router'

interface Props {
  collapsed: boolean
}

defineProps<Props>()
const emit = defineEmits<{ toggle: [] }>()
const router = useRouter()
const route = useRoute()

const navItems = [
  { label: 'Home', path: '/home', icon: 'home' },
  { label: 'Courses', path: '/courses', icon: 'book' },
  { label: 'My Courses', path: '/dashboard/courses', icon: 'bookmark' },
  { label: 'Settings', path: '/dashboard/settings', icon: 'settings' },
]

function isActive(path: string) {
  return route.path === path
}

function navigate(path: string) {
  router.push(path)
}
</script>

<template>
  <aside
    class="flex flex-col border-r border-[rgb(var(--color-border))] bg-[rgb(var(--color-card))] transition-all duration-200"
    :class="collapsed ? 'w-16' : 'w-56'"
  >
    <!-- Logo -->
    <div class="flex items-center h-14 px-4 border-b border-[rgb(var(--color-border))]">
      <span v-if="!collapsed" class="font-semibold text-sm tracking-tight">Alexandria</span>
      <span v-else class="font-bold text-sm">A</span>
    </div>

    <!-- Navigation -->
    <nav class="flex-1 py-2 space-y-0.5 px-2">
      <button
        v-for="item in navItems"
        :key="item.path"
        class="flex items-center w-full rounded-md px-2.5 py-2 text-sm transition-colors"
        :class="isActive(item.path)
          ? 'bg-[rgb(var(--color-primary)/0.1)] text-[rgb(var(--color-primary))] font-medium'
          : 'text-[rgb(var(--color-muted-foreground))] hover:bg-[rgb(var(--color-muted)/0.5)] hover:text-[rgb(var(--color-foreground))]'"
        @click="navigate(item.path)"
      >
        <span class="w-5 h-5 flex items-center justify-center text-xs">
          {{ item.icon === 'home' ? '\u2302' : item.icon === 'book' ? '\u{1F4D6}' : item.icon === 'bookmark' ? '\u2605' : '\u2699' }}
        </span>
        <span v-if="!collapsed" class="ml-2.5 truncate">{{ item.label }}</span>
      </button>
    </nav>

    <!-- Collapse toggle -->
    <div class="p-2 border-t border-[rgb(var(--color-border))]">
      <button
        class="flex items-center justify-center w-full rounded-md p-2 text-xs text-[rgb(var(--color-muted-foreground))] hover:bg-[rgb(var(--color-muted)/0.5)]"
        @click="emit('toggle')"
      >
        {{ collapsed ? '\u25B6' : '\u25C0' }}
      </button>
    </div>
  </aside>
</template>
