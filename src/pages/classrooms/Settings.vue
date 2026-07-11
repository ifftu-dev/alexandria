<template>
  <div>
    <!-- Header -->
    <div class="border-b border-border px-6 py-4 flex items-center gap-3">
      <RouterLink
        :to="{ name: 'classroom', params: { id: classroomId } }"
        class="text-muted-foreground hover:text-foreground transition-colors"
      >
        ← {{ $t('common.actions.back') }}
      </RouterLink>
      <div>
        <h1 class="text-xl font-bold text-foreground">{{ $t('classrooms.settings.title') }}</h1>
        <p class="text-sm text-muted-foreground">{{ currentClassroom?.name }}</p>
      </div>
    </div>

    <div class="max-w-2xl mx-auto px-4 sm:px-6 py-8 space-y-6">
      <!-- General info -->
      <section class="card p-6">
        <h2 class="text-base font-semibold text-foreground mb-4">{{ $t('classrooms.settings.general') }}</h2>
        <div class="space-y-4">
          <div>
            <label class="block text-sm text-muted-foreground mb-1">{{ $t('classrooms.settings.inviteCode') }}</label>
            <div class="flex gap-2">
              <code class="flex-1 px-3 py-2 bg-muted border border-border rounded-lg text-foreground text-sm font-mono truncate">
                {{ currentClassroom?.invite_code ?? '—' }}
              </code>
              <AppButton variant="secondary" size="sm" @click="copyInviteCode">
                {{ $t('common.actions.copy') }}
              </AppButton>
            </div>
            <p class="text-xs text-muted-foreground mt-1">{{ $t('classrooms.settings.inviteHint') }}</p>
          </div>
        </div>
      </section>

      <!-- Members -->
      <section class="card p-6">
        <h2 class="text-base font-semibold text-foreground mb-4">
          {{ $t('classrooms.settings.membersHeading') }} ({{ members.length }})
        </h2>
        <div class="space-y-2">
          <div
            v-for="m in members"
            :key="m.stake_address"
            class="flex items-center justify-between py-2"
          >
            <div class="flex items-center gap-3">
              <div class="w-8 h-8 rounded-full bg-muted flex items-center justify-center text-sm text-muted-foreground flex-shrink-0">
                {{ (m.display_name ?? m.stake_address).charAt(0).toUpperCase() }}
              </div>
              <div class="min-w-0">
                <div class="text-sm text-foreground truncate">
                  {{ m.display_name ?? formatAddress(m.stake_address) }}
                </div>
                <div class="text-xs text-muted-foreground capitalize">{{ m.role }}</div>
              </div>
            </div>

            <!-- Role + kick actions (owner only, not for self) -->
            <div
              v-if="currentClassroom?.my_role === 'owner' && m.role !== 'owner'"
              class="flex items-center gap-2 flex-shrink-0"
            >
              <select
                :value="m.role"
                @change="handleSetRole(m.stake_address, ($event.target as HTMLSelectElement).value)"
                class="text-xs bg-muted border border-border rounded px-2 py-1 text-foreground"
              >
                <option value="member">{{ $t('classrooms.roles.member') }}</option>
                <option value="moderator">{{ $t('classrooms.roles.moderator') }}</option>
              </select>
              <AppButton variant="ghost" size="xs" @click="handleKick(m.stake_address)" class="text-destructive hover:text-destructive">
                {{ $t('classrooms.settings.kick') }}
              </AppButton>
            </div>
          </div>
        </div>
      </section>

      <!-- Danger zone -->
      <section class="card p-6 border-destructive/30">
        <h2 class="text-base font-semibold text-destructive mb-4">{{ $t('classrooms.settings.dangerZone') }}</h2>
        <div class="flex items-center justify-between gap-4">
          <div class="min-w-0">
            <p class="text-sm text-foreground">{{ $t('classrooms.settings.archiveTitle') }}</p>
            <p class="text-xs text-muted-foreground">{{ $t('classrooms.settings.archiveBody') }}</p>
          </div>
          <AppButton variant="danger" size="sm" @click="handleArchive">
            {{ $t('classrooms.settings.archive') }}
          </AppButton>
        </div>
      </section>

      <!-- Leave classroom -->
      <section
        v-if="currentClassroom?.my_role !== 'owner'"
        class="card p-6"
      >
        <div class="flex items-center justify-between gap-4">
          <div class="min-w-0">
            <p class="text-sm text-foreground">{{ $t('classrooms.settings.leaveTitle') }}</p>
            <p class="text-xs text-muted-foreground">{{ $t('classrooms.settings.leaveBody') }}</p>
          </div>
          <AppButton variant="secondary" size="sm" @click="handleLeave">
            {{ $t('classrooms.settings.leave') }}
          </AppButton>
        </div>
      </section>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import { useRoute, useRouter } from 'vue-router'
import { useLocalApi } from '@/composables/useLocalApi'
import { useClassroom } from '@/composables/useClassroom'
import AppButton from '@/components/ui/AppButton.vue'

const { t } = useI18n()
const route = useRoute()
const router = useRouter()
const classroomId = computed(() => route.params.id as string)

const { currentClassroom, members, leaveClassroom } = useClassroom()
const { invoke } = useLocalApi()

function formatAddress(addr: string): string {
  return addr.length > 12 ? `${addr.slice(0, 8)}...${addr.slice(-4)}` : addr
}

function copyInviteCode() {
  const code = currentClassroom.value?.invite_code
  if (code) navigator.clipboard.writeText(code)
}

async function handleSetRole(stakeAddress: string, role: string) {
  try {
    await invoke('classroom_set_role', {
      classroomId: classroomId.value,
      stakeAddress,
      role,
    })
  } catch (e) {
    console.error(e)
  }
}

async function handleKick(stakeAddress: string) {
  if (!confirm(t('classrooms.settings.confirmKick'))) return
  try {
    await invoke('classroom_kick_member', {
      classroomId: classroomId.value,
      stakeAddress,
    })
  } catch (e) {
    console.error(e)
  }
}

async function handleArchive() {
  if (!confirm(t('classrooms.settings.confirmArchive'))) return
  try {
    await invoke('classroom_archive', { classroomId: classroomId.value })
    router.push({ name: 'classrooms' })
  } catch (e) {
    console.error(e)
  }
}

async function handleLeave() {
  if (!confirm(t('classrooms.settings.confirmLeave'))) return
  try {
    await leaveClassroom(classroomId.value)
    router.push({ name: 'classrooms' })
  } catch (e) {
    console.error(e)
  }
}
</script>
