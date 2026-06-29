<script setup lang="ts">
/**
 * Settings → Plugins.
 *
 * Single-stop management surface for community plugins. Lists installed
 * plugins with enable/disable toggle, install-from-disk picker, docs
 * viewer, donate button, capability grants, uninstall, and an inbox
 * tab for instructors reviewing IRL Review submissions.
 */

import { ref, computed, onMounted } from 'vue'
import { useRouter } from 'vue-router'
import { useLocalApi } from '@/composables/useLocalApi'
import { useDisplayNames } from '@/composables/useDisplayNames'
import {
  AppButton,
  AppInput,
  AppTextarea,
  AppBadge,
  AppSpinner,
  AppAlert,
  AppModal,
  AppTabs,
  EmptyState,
} from '@/components/ui'
import type {
  InstalledPlugin,
  IrlSubmission,
  PluginAttestationStatus,
  PluginCapability,
  PluginManifest,
  PluginPermissionRecord,
} from '@/types'

const { invoke } = useLocalApi()
const router = useRouter()
const { displayName, ensureNames } = useDisplayNames()

function openDocs(p: InstalledPlugin) {
  void router.push(`/settings/plugins/${p.plugin_cid}/docs`)
}

// ---- Tab state ------------------------------------------------------------
const activeTab = ref<'installed' | 'inbox' | 'my-submissions'>('installed')
const tabs = computed(() => [
  { key: 'installed', label: 'Installed', count: plugins.value.length },
  { key: 'inbox', label: 'Instructor Inbox', count: pendingInbox.value.length },
  { key: 'my-submissions', label: 'My Submissions', count: mySubmissions.value.length },
])

// ---- Installed plugins ----------------------------------------------------
const plugins = ref<InstalledPlugin[]>([])
const manifests = ref<Record<string, PluginManifest>>({})
const permissions = ref<Record<string, PluginPermissionRecord[]>>({})
const attestation = ref<Record<string, PluginAttestationStatus>>({})
const loading = ref(true)
const installPath = ref('')
const installing = ref(false)
const installError = ref<string | null>(null)
const installSuccess = ref<string | null>(null)

// Thumbnail data URLs keyed by plugin_cid (resolved from manifest.icon_path).
const thumbnails = ref<Record<string, string>>({})

// ---- Plugin search ----
const pluginQuery = ref('')
const filteredPlugins = computed(() => {
  const q = pluginQuery.value.trim().toLowerCase()
  if (!q) return plugins.value
  const terms = q.split(/\s+/)
  return plugins.value.filter((p) => {
    const m = manifests.value[p.plugin_cid]
    const hay = [
      p.name,
      p.version,
      p.source,
      m?.description ?? '',
      ...(m?.capabilities ?? []),
      ...(m?.subject_tags ?? []),
    ]
      .join(' ')
      .toLowerCase()
    return terms.every((t) => hay.includes(t))
  })
})

// Deterministic gradient + monogram fallback when a plugin ships no icon.
function thumbGradient(cid: string): string {
  let h = 0
  for (let i = 0; i < cid.length; i++) h = (h * 31 + cid.charCodeAt(i)) >>> 0
  const a = h % 360
  const b = (a + 60) % 360
  return `linear-gradient(135deg, hsl(${a} 70% 55%), hsl(${b} 70% 45%))`
}
function monogram(name: string): string {
  return (name.trim()[0] || '?').toUpperCase()
}

// Resolve a dependency plugin id (`did:key:…#slug`) to a friendly name: the
// installed plugin carrying that manifest id, else the slug.
function nameForPluginId(id: string): string {
  for (const p of plugins.value) {
    if (manifests.value[p.plugin_cid]?.id === id) return p.name
  }
  const hash = id.indexOf('#')
  return hash >= 0 ? id.slice(hash + 1) : id
}

// Plugins this one depends on (declared in its manifest), as names.
function dependencyNames(cid: string): string[] {
  return (manifests.value[cid]?.dependencies ?? []).map(nameForPluginId)
}

// Installed plugins that depend on this one (reverse edge), as names. These
// block uninstalling this plugin until they are removed first.
function requiredByNames(cid: string): string[] {
  const myId = manifests.value[cid]?.id
  if (!myId) return []
  return plugins.value
    .filter((p) => (manifests.value[p.plugin_cid]?.dependencies ?? []).includes(myId))
    .map((p) => p.name)
}

