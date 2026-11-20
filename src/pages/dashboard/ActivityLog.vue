<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { AppAlert, AppButton, ConfirmDialog, EmptyState } from '@/components/ui'
import ActivityLogRow from '@/components/activity-log/ActivityLogRow.vue'
import ActivityLogUsage from '@/components/activity-log/ActivityLogUsage.vue'
import { useActivityLog } from '@/composables/useActivityLog'
import { errorMessage } from '@/utils/errorMessage'

/**
 * The activity log: every learning record the app wrote about the learner,
 * the log's size limit, a signed export and deletion.
 */
const { t } = useI18n()
const log = useActivityLog()
const { entries, usage, loading } = log

const selected = ref(new Set<string>())
interface DeleteRequest { kind: 'one' | 'some' | 'all', ids: string[] }
/** The delete being confirmed. Kept after the dialog closes so it does not change wording while it leaves. */
const request = ref<DeleteRequest>({ kind: 'one', ids: [] })
const confirming = ref(false)
const deleting = ref(false)
const exporting = ref(false)
const status = ref('')
const failure = ref('')

const hasMore = computed(() => usage.value !== null && entries.value.length < usage.value.records)

onMounted(async () => {
  try {
    await log.refresh()
  } catch (cause) {
    failure.value = t('dashboard.activityLog.loadFailed', { error: errorMessage(cause) })
  }
})

function toggle(id: string) {
  const next = new Set(selected.value)
  if (!next.delete(id)) next.add(id)
  selected.value = next
}

function ask(kind: DeleteRequest['kind'], ids: string[]) {
  request.value = { kind, ids }
  confirming.value = true
}

const confirmTitle = computed(() => {
  const { kind, ids } = request.value
  if (kind === 'one') return t('dashboard.activityLog.confirm.oneTitle')
  if (kind === 'all') return t('dashboard.activityLog.confirm.allTitle')
  return t('dashboard.activityLog.confirm.someTitle', { count: ids.length }, ids.length)
})

async function confirmDelete() {
  const { kind, ids } = request.value
  deleting.value = true
  failure.value = ''
  try {
    if (kind === 'all') await log.removeAll()
    else await log.remove(ids)
    const next = new Set(selected.value)
    for (const id of kind === 'all' ? [...next] : ids) next.delete(id)
    selected.value = next
  } catch (cause) {
    failure.value = t('dashboard.activityLog.deleteFailed', { error: errorMessage(cause) })
  } finally {
    deleting.value = false
    confirming.value = false
  }
}

/** Saves one file through the webview's download, bytes unchanged. */
function save(name: string, text: string) {
  const url = URL.createObjectURL(new Blob([text], { type: 'application/json' }))
  const anchor = document.createElement('a')
  anchor.href = url
  anchor.download = name
  document.body.appendChild(anchor)
  anchor.click()
  document.body.removeChild(anchor)
  URL.revokeObjectURL(url)
}

async function exportLog() {
  exporting.value = true
  status.value = ''
  failure.value = ''
  try {
    const exported = await log.exportLog()
    const name = `alexandria-activity-log-${new Date().toISOString().slice(0, 10)}.json`
    save(name, exported.statements)
    save(`${name}.sig`, exported.signature)
    const count = usage.value?.records ?? 0
    status.value = t('dashboard.activityLog.exported', { count }, count)
  } catch (cause) {
    failure.value = t('dashboard.activityLog.exportFailed', { error: errorMessage(cause) })
  } finally {
    exporting.value = false
  }
}

async function loadMore() {
  try {
    await log.loadMore()
  } catch (cause) {
    failure.value = t('dashboard.activityLog.loadFailed', { error: errorMessage(cause) })
  }
}
</script>

<template>
  <div class="mx-auto max-w-4xl space-y-6">
    <header>
      <h1 class="text-3xl font-bold text-foreground">{{ t('dashboard.activityLog.title') }}</h1>
      <p class="mt-2 text-muted-foreground">{{ t('dashboard.activityLog.subtitle') }}</p>
    </header>

    <ActivityLogUsage v-if="usage" :usage="usage" />

    <!-- Further panels about the log (send failures, for example) go here. -->

    <div class="flex flex-wrap items-center gap-2">
      <AppButton :loading="exporting" :disabled="usage === null" @click="exportLog">
        {{ t('dashboard.activityLog.actions.export') }}
      </AppButton>
      <AppButton
        variant="outline"
        :disabled="selected.size === 0"
        @click="ask('some', [...selected])"
      >
        {{ t('dashboard.activityLog.actions.deleteSelected', { count: selected.size }) }}
      </AppButton>
      <AppButton
        variant="danger"
        :disabled="!usage || usage.records === 0"
        @click="ask('all', [])"
      >
        {{ t('dashboard.activityLog.actions.deleteAll') }}
      </AppButton>
    </div>
    <p class="-mt-4 text-xs text-muted-foreground">{{ t('dashboard.activityLog.actions.exportHint') }}</p>

    <div role="status" aria-live="polite">
      <AppAlert v-if="status" variant="success">{{ status }}</AppAlert>
    </div>
    <AppAlert v-if="failure" variant="error">{{ failure }}</AppAlert>

    <EmptyState
      v-if="!loading && usage && entries.length === 0"
      :title="t('dashboard.activityLog.empty.title')"
      :description="t('dashboard.activityLog.empty.description')"
    />
    <ul
      v-else-if="entries.length > 0"
      class="card divide-y divide-border p-0"
      :aria-label="t('dashboard.activityLog.list.label')"
    >
      <ActivityLogRow
        v-for="entry in entries"
        :key="entry.id"
        :entry="entry"
        :selected="selected.has(entry.id)"
        @toggle="toggle(entry.id)"
        @delete="ask('one', [entry.id])"
      />
    </ul>
    <div v-if="hasMore" class="flex justify-center">
      <AppButton variant="ghost" @click="loadMore">{{ t('dashboard.activityLog.actions.showMore') }}</AppButton>
    </div>

    <ConfirmDialog
      :open="confirming"
      :title="confirmTitle"
      :message="request.kind === 'one' ? t('dashboard.activityLog.confirm.one') : t('dashboard.activityLog.confirm.many')"
      :confirm-label="request.kind === 'all' ? t('dashboard.activityLog.actions.deleteAll') : t('dashboard.activityLog.confirm.delete')"
      confirm-variant="danger"
      :loading="deleting"
      @confirm="confirmDelete"
      @cancel="confirming = false"
    />
  </div>
</template>
