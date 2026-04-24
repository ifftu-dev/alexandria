<script setup lang="ts">
import { ref, onMounted, computed } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { invoke } from '@tauri-apps/api/core'
import {
  AppButton,
  AppBadge,
  AppAlert,
  AppSpinner,
  ProvenanceBadge,
} from '@/components/ui'
import VideoPlayer from '@/components/course/VideoPlayer.vue'
import type {
  OpinionRow,
  SubjectFieldInfo,
  VerifiableCredential,
} from '@/types'

const route = useRoute()
const router = useRouter()

const opinion = ref<OpinionRow | null>(null)
const subjectField = ref<SubjectFieldInfo | null>(null)
const loading = ref(true)
const error = ref('')

// Local identity (to know if we're looking at our own post)
const selfStakeAddress = ref<string>('')

const isOwner = computed(
  () => opinion.value !== null && opinion.value.author_address === selfStakeAddress.value,
)

// Credentials the author staked on this opinion, fetched by id.
// Unknown / unsynced credentials simply render as raw id badges.
const linkedCredentials = ref<VerifiableCredential[]>([])

const bloomOrder = ['remember', 'understand', 'apply', 'analyze', 'evaluate', 'create']

function skillClaim(vc: VerifiableCredential) {
  const claim = vc.credential_subject.claim
  if (claim.kind !== 'skill') return null
  return claim
}

async function loadOpinion() {
  loading.value = true
  error.value = ''
  try {
    const id = route.params.id as string
    const row = await invoke<OpinionRow | null>('get_opinion', { opinionId: id })
    if (!row) {
      error.value = 'Opinion not found.'
      return
    }
    opinion.value = row

    const [fields, identity] = await Promise.all([
      invoke<SubjectFieldInfo[]>('list_subject_fields', {}).catch(() => []),
      invoke<{ stake_address: string } | null>('get_profile').catch(() => null),
    ])
    subjectField.value = fields.find((f) => f.id === row.subject_field_id) ?? null
    selfStakeAddress.value = identity?.stake_address ?? ''

    // Resolve each referenced credential locally — any that haven't
    // synced to this peer just show as raw ids below.
    const fetched = await Promise.all(
      row.credential_proof_ids.map((cid) =>
        invoke<VerifiableCredential | null>('get_credential', { credentialId: cid }).catch(
          () => null,
        ),
      ),
    )
    linkedCredentials.value = fetched.filter((vc): vc is VerifiableCredential => vc != null)
  } catch (e) {
    error.value = String(e)
  } finally {
    loading.value = false
  }
}

async function withdraw() {
  if (!opinion.value) return
  if (!confirm('Withdraw this opinion? The video will be unpinned locally and the post hidden.')) {
    return
  }
  try {
    await invoke('withdraw_own_opinion', { opinionId: opinion.value.id })
    router.push('/opinions')
  } catch (e) {
    error.value = `Withdraw failed: ${e}`
  }
}

function formatDate(iso: string | null | undefined): string {
  if (!iso) return ''
  try {
    return new Date(iso).toLocaleDateString(undefined, {
      year: 'numeric',
      month: 'short',
      day: 'numeric',
    })
  } catch {
    return iso
  }
}

function unresolvedIds(): string[] {
  if (!opinion.value) return []
  const knownIds = new Set(linkedCredentials.value.map((vc) => vc.id))
  return opinion.value.credential_proof_ids.filter((id) => !knownIds.has(id))
}

onMounted(async () => {
  await loadOpinion()
})
</script>

<template>
  <div class="max-w-4xl">
    <button
      type="button"
      class="mb-4 flex items-center gap-1 text-sm text-muted-foreground hover:text-foreground"
      @click="$router.back()"
    >
      <svg class="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
        <path stroke-linecap="round" stroke-linejoin="round" d="M15 19l-7-7 7-7" />
      </svg>
      Back
    </button>

    <AppSpinner v-if="loading" />
    <AppAlert v-else-if="error" type="error">{{ error }}</AppAlert>

    <div v-else-if="opinion" class="space-y-6">
      <header class="flex items-start justify-between gap-3">
        <div class="min-w-0 flex-1">
          <div class="flex items-center gap-2 mb-2 flex-wrap">
            <AppBadge v-if="subjectField" variant="secondary">
              {{ subjectField.icon_emoji ? subjectField.icon_emoji + ' ' : '' }}{{ subjectField.name }}
            </AppBadge>
            <ProvenanceBadge :provenance="opinion.provenance" />
          </div>
          <h1 class="text-2xl font-bold text-foreground">{{ opinion.title }}</h1>
          <p v-if="opinion.summary" class="mt-2 text-sm text-muted-foreground">
            {{ opinion.summary }}
          </p>
          <p class="mt-2 text-xs text-muted-foreground font-mono">
            by {{ opinion.author_address }} · {{ formatDate(opinion.published_at) }}
          </p>
        </div>
        <AppButton v-if="isOwner" variant="ghost" @click="withdraw">Withdraw</AppButton>
      </header>

      <div v-if="opinion.video_cid" class="rounded-xl overflow-hidden bg-black">
        <VideoPlayer :content-cid="opinion.video_cid" :title="opinion.title" />
      </div>

      <div class="rounded-xl border border-border bg-card p-5 space-y-3">
        <h3 class="text-xs font-semibold uppercase tracking-wider text-muted-foreground">
          Staked credentials
        </h3>

        <div v-if="opinion.credential_proof_ids.length === 0" class="text-xs text-muted-foreground">
          No credentials referenced.
        </div>

        <div v-else class="space-y-3">
          <div
            v-for="vc in linkedCredentials"
            :key="vc.id"
            class="rounded-lg bg-muted/30 p-3"
          >
            <div class="flex items-center justify-between gap-3">
              <div class="min-w-0">
                <div class="text-sm font-medium text-foreground">
                  <template v-if="skillClaim(vc)">
                    {{ skillClaim(vc)!.skill_id }}
                    <AppBadge variant="secondary" class="ml-2 text-[0.6rem]">
                      {{ bloomOrder[skillClaim(vc)!.level] ?? 'apply' }}
                    </AppBadge>
                  </template>
                  <template v-else>
                    {{ vc.type[vc.type.length - 1] }}
                  </template>
                </div>
                <div class="text-[11px] text-muted-foreground font-mono mt-1">
                  {{ vc.id }}
                </div>
              </div>
              <div class="text-right flex-shrink-0 space-y-1">
                <AppBadge v-if="vc.witness" variant="success" class="text-[0.6rem]">
                  on-chain witness
                </AppBadge>
                <div class="text-[10px] text-muted-foreground">
                  {{ vc.issuance_date.slice(0, 10) }}
                </div>
              </div>
            </div>
          </div>

          <div
            v-if="unresolvedIds().length > 0"
            class="text-xs text-muted-foreground"
          >
            Unsynced credentials:
            <span
              v-for="pid in unresolvedIds()"
              :key="pid"
              class="inline-block ml-1 font-mono"
            >
              {{ pid.slice(0, 16) }}…
            </span>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>
