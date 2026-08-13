<script setup lang="ts">
/**
 * What directories are holding about you, and answering when one asks.
 *
 * The counterpart to talent-index consent, and deliberately next to it: that
 * screen decides what may leave this device, and this one shows what came of
 * it. Split across two places, a learner could agree to be findable and never
 * see who then went looking.
 *
 * Three rules drive the design.
 *
 * 1. **Nothing is contacted until the learner adds it.** The list starts empty
 *    and is never populated by discovery, an update, or a default. An
 *    offline-first application does not decide on its user's behalf to start
 *    talking to a server.
 * 2. **A directory that cannot be answered is shown, not skipped.** Silence
 *    from a server that is down looks exactly like "nobody asked about you",
 *    and those are opposite facts.
 * 3. **There is no decline button.** A request that is not answered expires.
 *    A button that recorded a refusal would turn a consent mechanism into a
 *    record of who said no to whom, held by the party that was refused.
 */

import { onMounted, ref } from 'vue'
import { useRouter } from 'vue-router'
import { invoke } from '@tauri-apps/api/core'
import { useI18n } from 'vue-i18n'

import { AppButton } from '@/components/ui'

interface Directory {
  name: string
  url: string
}

interface Problem {
  directory: string
  detail: string
}

interface DisclosureRequest {
  directory: string
  id: string
  from: string
  skillIds: string[]
  purpose: string
  expiresAt: string
}

interface AccessEntry {
  directory: string
  institution: string
  role: string
  moduleId: string | null
  granularity: string
  at: string
}

interface Grant {
  directory: string
  organisationId: string
  institution: string
  visibleToAdministrators: boolean
}

interface PullResult<T> {
  items: T[]
  problems: Problem[]
}

const { t, locale } = useI18n()
const router = useRouter()

const directories = ref<Directory[]>([])
const requests = ref<DisclosureRequest[]>([])
const accesses = ref<AccessEntry[]>([])
const grants = ref<Grant[]>([])
const problems = ref<Problem[]>([])

const newName = ref('')
const newUrl = ref('')
const checking = ref(false)
const sharing = ref('')
/**
 * Keyed by request, because a failure belongs beside the button that caused it.
 *
 * This used to set the shared `error` at the top of the section, which on a
 * scrolled page is off-screen — so the commonest failure, having consented to
 * publish nothing yet, looked exactly like a button that does nothing.
 */
const shareError = ref<Record<string, string>>({})
const publishing = ref('')
const publishedTo = ref('')
const publishError = ref<Record<string, string>>({})
const changingGrant = ref('')
const sharedIds = ref<Set<string>>(new Set())
const error = ref('')

async function loadDirectories() {
  directories.value = await invoke<Directory[]>('list_directories')
}

/**
 * Ask every directory at once.
 *
 * Problems from both calls are merged, deduplicated by directory: a server
 * that is down fails both, and telling the learner twice about one outage
 * makes it look like two.
 */
async function check() {
  if (!directories.value.length) return
  checking.value = true
  error.value = ''
  try {
    const [pending, log, visibility] = await Promise.all([
      invoke<PullResult<DisclosureRequest>>('fetch_disclosure_requests'),
      invoke<PullResult<AccessEntry>>('fetch_access_log'),
      invoke<PullResult<Grant>>('fetch_visibility'),
    ])
    requests.value = pending.items
    accesses.value = log.items
    grants.value = visibility.items

    const seen = new Map<string, Problem>()
    for (const p of [...pending.problems, ...log.problems, ...visibility.problems]) {
      if (!seen.has(p.directory)) seen.set(p.directory, p)
    }
    problems.value = [...seen.values()]
  } catch (e) {
    error.value = String(e)
  } finally {
    checking.value = false
  }
}

async function add() {
  error.value = ''
  try {
    const next = [...directories.value, { name: newName.value.trim(), url: newUrl.value.trim() }]
    await invoke('set_directories', { directories: next })
    directories.value = next
    newName.value = ''
    newUrl.value = ''
    await check()
  } catch (e) {
    error.value = String(e)
  }
}