// Uninstall confirm
const uninstallTarget = ref<InstalledPlugin | null>(null)

onMounted(async () => {
  await Promise.all([refresh(), loadInbox(), loadMySubmissions()])
})

async function refresh() {
  loading.value = true
  try {
    plugins.value = await invoke<InstalledPlugin[]>('plugin_list')
    const detail = await Promise.all(
      plugins.value.map(async (p) => {
        const [m, perms, att] = await Promise.all([
          invoke<PluginManifest>('plugin_get_manifest', { pluginCid: p.plugin_cid }).catch(
            () => null,
          ),
          invoke<PluginPermissionRecord[]>('plugin_list_permissions', {
            pluginCid: p.plugin_cid,
          }).catch(() => [] as PluginPermissionRecord[]),
          invoke<PluginAttestationStatus>('plugin_attestation_status', {
            pluginCid: p.plugin_cid,
          }).catch(() => null),
        ])
        return { cid: p.plugin_cid, m, perms, att }
      }),
    )
    const mMap: Record<string, PluginManifest> = {}
    const pMap: Record<string, PluginPermissionRecord[]> = {}
    const aMap: Record<string, PluginAttestationStatus> = {}
    for (const d of detail) {
      if (d.m) mMap[d.cid] = d.m
      pMap[d.cid] = d.perms
      if (d.att) aMap[d.cid] = d.att
    }
    manifests.value = mMap
    permissions.value = pMap
    attestation.value = aMap
    void loadThumbnails()
  } catch (e) {
    installError.value = `Failed to load plugins: ${e}`
  } finally {
    loading.value = false
  }
}

async function loadThumbnails() {
  const next: Record<string, string> = {}
  await Promise.all(
    plugins.value.map(async (p) => {
      const icon = manifests.value[p.plugin_cid]?.icon_path
      if (!icon) return
      try {
        const url = await invoke<string>('plugin_read_asset_data_url', {
          pluginCid: p.plugin_cid,
          path: icon,
        })
        if (url) next[p.plugin_cid] = url
      } catch {
        // fall back to monogram
      }
    }),
  )
  thumbnails.value = next
}

async function install() {
  const path = installPath.value.trim()
  if (!path) {
    installError.value = 'Enter the path to a plugin bundle directory.'
    return
  }
  installing.value = true
  installError.value = null
  installSuccess.value = null
  try {
    const result = await invoke<InstalledPlugin>('plugin_install_from_file', {
      directory: path,
    })
    installSuccess.value = `Installed "${result.name}" v${result.version}.`
    installPath.value = ''
    await refresh()
  } catch (e) {
    installError.value = `Install failed: ${e}`
  } finally {
    installing.value = false
  }
}

async function toggleEnabled(p: InstalledPlugin) {
  const next = !p.enabled
  try {
    await invoke('plugin_set_enabled', { pluginCid: p.plugin_cid, enabled: next })
    p.enabled = next
  } catch (e) {
    installError.value = `Could not ${next ? 'enable' : 'disable'} plugin: ${e}`
  }
}

function confirmUninstall(p: InstalledPlugin) {
  uninstallTarget.value = p
}

async function doUninstall() {
  const p = uninstallTarget.value
  if (!p) return
  uninstallTarget.value = null
  try {
    await invoke('plugin_uninstall', { pluginCid: p.plugin_cid })
    await refresh()
  } catch (e) {
    installError.value = `Uninstall failed: ${e}`
  }
}

function donate(p: InstalledPlugin) {
  const url = manifests.value[p.plugin_cid]?.donate_url
  if (!url) return
  window.open(url, '_blank', 'noopener,noreferrer')
}

async function revoke(cid: string, capability: PluginCapability) {
  try {
    await invoke('plugin_revoke_capability', { pluginCid: cid, capability })
    permissions.value = {
      ...permissions.value,
      [cid]: await invoke<PluginPermissionRecord[]>('plugin_list_permissions', {
        pluginCid: cid,
      }),
    }
  } catch (e) {
    installError.value = `Revoke failed: ${e}`
  }
}

