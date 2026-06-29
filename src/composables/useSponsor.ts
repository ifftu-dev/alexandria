import { ref } from 'vue'
import { useLocalApi } from './useLocalApi'
import type {
  CreateRoleAssessmentRequest,
  Organization,
  RoleAssessment,
  VerifiableCredential,
} from '@/types'

/**
 * Frontend wrapper for the enterprise sponsor / role-assessment IPC
 * surface (productization P2). Mirrors `commands/role_assessment.rs`
 * one-to-one. Reactive `organizations` / `roleAssessments` / `loading`
 * / `error` are shared across calls to the same instance.
 */
export function useSponsor() {
  const { invoke } = useLocalApi()

  const organizations = ref<Organization[]>([])
  const roleAssessments = ref<RoleAssessment[]>([])
  const loading = ref(false)
  const error = ref<string | null>(null)

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

  async function listOrganizations(ownerAddress?: string) {
    const result = await run(() =>
      invoke<Organization[]>('list_organizations', { ownerAddress }),
    )
    if (result) organizations.value = result
    return result
  }

  async function createOrganization(name: string, ownerAddress: string, did?: string) {
    const org = await run(() =>
      invoke<Organization>('create_organization', { name, ownerAddress, did }),
    )
    if (org) await listOrganizations()
    return org
  }

  async function listRoleAssessments(orgId?: string) {
    const result = await run(() =>
      invoke<RoleAssessment[]>('list_role_assessments', { orgId }),
    )
    if (result) roleAssessments.value = result
    return result
  }

  async function getRoleAssessment(id: string) {
    return run(() => invoke<RoleAssessment | null>('get_role_assessment', { id }))
  }

  async function createRoleAssessment(req: CreateRoleAssessmentRequest) {
    const ra = await run(() =>
      invoke<RoleAssessment>('create_role_assessment', { req }),
    )
    if (ra) await listRoleAssessments(req.org_id)
    return ra
  }

  async function setRoleAssessmentStatus(id: string, status: string) {
    const ra = await run(() =>
      invoke<RoleAssessment>('set_role_assessment_status', { id, status }),
    )
    if (ra) await listRoleAssessments()
    return ra
  }

  async function issueRoleCredential(
    roleAssessmentId: string,
    subject: string,
    integritySessionId: string,
  ) {
    return run(() =>
      invoke<VerifiableCredential>('issue_role_credential', {
        roleAssessmentId,
        subject,
        integritySessionId,
      }),
    )
  }

  return {
    organizations,
    roleAssessments,
    loading,
    error,
    listOrganizations,
    createOrganization,
    listRoleAssessments,
    getRoleAssessment,
    createRoleAssessment,
    setRoleAssessmentStatus,
    issueRoleCredential,
  }
}