async function remove(url: string) {
  error.value = ''
  try {
    const next = directories.value.filter((x) => x.url !== url)
    await invoke('set_directories', { directories: next })
    directories.value = next
    // Dropped locally too. Leaving rows from a directory the learner just
    // removed on screen would suggest it is still being asked.
    requests.value = requests.value.filter((r) => next.some((n) => n.name === r.directory))
    accesses.value = accesses.value.filter((a) => next.some((n) => n.name === a.directory))
    grants.value = grants.value.filter((g) => next.some((n) => n.name === g.directory))
    problems.value = problems.value.filter((p) => next.some((n) => n.name === p.directory))
  } catch (e) {
    error.value = String(e)
  }
}

async function share(req: DisclosureRequest) {
  const dir = directories.value.find((x) => x.name === req.directory)
  if (!dir) {
    shareError.value = { ...shareError.value, [req.id]: t('profile.directories.noDirectory') }
    return
  }
  sharing.value = req.id
  shareError.value = { ...shareError.value, [req.id]: '' }
  try {
    await invoke('share_disclosure', { directoryUrl: dir.url, requestId: req.id })
    sharedIds.value = new Set([...sharedIds.value, req.id])
  } catch (e) {
    shareError.value = { ...shareError.value, [req.id]: String(e) }
  } finally {
    sharing.value = ''
  }
}

async function publish(dir: Directory) {
  publishing.value = dir.url
  publishError.value = { ...publishError.value, [dir.url]: '' }
  try {
    await invoke('publish_listing', { directoryUrl: dir.url })
    publishedTo.value = dir.name
  } catch (e) {
    publishError.value = { ...publishError.value, [dir.url]: String(e) }
  } finally {
    publishing.value = ''
  }
}

/**
 * Grant or withdraw one institution's administrators named visibility.
 *
 * Re-read afterwards rather than flipped locally. This is a permission held on
 * a server; showing the toggle in a state the server has not confirmed would
 * tell somebody they had withdrawn consent when the call had failed.
 */
async function toggleGrant(g: Grant) {
  const dir = directories.value.find((x) => x.name === g.directory)
  if (!dir) return
  changingGrant.value = g.organisationId
  error.value = ''
  try {
    await invoke('set_visibility', {
      directoryUrl: dir.url,
      organisationId: g.organisationId,
      visible: !g.visibleToAdministrators,
    })
    grants.value = (await invoke<PullResult<Grant>>('fetch_visibility')).items
  } catch (e) {
    error.value = String(e)
  } finally {
    changingGrant.value = ''
  }
}

/**
 * A timestamp in the reader's own locale.
 *
 * `Intl` rather than a vue-i18n datetime format, because this catalogue
 * declares none and inventing one here would put a formatting convention in a
 * component instead of the place formats live. An unparseable value is shown
 * as it arrived — a server sending something odd is worth seeing, not hiding
 * behind "Invalid Date".
 */
function when(iso: string) {
  const parsed = new Date(iso)
  if (Number.isNaN(parsed.getTime())) return iso
  return new Intl.DateTimeFormat(locale.value, {
    dateStyle: 'medium',
    timeStyle: 'short',
  }).format(parsed)
}

onMounted(async () => {
  await loadDirectories()
  await check()
})
</script>