function attestationBadge(cid: string): { label: string; variant: 'success' | 'warning' | 'secondary' } {
  const s = attestation.value[cid]
  if (!s) return { label: 'Status pending', variant: 'secondary' }
  if (s.advisories.some((a) => a.kind === 'known_flawed')) {
    return { label: 'Known flawed', variant: 'warning' }
  }
  if (s.attested) return { label: 'DAO attested', variant: 'success' }
  return { label: 'Unattested', variant: 'secondary' }
}


// ---- IRL Review: instructor inbox ----------------------------------------
const pendingInbox = ref<IrlSubmission[]>([])
const reviewing = ref<IrlSubmission | null>(null)
const reviewScore = ref(0.8)
const reviewFeedback = ref('')
const reviewSkillRatings = ref<Record<string, number>>({})
const reviewSubmitting = ref(false)
const reviewError = ref<string | null>(null)

async function loadInbox() {
  try {
    pendingInbox.value = await invoke<IrlSubmission[]>('irl_list_pending', { pluginCid: null })
    void ensureNames(pendingInbox.value.map((s) => s.learner_did))
  } catch (e) {
    console.error('failed to load instructor inbox', e)
  }
}

function openReview(s: IrlSubmission) {
  reviewing.value = s
  reviewScore.value = 0.8
  reviewFeedback.value = ''
  reviewError.value = null
  const skills = parseSkills(s.skills_json)
  const map: Record<string, number> = {}
  for (const sk of skills) map[sk] = 0.8
  reviewSkillRatings.value = map
}

async function submitReview() {
  if (!reviewing.value) return
  reviewSubmitting.value = true
  reviewError.value = null
  try {
    await invoke('irl_post_review', {
      submissionId: reviewing.value.id,
      score: reviewScore.value,
      feedback: reviewFeedback.value,
      skillRatingsJson: JSON.stringify(reviewSkillRatings.value),
    })
    reviewing.value = null
    await Promise.all([loadInbox(), loadMySubmissions()])
  } catch (e) {
    reviewError.value = `Failed to post review: ${e}`
  } finally {
    reviewSubmitting.value = false
  }
}

function parseSkills(skillsJson: string): string[] {
  try {
    const v = JSON.parse(skillsJson)
    return Array.isArray(v) ? v.map(String) : []
  } catch {
    return []
  }
}

function parseSubmission(json: string): { comment?: string; files?: { name: string; mime: string; size: number; data_b64: string }[] } {
  try {
    return JSON.parse(json) as { comment?: string; files?: { name: string; mime: string; size: number; data_b64: string }[] }
  } catch {
    return {}
  }
}

function fileDataUrl(file: { mime: string; data_b64: string }): string {
  return `data:${file.mime};base64,${file.data_b64}`
}

