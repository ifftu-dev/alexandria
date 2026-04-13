import { ref } from 'vue'
import { useLocalApi } from './useLocalApi'
import type {
  CreatePresentationRequest,
  CredentialBundle,
  DerivedSkillState,
  IssueCredentialRequest,
  PinboardCommitment,
  PresentationEnvelope,
  PresentationVerification,
  QuotaBreakdown,
  VerifiableCredential,
  VerificationResult,
} from '@/types'

/**
 * Frontend-side wrapper for the VC-first IPC surface (PRs 2–13).
 *
 * Wraps `useLocalApi().invoke` for every command registered in
 * `src-tauri/src/lib.rs:invoke_handler!` under credentials /
 * presentation / pinning / aggregation. The shape mirrors the Rust
 * `*_impl` functions one-to-one — anything unit-tested on the
 * backend has a corresponding method here.
 *
 * State (`credentials`, `loading`, `error`) is reactive and shared
 * across calls to the same `useCredentials()` instance. Pages that
 * want isolated state should call this composable per-component;
 * the project doesn't use Pinia, so each call returns a fresh
 * reactive scope.
 */
export function useCredentials() {
  const { invoke } = useLocalApi()

  const credentials = ref<VerifiableCredential[]>([])
  const loading = ref(false)
  const error = ref<string | null>(null)

  /** Run a backend call, surfacing errors into `error.value` instead of throwing. */
  async function run<T>(fn: () => Promise<T>): Promise<T | null> {
    loading.value = true
    error.value = null
    try {
      return await fn()
    } catch (e) {
      error.value = e instanceof Error ? e.message : String(e)
      return null
    } finally {
      loading.value = false
    }
  }

  /** Refresh `credentials.value` from the backend. */
  async function list(filter?: { subject?: string; skillId?: string }) {
    const result = await run(() =>
      invoke<VerifiableCredential[]>('list_credentials', {
        subject: filter?.subject,
        skillId: filter?.skillId,
      }),
    )
    if (result) credentials.value = result
    return result
  }

  async function get(credentialId: string) {
    return run(() =>
      invoke<VerifiableCredential | null>('get_credential', { credentialId }),
    )
  }

  async function issue(req: IssueCredentialRequest) {
    return run(() => invoke<VerifiableCredential>('issue_credential', { req }))
  }

  async function revoke(credentialId: string, reason: string) {
    return run(() =>
      invoke<void>('revoke_credential', { credentialId, reason }),
    )
  }

  async function verify(credential: VerifiableCredential) {
    return run(() =>
      invoke<VerificationResult>('verify_credential_cmd', { credential }),
    )
  }

  /** Returns the JCS-canonical bundle string, ready to write to disk. */
  async function exportBundle() {
    return run(() => invoke<string>('export_credentials_bundle'))
  }

  // --- Selective-disclosure presentations ---

  async function createPresentation(req: CreatePresentationRequest) {
    return run(() =>
      invoke<PresentationEnvelope>('create_presentation', { req }),
    )
  }

  async function verifyPresentation(
    envelope: PresentationEnvelope,
    audience: string,
  ) {
    return run(() =>
      invoke<PresentationVerification>('verify_presentation', {
        envelope,
        audience,
      }),
    )
  }

  // --- PinBoard ---

  async function declarePinboardCommitment(
    subjectDid: string,
    scope: string[],
  ) {
    return run(() =>
      invoke<PinboardCommitment>('declare_pinboard_commitment', {
        subjectDid,
        scope,
      }),
    )
  }

  async function revokePinboardCommitment(commitmentId: string) {
    return run(() =>
      invoke<void>('revoke_pinboard_commitment', { commitmentId }),
    )
  }

  async function listMyCommitments() {
    return run(() => invoke<PinboardCommitment[]>('list_my_commitments'))
  }

  async function listIncomingCommitments() {
    return run(() =>
      invoke<PinboardCommitment[]>('list_incoming_commitments'),
    )
  }

  async function getQuotaBreakdown() {
    return run(() => invoke<QuotaBreakdown>('get_quota_breakdown'))
  }

  // --- Derived skill state ---

  async function getDerivedSkillState(subjectDid: string, skillId: string) {
    return run(() =>
      invoke<DerivedSkillState | null>('get_derived_skill_state', {
        subjectDid,
        skillId,
      }),
    )
  }

  async function listDerivedStates(subjectDid?: string) {
    return run(() =>
      invoke<DerivedSkillState[]>('list_derived_states', { subjectDid }),
    )
  }

  async function recomputeAll() {
    return run(() => invoke<number>('recompute_all'))
  }

  return {
    // reactive state
    credentials,
    loading,
    error,

    // credentials
    list,
    get,
    issue,
    revoke,
    verify,
    exportBundle,

    // presentations
    createPresentation,
    verifyPresentation,

    // pinboard
    declarePinboardCommitment,
    revokePinboardCommitment,
    listMyCommitments,
    listIncomingCommitments,
    getQuotaBreakdown,

    // derived state
    getDerivedSkillState,
    listDerivedStates,
    recomputeAll,
  }
}

/** Convenience: parse a bundle string for the offline verifier UI flow. */
export function parseBundle(json: string): CredentialBundle {
  return JSON.parse(json) as CredentialBundle
}
