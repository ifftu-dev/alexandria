<script setup lang="ts">
/**
 * Who can see the learner's skills, and the means to stop them.
 *
 * Each grant is something the learner signed. "Keep up to date" and "Stop
 * sharing" sign follow-ups to it, and the screen shows the grant as the
 * backend returns it afterwards rather than as the switch predicted: a
 * permission flipped on screen but not on the server would tell somebody they
 * had withdrawn consent when they had not.
 *
 * Below the grants sit the two older views of the same question — which
 * institutions see the learner by name, and who has looked — because "who
 * can see me" is one question to the person asking it.
 */
import { computed, onMounted, onUnmounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { useLocalApi } from '@/composables/useLocalApi'
import { AppButton, AppSwitch, ConfirmDialog } from '@/components/ui'
import OrgBadge from './OrgBadge.vue'
import { bloomName } from '@/utils/bloom'
import type { CloudPull, GrantView, GrantsView, OrgCheck, SyncReport } from '@/types'

const props = defineProps<{
  /** The Alexandria Cloud visibility changes are sent to. */
  cloudUrl: string
}>()

interface AccessEntry {
  directory: string
  institution: string
  role: string
  moduleId: string | null
  granularity: string
  at: string
}

interface Visibility {
  directory: string
  organisationId: string
  institution: string
  visibleToAdministrators: boolean
}

const { invoke } = useLocalApi()
const { t, locale } = useI18n()

const grants = ref<GrantView[]>([])
const visibility = ref<Visibility[]>([])
const views = ref<AccessEntry[]>([])
const problems = ref<string[]>([])
const loading = ref(false)
const changing = ref('')
const grantError = ref<Record<string, string>>({})
const error = ref('')
const syncing = ref(false)
const syncReport = ref<SyncReport | null>(null)
const stopping = ref<GrantView | null>(null)
let generation = 0

const active = computed(() => grants.value.filter((g) => g.revokedAt === null))
const revoked = computed(() => grants.value.filter((g) => g.revokedAt !== null))

const orgOf = (g: GrantView): OrgCheck => ({ name: g.orgName, domain: g.orgDomain, verified: true, problem: null })

function when(unix: number): string {
  return new Intl.DateTimeFormat(locale.value, { dateStyle: 'medium' }).format(new Date(unix * 1000))
}

function whenIso(iso: string): string {
  const parsed = new Date(iso)
  if (Number.isNaN(parsed.getTime())) return iso
  return new Intl.DateTimeFormat(locale.value, { dateStyle: 'medium', timeStyle: 'short' }).format(parsed)
}

/** "3 minutes ago", in the reader's language. */
function relative(unix: number): string {
  const seconds = Math.round(unix - Date.now() / 1000)
  const format = new Intl.RelativeTimeFormat(locale.value, { numeric: 'auto' })
  const steps: [Intl.RelativeTimeFormatUnit, number][] = [
    ['day', 86_400], ['hour', 3_600], ['minute', 60],
  ]
  for (const [unit, size] of steps) {
    if (Math.abs(seconds) >= size) return format.format(Math.round(seconds / size), unit)
  }
  return format.format(0, 'minute')
}

const bloomLabel = (rank: number | null) => (rank === null ? '' : t(`common.bloom.${bloomName(rank)}`))

/**
 * Keep what Cloud last said about views when a command answers without it:
 * the follow-up commands return the grant from the local record only.
 */
function replace(next: GrantView) {
  grants.value = grants.value.map((g) =>
    g.grantId === next.grantId
      ? { ...next, lastViewedAt: next.lastViewedAt ?? g.lastViewedAt, viewCount: next.viewCount ?? g.viewCount }
      : g,
  )
}

async function refresh() {
  const run = ++generation
  loading.value = true
  const [g, v, l] = await Promise.allSettled([
    invoke<GrantsView>('org_grants'),
    invoke<CloudPull<Visibility>>('fetch_visibility'),
    invoke<CloudPull<AccessEntry>>('fetch_access_log'),
  ])
  if (run !== generation) return
  const found: string[] = []
  if (g.status === 'fulfilled') {
    grants.value = g.value.grants
    found.push(...g.value.problems.map((p) => p.detail))
  } else found.push(String(g.reason))
  if (v.status === 'fulfilled') {
    visibility.value = v.value.items
    found.push(...v.value.problems.map((p) => p.detail))
  } else found.push(String(v.reason))
  if (l.status === 'fulfilled') {
    views.value = l.value.items
    found.push(...l.value.problems.map((p) => p.detail))
  } else found.push(String(l.reason))
  problems.value = [...new Set(found)]
  loading.value = false
}

async function setSync(g: GrantView, sync: boolean) {
  if (changing.value) return
  changing.value = g.grantId
  grantError.value = { ...grantError.value, [g.grantId]: '' }
  try {
    replace(await invoke<GrantView>('org_grant_set_sync', { grantId: g.grantId, sync }))
  } catch (e) {
    grantError.value = { ...grantError.value, [g.grantId]: String(e) }
  } finally {
    changing.value = ''
  }
}

async function confirmStop() {
  const g = stopping.value
  if (!g) return
  changing.value = g.grantId
  grantError.value = { ...grantError.value, [g.grantId]: '' }
  try {
    replace(await invoke<GrantView>('org_grant_revoke', { grantId: g.grantId }))
    stopping.value = null
  } catch (e) {
    grantError.value = { ...grantError.value, [g.grantId]: String(e) }
    stopping.value = null
  } finally {
    changing.value = ''
  }
}

async function syncNow() {
  syncing.value = true
  syncReport.value = null
  error.value = ''
  try {
    syncReport.value = await invoke<SyncReport>('org_sync_now')
    await refresh()
  } catch (e) {
    error.value = String(e)
  } finally {
    syncing.value = false
  }
}

/**
 * Re-read afterwards rather than flipped locally, for the same reason as the
 * grants: this is a permission held on a server.
 */
async function toggleVisibility(v: Visibility) {
  changing.value = v.organisationId
  error.value = ''
  try {
    await invoke('set_visibility', {
      directoryUrl: props.cloudUrl,
      organisationId: v.organisationId,
      visible: !v.visibleToAdministrators,
    })
    visibility.value = (await invoke<CloudPull<Visibility>>('fetch_visibility')).items
  } catch (e) {
    error.value = String(e)
  } finally {
    changing.value = ''
  }
}

defineExpose({ refresh })

onMounted(refresh)
onUnmounted(() => { generation++ })
</script>

<template>
  <section class="space-y-5 rounded-xl border border-border bg-card p-5" data-testid="org-access">
    <header class="flex items-start justify-between gap-3">
      <div>
        <h2 class="text-base font-semibold text-foreground">{{ t('organisations.access.title') }}</h2>
        <p class="mt-1 text-sm text-muted-foreground">{{ t('organisations.access.note') }}</p>
      </div>
      <AppButton variant="outline" size="sm" :disabled="syncing" @click="syncNow">
        {{ syncing ? t('organisations.access.syncing') : t('organisations.access.syncNow') }}
      </AppButton>
    </header>

    <p v-if="syncReport" role="status" class="text-sm text-muted-foreground">
      {{ t('organisations.access.syncReport', { sent: syncReport.sent, unchanged: syncReport.unchanged }) }}
      <span v-for="f in syncReport.failed" :key="f" class="block text-destructive">
        {{ t('organisations.access.syncFailedItem', { detail: f }) }}
      </span>
    </p>
    <p v-if="error" role="alert" class="text-sm text-destructive">{{ error }}</p>

    <p v-if="!loading && !active.length" class="text-sm text-muted-foreground">{{ t('organisations.access.empty') }}</p>

    <article
      v-for="g in active"
      :key="g.grantId"
      class="space-y-3 rounded-lg border border-border p-4"
      data-testid="grant"
    >
      <OrgBadge :org="orgOf(g)" />
      <p class="text-sm text-foreground">{{ t('organisations.access.purpose', { purpose: g.purpose }) }}</p>
      <ul class="flex flex-wrap gap-1.5">
        <li
          v-for="s in g.skills"
          :key="s.skillId"
          class="rounded-full border border-border bg-muted px-2 py-0.5 text-xs text-foreground"
        >
          {{ s.name }}<span v-if="s.bloomLevel !== null" class="ms-1 opacity-70">{{ bloomLabel(s.bloomLevel) }}</span>
        </li>
      </ul>
      <p class="text-xs text-muted-foreground">
        {{ t('organisations.access.sharedOn', { date: when(g.grantedAt) }) }}
        <template v-if="g.viewCount !== null">
          ·
          <template v-if="g.viewCount > 0 && g.lastViewedAt">
            {{ t('organisations.access.viewed', { count: g.viewCount, date: whenIso(g.lastViewedAt) }, g.viewCount) }}
          </template>
          <template v-else>{{ t('organisations.access.notViewed') }}</template>
        </template>
      </p>

      <div class="flex flex-wrap items-center gap-3 border-t border-border pt-3">
        <AppSwitch
          :model-value="g.sync"
          :disabled="changing === g.grantId"
          :label="t('organisations.access.keepUpToDate')"
          @update:model-value="(v: boolean) => setSync(g, v)"
        />
        <span class="min-w-0 flex-1">
          <span class="block text-sm text-foreground">{{ t('organisations.access.keepUpToDate') }}</span>
          <span v-if="g.syncError" class="block text-xs text-destructive">
            {{ t('organisations.access.syncFailed', { error: g.syncError }) }}
          </span>
          <span v-else-if="g.syncedAt" class="block text-xs text-muted-foreground">
            {{ t('organisations.access.updated', { relative: relative(g.syncedAt) }) }}
          </span>
        </span>
        <AppButton size="sm" variant="danger" :disabled="changing === g.grantId" @click="stopping = g">
          {{ t('organisations.access.stop') }}
        </AppButton>
      </div>
      <p v-if="grantError[g.grantId]" role="alert" class="text-sm text-destructive">{{ grantError[g.grantId] }}</p>
    </article>

    <details v-if="revoked.length" data-testid="revoked">
      <summary class="cursor-pointer text-sm font-medium text-foreground">
        {{ t('organisations.access.revokedHeading', { count: revoked.length }) }}
      </summary>
      <ul class="mt-2 space-y-1">
        <li v-for="g in revoked" :key="g.grantId" class="text-sm text-muted-foreground">
          {{ g.orgName }} ({{ g.orgDomain }}) · {{ t('organisations.access.stoppedOn', { date: when(g.revokedAt!) }) }}
        </li>
      </ul>
    </details>

    <div class="space-y-2">
      <h3 class="text-sm font-medium text-foreground">{{ t('organisations.access.visibility.heading') }}</h3>
      <p v-if="!visibility.length" class="text-sm text-muted-foreground">{{ t('organisations.access.visibility.empty') }}</p>
      <div
        v-for="v in visibility"
        :key="v.organisationId"
        class="flex items-center gap-3 rounded-lg border border-border p-3"
      >
        <span class="min-w-0 flex-1 text-sm text-foreground">
          {{
            v.visibleToAdministrators
              ? t('organisations.access.visibility.on', { institution: v.institution })
              : t('organisations.access.visibility.off', { institution: v.institution })
          }}
        </span>
        <AppButton variant="ghost" size="sm" :disabled="changing === v.organisationId" @click="toggleVisibility(v)">
          {{ v.visibleToAdministrators ? t('organisations.access.visibility.withdraw') : t('organisations.access.visibility.allow') }}
        </AppButton>
      </div>
      <p class="text-xs text-muted-foreground">{{ t('organisations.access.visibility.note') }}</p>
    </div>

    <div class="space-y-2">
      <h3 class="text-sm font-medium text-foreground">{{ t('organisations.access.views.heading') }}</h3>
      <p v-if="!views.length" class="text-sm text-muted-foreground">{{ t('organisations.access.views.empty') }}</p>
      <ul v-else class="space-y-1">
        <li v-for="(a, i) in views" :key="i" class="text-sm text-foreground">
          <template v-if="a.granularity === 'named'">
            {{ t('organisations.access.views.named', { who: a.role, where: a.institution }) }}
          </template>
          <template v-else>{{ t('organisations.access.views.aggregate', { where: a.institution }) }}</template>
          <span v-if="a.moduleId" class="text-muted-foreground">
            {{ t('organisations.access.views.module', { module: a.moduleId }) }}
          </span>
          <span class="text-xs text-muted-foreground"> · {{ whenIso(a.at) }}</span>
        </li>
      </ul>
    </div>

    <div v-if="problems.length" class="space-y-1" data-testid="access-problems">
      <h3 class="text-sm font-medium text-foreground">{{ t('organisations.problems.heading') }}</h3>
      <p v-for="p in problems" :key="p" role="alert" class="text-sm text-muted-foreground">{{ p }}</p>
      <p class="text-xs text-muted-foreground">{{ t('organisations.problems.note') }}</p>
    </div>

    <ConfirmDialog
      :open="stopping !== null"
      :title="t('organisations.access.stopTitle', { org: stopping?.orgName ?? '' })"
      :message="t('organisations.access.stopMessage', { org: stopping?.orgName ?? '' })"
      :confirm-label="t('organisations.access.stop')"
      confirm-variant="danger"
      :loading="!!stopping && changing === stopping.grantId"
      @confirm="confirmStop"
      @cancel="stopping = null"
    />
  </section>
</template>