function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`
  return `${(n / 1024 / 1024).toFixed(1)} MB`
}

// ---- My Submissions tab --------------------------------------------------
const mySubmissions = ref<IrlSubmission[]>([])

async function loadMySubmissions() {
  try {
    mySubmissions.value = await invoke<IrlSubmission[]>('irl_list_my_submissions', {
      pluginCid: null,
    })
    void ensureNames(mySubmissions.value.map((s) => s.reviewer_did).filter(Boolean) as string[])
  } catch (e) {
    console.error('failed to load my submissions', e)
  }
}
</script>


<template>
  <div>
    <p class="mb-6 text-sm text-muted-foreground">
      Install, enable, and manage community-authored learning elements.
      Plugins run in a sandboxed iframe with no network access; capabilities
      (microphone, camera, etc.) are granted per-plugin on first use.
    </p>

    <AppTabs
      :model-value="activeTab"
      :tabs="tabs"
      class="mb-6"
      @update:model-value="(v) => (activeTab = v as 'installed' | 'inbox' | 'my-submissions')"
    />

    <!-- ============================================================ -->
    <!-- Tab: Installed                                                -->
    <!-- ============================================================ -->
    <section v-if="activeTab === 'installed'">
      <div class="mb-6 rounded-2xl border border-border bg-card/50 p-4">
        <h2 class="text-sm font-semibold text-foreground">Install from local directory</h2>
        <p class="mt-1 text-xs text-muted-foreground">
          Point to a directory containing <code class="font-mono">manifest.json</code>,
          <code class="font-mono">manifest.sig</code>, and the entry HTML the manifest references.
        </p>
        <div class="mt-3 flex gap-2">
          <AppInput
            v-model="installPath"
            placeholder="/absolute/path/to/plugin-bundle"
            class="flex-1"
            :disabled="installing"
          />
          <AppButton :loading="installing" @click="install">Install</AppButton>
        </div>
        <AppAlert v-if="installError" variant="error" class="mt-3">{{ installError }}</AppAlert>
        <AppAlert v-if="installSuccess" variant="success" class="mt-3">{{ installSuccess }}</AppAlert>
      </div>

      <div v-if="loading" class="flex justify-center p-10">
        <AppSpinner />
      </div>

      <EmptyState
        v-else-if="plugins.length === 0"
        title="No plugins installed"
        description="Install a plugin above to add new learning and assessment element types."
      />

      <template v-else>
        <!-- Search -->
        <div class="relative mb-4">
          <svg class="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-muted-foreground" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
            <path stroke-linecap="round" stroke-linejoin="round" d="M21 21l-4.35-4.35M17 11a6 6 0 11-12 0 6 6 0 0112 0z" />
          </svg>
          <input
            v-model="pluginQuery"
            type="text"
            placeholder="Search plugins by name, capability, tag…"
            class="w-full rounded-lg border border-border bg-background py-2 pl-9 pr-8 text-sm text-foreground outline-none focus:border-primary"
          >
          <button
            v-if="pluginQuery"
            class="absolute right-2.5 top-1/2 -translate-y-1/2 text-muted-foreground hover:text-foreground"
            aria-label="Clear search"
            @click="pluginQuery = ''"
          >
            <svg class="h-3.5 w-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
              <path stroke-linecap="round" stroke-linejoin="round" d="M6 18L18 6M6 6l12 12" />
            </svg>
          </button>
        </div>

        <p v-if="filteredPlugins.length === 0" class="py-8 text-center text-sm text-muted-foreground">
          No plugins match "{{ pluginQuery }}".
        </p>

        <div class="plugin-grid">
        <div
          v-for="p in filteredPlugins"
          :key="p.plugin_cid"
          role="button"
          tabindex="0"
          class="group relative flex cursor-pointer flex-col rounded-2xl border border-border bg-card/40 p-4 text-left transition-all hover:border-primary/50 hover:bg-card/70 hover:shadow-md focus:outline-none focus-visible:ring-2 focus-visible:ring-primary"
          :class="{ 'opacity-60': !p.enabled }"
          @click="openDocs(p)"
          @keydown.enter="openDocs(p)"
          @keydown.space.prevent="openDocs(p)"
        >
          <!-- Header: thumbnail + name -->
          <div class="flex items-start gap-3">
            <div
              class="plugin-thumb flex h-14 w-14 shrink-0 items-center justify-center overflow-hidden rounded-xl text-xl font-bold text-white shadow-sm"
              :style="thumbnails[p.plugin_cid] ? undefined : { background: thumbGradient(p.plugin_cid) }"
            >
              <img
                v-if="thumbnails[p.plugin_cid]"
                :src="thumbnails[p.plugin_cid]"
                :alt="p.name"
                class="h-full w-full object-cover"
              />
              <span v-else>{{ monogram(p.name) }}</span>
            </div>
            <div class="min-w-0 flex-1">
              <h3 class="truncate text-base font-semibold text-foreground">{{ p.name }}</h3>
              <div class="mt-1 flex flex-wrap items-center gap-1.5">
                <AppBadge variant="secondary">v{{ p.version }}</AppBadge>
                <AppBadge variant="secondary">{{ p.source }}</AppBadge>
                <AppBadge v-if="!p.enabled" variant="warning">Disabled</AppBadge>
              </div>
            </div>
          </div>

          <!-- Description -->
          <p
            v-if="manifests[p.plugin_cid]?.description"
            class="mt-3 line-clamp-3 flex-1 text-sm text-muted-foreground"
          >
            {{ manifests[p.plugin_cid]?.description }}
          </p>
          <div v-else class="flex-1" />

          <!-- Attestation + capability chips -->
          <div class="mt-3 flex flex-wrap items-center gap-1.5">
            <AppBadge :variant="attestationBadge(p.plugin_cid).variant">
              {{ attestationBadge(p.plugin_cid).label }}
            </AppBadge>
            <button
              v-for="perm in permissions[p.plugin_cid] ?? []"
              :key="perm.capability"
              type="button"
              class="group/cap inline-flex items-center gap-1 rounded-md bg-muted/30 px-1.5 py-0.5 text-[10px] text-muted-foreground transition-colors hover:bg-destructive/15 hover:text-destructive"
              :title="`${perm.capability} (${perm.scope}) — click to revoke`"
              @click.stop="revoke(p.plugin_cid, perm.capability)"
            >
              {{ perm.capability }}
              <svg class="h-2.5 w-2.5 opacity-0 transition-opacity group-hover/cap:opacity-100" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2.5">
                <path stroke-linecap="round" stroke-linejoin="round" d="M6 18L18 6M6 6l12 12" />
              </svg>
            </button>
          </div>

          <!-- Dependency relationships (auto-installed together) -->
          <div
            v-if="dependencyNames(p.plugin_cid).length || requiredByNames(p.plugin_cid).length"
            class="mt-2 flex flex-col gap-1 text-[10px] text-muted-foreground"
          >
            <div v-if="dependencyNames(p.plugin_cid).length" class="flex flex-wrap items-center gap-1">
              <span class="opacity-70">Requires</span>
              <span
                v-for="dep in dependencyNames(p.plugin_cid)"
                :key="`req-${p.plugin_cid}-${dep}`"
                class="rounded-md bg-muted/30 px-1.5 py-0.5"
              >{{ dep }}</span>
            </div>
            <div v-if="requiredByNames(p.plugin_cid).length" class="flex flex-wrap items-center gap-1">
              <span class="opacity-70">Required by</span>
              <span
                v-for="dep in requiredByNames(p.plugin_cid)"
                :key="`reqby-${p.plugin_cid}-${dep}`"
                class="rounded-md bg-muted/30 px-1.5 py-0.5"
              >{{ dep }}</span>
            </div>
          </div>

          <!-- Actions — stop propagation so they don't open docs -->
          <div class="mt-4 flex items-center justify-between gap-2 border-t border-border/50 pt-3" @click.stop>
            <label class="flex items-center gap-1.5 text-xs text-muted-foreground" @click.stop>
              <input
                type="checkbox"
                :checked="p.enabled"
                class="h-4 w-4 rounded border-border accent-primary"
                @change="toggleEnabled(p)"
              />
              Enabled
            </label>
            <div class="flex items-center gap-1">
              <AppButton
                v-if="manifests[p.plugin_cid]?.donate_url"
                size="sm"
                variant="ghost"
                @click.stop="donate(p)"
              >
                Donate
              </AppButton>
              <AppButton
                v-if="p.source !== 'builtin'"
                size="sm"
                variant="danger"
                @click.stop="confirmUninstall(p)"
              >
                Uninstall
              </AppButton>
            </div>
          </div>

          <!-- Open-docs affordance -->
          <span class="pointer-events-none absolute right-3 top-3 text-muted-foreground opacity-0 transition-opacity group-hover:opacity-100">
            <svg class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
              <path stroke-linecap="round" stroke-linejoin="round" d="M9 5l7 7-7 7" />
            </svg>
          </span>
        </div>
        </div>
      </template>
    </section>

    <!-- ============================================================ -->
    <!-- Tab: Instructor Inbox (IRL Review)                            -->
    <!-- ============================================================ -->
    <section v-if="activeTab === 'inbox'">
      <p class="mb-4 text-sm text-muted-foreground">
        Pending submissions from learners using the IRL Review plugin on this device.
      </p>

      <EmptyState
        v-if="pendingInbox.length === 0"
        title="No pending reviews"
        description="When a learner submits work to the IRL Review plugin, it will appear here."
      />

      <ul v-else class="space-y-3">
        <li
          v-for="s in pendingInbox"
          :key="s.id"
          class="rounded-xl border border-border bg-card/40 p-4"
        >
          <div class="flex flex-wrap items-start justify-between gap-3">
            <div class="min-w-0 flex-1">
              <div class="flex items-center gap-2 text-sm">
                <AppBadge variant="warning">pending</AppBadge>
                <span class="text-muted-foreground">
                  from <span class="font-medium text-foreground">{{ displayName(s.learner_did) }}</span>
                </span>
              </div>
              <p class="mt-1 text-[11px] text-muted-foreground">
                Submitted {{ new Date(s.created_at).toLocaleString() }}
              </p>
              <p
                v-if="parseSubmission(s.submission_json).comment"
                class="mt-2 text-sm text-foreground"
              >
                “{{ parseSubmission(s.submission_json).comment }}”
              </p>
              <div
                v-if="parseSkills(s.skills_json).length > 0"
                class="mt-2 flex flex-wrap gap-1 text-[11px]"
              >
                <span
                  v-for="sk in parseSkills(s.skills_json)"
                  :key="sk"
                  class="rounded-full bg-primary/10 px-2 py-0.5 text-primary"
                >{{ sk }}</span>
              </div>
              <p
                v-if="(parseSubmission(s.submission_json).files?.length ?? 0) > 0"
                class="mt-2 text-[11px] text-muted-foreground"
              >
                {{ parseSubmission(s.submission_json).files?.length }} file(s) attached
              </p>
            </div>
            <AppButton size="sm" @click="openReview(s)">Open</AppButton>
          </div>
        </li>
      </ul>
    </section>

    <!-- ============================================================ -->
    <!-- Tab: My Submissions                                           -->
    <!-- ============================================================ -->
    <section v-if="activeTab === 'my-submissions'">
      <p class="mb-4 text-sm text-muted-foreground">
        Submissions you have sent to instructors through the IRL Review plugin.
      </p>

      <EmptyState
        v-if="mySubmissions.length === 0"
        title="No submissions yet"
        description="Use the IRL Review plugin inside a course to submit work for review."
      />

      <ul v-else class="space-y-3">
        <li
          v-for="s in mySubmissions"
          :key="s.id"
          class="rounded-xl border border-border bg-card/40 p-4"
        >
          <div class="flex items-center gap-2 text-sm">
            <AppBadge
              :variant="s.status === 'reviewed' ? 'success' : s.status === 'rejected' ? 'warning' : 'secondary'"
            >
              {{ s.status }}
            </AppBadge>
            <span v-if="s.status === 'reviewed' && s.score !== null" class="font-medium">
              {{ Math.round((s.score ?? 0) * 100) }}%
            </span>
            <span class="text-muted-foreground text-xs">
              · {{ new Date(s.created_at).toLocaleString() }}
            </span>
          </div>
          <p v-if="s.feedback" class="mt-2 text-sm text-foreground">{{ s.feedback }}</p>
          <div
            v-if="s.skill_ratings_json"
            class="mt-2 grid grid-cols-2 gap-x-4 gap-y-1 text-xs"
          >
            <template
              v-for="(rating, skill) in (() => { try { return JSON.parse(s.skill_ratings_json ?? '{}') as Record<string, number> } catch { return {} } })()"
              :key="skill"
            >
              <span class="text-muted-foreground">{{ skill }}</span>
              <span class="text-foreground text-right">{{ Math.round((rating || 0) * 100) }}%</span>
            </template>
          </div>
        </li>
      </ul>
    </section>

    <!-- ============================================================ -->
    <!-- Modals                                                        -->
    <!-- ============================================================ -->

    <AppModal
      v-if="uninstallTarget"
      :open="!!uninstallTarget"
      title="Uninstall plugin?"
      @close="uninstallTarget = null"
    >
      <p class="text-sm">
        Remove <strong>{{ uninstallTarget.name }}</strong> v{{ uninstallTarget.version }}?
        Courses that use it will stop working until you re-install.
      </p>
      <template #footer>
        <AppButton variant="ghost" @click="uninstallTarget = null">Cancel</AppButton>
        <AppButton variant="danger" @click="doUninstall">Uninstall</AppButton>
      </template>
    </AppModal>

    <AppModal
      v-if="reviewing"
      :open="!!reviewing"
      title="Review submission"
      @close="reviewing = null"
    >
      <div class="space-y-4">
        <div>
          <p class="text-xs text-muted-foreground">
            From <span class="font-medium text-foreground">{{ displayName(reviewing.learner_did) }}</span>
            on {{ new Date(reviewing.created_at).toLocaleString() }}
          </p>
          <p
            v-if="parseSubmission(reviewing.submission_json).comment"
            class="mt-2 text-sm"
          >
            “{{ parseSubmission(reviewing.submission_json).comment }}”
          </p>
        </div>

        <div v-if="(parseSubmission(reviewing.submission_json).files?.length ?? 0) > 0">
          <h4 class="text-xs font-semibold uppercase tracking-wide text-muted-foreground">
            Attached files
          </h4>
          <ul class="mt-2 space-y-2">
            <li
              v-for="(file, i) in parseSubmission(reviewing.submission_json).files ?? []"
              :key="i"
              class="rounded-md border border-border/40 bg-background/30 p-2"
            >
              <div class="flex items-center justify-between text-xs">
                <span class="font-medium">{{ file.name }}</span>
                <span class="text-muted-foreground">{{ formatBytes(file.size) }} · {{ file.mime }}</span>
              </div>
              <a
                v-if="file.mime.startsWith('image/')"
                :href="fileDataUrl(file)"
                target="_blank"
                rel="noopener"
              >
                <img
                  :src="fileDataUrl(file)"
                  :alt="file.name"
                  class="mt-2 max-h-48 rounded-md object-contain"
                />
              </a>
              <video
                v-else-if="file.mime.startsWith('video/')"
                controls
                :src="fileDataUrl(file)"
                class="mt-2 w-full max-h-48 rounded-md"
              />
              <audio
                v-else-if="file.mime.startsWith('audio/')"
                controls
                :src="fileDataUrl(file)"
                class="mt-2 w-full"
              />
              <a
                v-else
                :href="fileDataUrl(file)"
                :download="file.name"
                class="mt-2 inline-block text-xs text-primary hover:underline"
              >
                Download
              </a>
            </li>
          </ul>
        </div>

        <div>
          <label class="text-xs font-semibold uppercase tracking-wide text-muted-foreground">
            Overall score
          </label>
          <div class="mt-1 flex items-center gap-3">
            <input
              v-model.number="reviewScore"
              type="range"
              min="0"
              max="1"
              step="0.01"
              class="flex-1"
            />
            <span class="w-12 text-right font-medium">{{ Math.round(reviewScore * 100) }}%</span>
          </div>
        </div>

        <div>
          <label class="text-xs font-semibold uppercase tracking-wide text-muted-foreground">
            Per-skill ratings
          </label>
          <div v-if="parseSkills(reviewing.skills_json).length === 0" class="mt-2 text-xs text-muted-foreground">
            The learner did not declare any skills.
          </div>
          <div v-else class="mt-2 space-y-2">
            <div
              v-for="skill in parseSkills(reviewing.skills_json)"
              :key="skill"
              class="flex items-center gap-3"
            >
              <span class="w-32 text-sm">{{ skill }}</span>
              <input
                v-model.number="reviewSkillRatings[skill]"
                type="range"
                min="0"
                max="1"
                step="0.01"
                class="flex-1"
              />
              <span class="w-12 text-right text-sm">
                {{ Math.round((reviewSkillRatings[skill] ?? 0) * 100) }}%
              </span>
            </div>
          </div>
        </div>

        <div>
          <label class="text-xs font-semibold uppercase tracking-wide text-muted-foreground">
            Feedback
          </label>
          <AppTextarea
            v-model="reviewFeedback"
            class="mt-1"
            :rows="4"
            placeholder="Tell the learner what worked and what to improve…"
          />
        </div>

        <AppAlert v-if="reviewError" variant="error">{{ reviewError }}</AppAlert>
      </div>

      <template #footer>
        <AppButton variant="ghost" @click="reviewing = null">Cancel</AppButton>
        <AppButton :loading="reviewSubmitting" @click="submitReview">Post review</AppButton>
      </template>
    </AppModal>
  </div>
</template>

<style scoped>
/* Responsive plugin card grid — auto-fills columns by available width. */
.plugin-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(280px, 1fr));
  gap: 0.85rem;
}

/* Clamp the card description to 3 lines. */
.line-clamp-3 {
  display: -webkit-box;
  -webkit-line-clamp: 3;
  -webkit-box-orient: vertical;
  overflow: hidden;
}
</style>