<template>
  <section class="space-y-5 rounded-xl border border-border bg-card p-5">
    <header>
      <h2 class="text-base font-semibold text-foreground">{{ t('profile.directories.title') }}</h2>
      <p class="mt-1 text-sm text-muted-foreground">
        {{ t('profile.directories.description') }}
      </p>
    </header>

    <p v-if="!directories.length" class="text-sm text-muted-foreground">
      {{ t('profile.directories.none') }}
    </p>

    <ul v-else class="space-y-2">
      <li
        v-for="dir in directories"
        :key="dir.url"
        class="flex items-center gap-3 rounded-lg border border-border p-3"
      >
        <span class="min-w-0 flex-1">
          <span class="block truncate text-sm text-foreground">{{ dir.name }}</span>
          <span class="block truncate font-mono text-xs text-muted-foreground">{{ dir.url }}</span>
        </span>
        <AppButton variant="ghost" size="sm" @click="remove(dir.url)">
          {{ t('profile.directories.remove') }}
        </AppButton>
      </li>
    </ul>

    <div class="space-y-2">
      <h3 class="text-sm font-medium text-foreground">
        {{ t('profile.directories.addHeading') }}
      </h3>
      <div class="flex flex-wrap items-end gap-2">
        <label class="min-w-40 flex-1 text-xs text-muted-foreground">
          {{ t('profile.directories.nameLabel') }}
          <input
            v-model="newName"
            type="text"
            class="mt-1 w-full rounded-lg border border-border bg-background p-2 text-sm text-foreground"
            :placeholder="t('profile.directories.namePlaceholder')"
          />
        </label>
        <label class="min-w-56 flex-[2] text-xs text-muted-foreground">
          {{ t('profile.directories.urlLabel') }}
          <input
            v-model="newUrl"
            type="url"
            spellcheck="false"
            class="mt-1 w-full rounded-lg border border-border bg-background p-2 font-mono text-sm text-foreground"
            :placeholder="t('profile.directories.urlPlaceholder')"
          />
        </label>
        <AppButton :disabled="!newName.trim() || !newUrl.trim()" @click="add">
          {{ t('profile.directories.add') }}
        </AppButton>
        <AppButton variant="ghost" :disabled="checking || !directories.length" @click="check">
          {{ checking ? t('profile.directories.checking') : t('profile.directories.refresh') }}
        </AppButton>
      </div>
    </div>

    <p v-if="error" class="text-sm text-destructive">{{ error }}</p>

    <template v-if="directories.length">
      <div class="space-y-2">
        <h3 class="text-sm font-medium text-foreground">
          {{ t('profile.directories.requestsHeading') }}
        </h3>

        <p v-if="!requests.length" class="text-sm text-muted-foreground">
          {{ t('profile.directories.noRequests') }}
        </p>

        <div
          v-for="r in requests"
          :key="r.id"
          class="space-y-2 rounded-lg border border-border p-3"
        >
          <p class="text-sm text-foreground">
            {{ t('profile.directories.requestFrom', { org: r.from }) }}
          </p>
          <p class="text-xs text-muted-foreground">
            {{ t('profile.directories.requestPurpose', { purpose: r.purpose }) }}
          </p>
          <p class="text-xs text-muted-foreground">
            {{ t('profile.directories.requestSkills', { skills: r.skillIds.join(', ') }) }}
          </p>
          <p class="text-xs text-muted-foreground">
            {{ t('profile.directories.requestExpires', { when: when(r.expiresAt) }) }}
          </p>
          <AppButton
            v-if="!sharedIds.has(r.id)"
            size="sm"
            :disabled="sharing === r.id"
            @click="share(r)"
          >
            {{
              sharing === r.id ? t('profile.directories.sharing') : t('profile.directories.share')
            }}
          </AppButton>
          <p v-else class="text-xs text-foreground">{{ t('profile.directories.shared') }}</p>

          <p v-if="shareError[r.id]" class="text-xs text-destructive">
            {{ shareError[r.id] }}
            <button
              type="button"
              class="text-primary underline underline-offset-2"
              @click="router.push('/profile')"
            >
              {{ t('profile.directories.consentLink') }}
            </button>
          </p>
        </div>

        <p class="text-xs text-muted-foreground">
          {{ t('profile.directories.ignoreNote') }}
        </p>
      </div>

      <div class="space-y-2">
        <h3 class="text-sm font-medium text-foreground">
          {{ t('profile.directories.publishHeading') }}
        </h3>
        <p class="text-xs text-muted-foreground">{{ t('profile.directories.publishNote') }}</p>
        <p class="text-xs text-muted-foreground">
          {{ t('profile.directories.consentHint') }}
          <button
            type="button"
            class="text-primary underline underline-offset-2"
            @click="router.push('/profile')"
          >
            {{ t('profile.directories.consentLink') }}
          </button>
        </p>
        <div class="flex flex-wrap gap-2">
          <AppButton
            v-for="dir in directories"
            :key="dir.url"
            variant="outline"
            size="sm"
            :disabled="publishing === dir.url"
            @click="publish(dir)"
          >
            {{
              publishing === dir.url
                ? t('profile.directories.publishing')
                : t('profile.directories.publish', { directory: dir.name })
            }}
          </AppButton>
        </div>
        <p v-if="publishedTo" class="text-xs text-foreground">
          {{ t('profile.directories.published', { directory: publishedTo }) }}
        </p>

        <p
          v-for="dir in directories"
          v-show="publishError[dir.url]"
          :key="`err-${dir.url}`"
          class="text-xs text-destructive"
        >
          {{ publishError[dir.url] }}
          <button
            type="button"
            class="text-primary underline underline-offset-2"
            @click="router.push('/profile')"
          >
            {{ t('profile.directories.consentLink') }}
          </button>
        </p>
      </div>

      <div class="space-y-2">
        <h3 class="text-sm font-medium text-foreground">
          {{ t('profile.directories.visibilityHeading') }}
        </h3>

        <p v-if="!grants.length" class="text-sm text-muted-foreground">
          {{ t('profile.directories.noGrants') }}
        </p>

        <div
          v-for="g in grants"
          :key="g.organisationId"
          class="flex items-center gap-3 rounded-lg border border-border p-3"
        >
          <span class="min-w-0 flex-1 text-sm text-foreground">
            {{
              g.visibleToAdministrators
                ? t('profile.directories.grantOn', { institution: g.institution })
                : t('profile.directories.grantOff', { institution: g.institution })
            }}
          </span>
          <AppButton
            variant="ghost"
            size="sm"
            :disabled="changingGrant === g.organisationId"
            @click="toggleGrant(g)"
          >
            {{
              g.visibleToAdministrators
                ? t('profile.directories.revoke')
                : t('profile.directories.grant')
            }}
          </AppButton>
        </div>

        <p class="text-xs text-muted-foreground">{{ t('profile.directories.visibilityNote') }}</p>
      </div>

      <div class="space-y-2">
        <h3 class="text-sm font-medium text-foreground">
          {{ t('profile.directories.accessHeading') }}
        </h3>

        <p v-if="!accesses.length" class="text-sm text-muted-foreground">
          {{ t('profile.directories.noAccess') }}
        </p>

        <ul v-else class="space-y-1">
          <li v-for="(a, i) in accesses" :key="i" class="text-sm text-foreground">
            <template v-if="a.granularity === 'named'">
              {{ t('profile.directories.accessNamed', { who: a.role, where: a.institution }) }}
            </template>
            <template v-else>
              {{ t('profile.directories.accessAggregate', { where: a.institution }) }}
            </template>
            <span v-if="a.moduleId" class="text-muted-foreground">
              {{ t('profile.directories.accessModule', { module: a.moduleId }) }}
            </span>
            <span class="text-xs text-muted-foreground"> · {{ when(a.at) }}</span>
          </li>
        </ul>
      </div>

      <div v-if="problems.length" class="space-y-1">
        <h3 class="text-sm font-medium text-foreground">
          {{ t('profile.directories.problemsHeading') }}
        </h3>
        <p v-for="p in problems" :key="p.directory" class="text-sm text-muted-foreground">
          {{ t('profile.directories.problem', { directory: p.directory, detail: p.detail }) }}
        </p>
        <p class="text-xs text-muted-foreground">{{ t('profile.directories.problemNote') }}</p>
      </div>
    </template>
  </section>
</template>
