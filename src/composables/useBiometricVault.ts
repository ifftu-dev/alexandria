import { authenticate, checkStatus, getData, hasData, removeData, setData } from '@choochmeque/tauri-plugin-biometry-api'

const BIOMETRY_DOMAIN = 'org.alexandria.node'
/** Auto-clear session passwords after 15 minutes of inactivity. */
const SESSION_TIMEOUT_MS = 15 * 60 * 1000

// Credentials are keyed per profile so a multi-user device can biometric-
// unlock any profile, not just the last one enabled.
function vaultPasswordKey(profileId: string): string {
  return `vault_password_${profileId}`
}

// In-memory fallback (keyed by profile id), used when the OS keychain is
// unavailable — e.g. an unsigned dev build missing the entitlement.
const sessionBiometricPasswords = new Map<string, string>()
let sessionTimeout: ReturnType<typeof setTimeout> | null = null

function resetSessionTimeout() {
  if (sessionTimeout) clearTimeout(sessionTimeout)
  sessionTimeout = setTimeout(() => {
    sessionBiometricPasswords.clear()
    sessionTimeout = null
  }, SESSION_TIMEOUT_MS)
}

export interface BiometricStatus {
  isAvailable: boolean
  biometryType?: number
  error?: string
  errorCode?: string
}

function messageFromError(error: unknown): string {
  if (typeof error === 'string') return error
  if (error && typeof error === 'object' && 'message' in error) {
    return String((error as { message?: unknown }).message ?? error)
  }
  return String(error)
}

function isMissingEntitlementError(message: string): boolean {
  return message.includes('-34018') || message.includes('missing entitlement')
}

function isNotFoundError(message: string): boolean {
  const lower = message.toLowerCase()
  return lower.includes('itemnotfound') || lower.includes('not found')
}

export async function getBiometricStatus(): Promise<BiometricStatus> {
  try {
    const status = await checkStatus()
    return {
      isAvailable: !!status?.isAvailable,
      biometryType: status?.biometryType,
      error: status?.error,
      errorCode: status?.errorCode,
    }
  } catch (error) {
    return {
      isAvailable: false,
      error: messageFromError(error),
    }
  }
}

export async function biometricSupported(): Promise<boolean> {
  try {
    const status = await checkStatus()
    return !!status?.isAvailable
  } catch {
    return false
  }
}

export async function biometricCredentialExists(profileId: string): Promise<boolean> {
  try {
    const secure = await hasData({ domain: BIOMETRY_DOMAIN, name: vaultPasswordKey(profileId) })
    return secure || sessionBiometricPasswords.has(profileId)
  } catch {
    return sessionBiometricPasswords.has(profileId)
  }
}

export async function storeVaultPasswordForBiometric(
  profileId: string,
  password: string,
): Promise<'secure' | 'session'> {
  try {
    await setData({
      domain: BIOMETRY_DOMAIN,
      name: vaultPasswordKey(profileId),
      data: password,
    })
    // Keep an in-memory copy for this app session as a resilience fallback
    // in case keychain retrieval is flaky in some runtime configurations.
    sessionBiometricPasswords.set(profileId, password)
    resetSessionTimeout()
    return 'secure'
  } catch (error) {
    const message = messageFromError(error)
    if (isMissingEntitlementError(message)) {
      sessionBiometricPasswords.set(profileId, password)
      resetSessionTimeout()
      return 'session'
    }
    throw new Error(message)
  }
}

export async function clearBiometricVaultPassword(profileId: string): Promise<void> {
  try {
    await removeData({ domain: BIOMETRY_DOMAIN, name: vaultPasswordKey(profileId) })
  } catch (error) {
    const message = messageFromError(error)
    if (!isMissingEntitlementError(message)) {
      throw new Error(message)
    }
  } finally {
    sessionBiometricPasswords.delete(profileId)
    if (sessionBiometricPasswords.size === 0 && sessionTimeout) {
      clearTimeout(sessionTimeout)
      sessionTimeout = null
    }
  }
}

export async function getVaultPasswordViaBiometric(
  profileId: string,
  reason = 'Authenticate to unlock Alexandria',
): Promise<string> {
  try {
    const result = await getData({
      domain: BIOMETRY_DOMAIN,
      name: vaultPasswordKey(profileId),
      reason,
    })
    resetSessionTimeout()
    return result.data
  } catch (error) {
    const message = messageFromError(error)
    const sessionPassword = sessionBiometricPasswords.get(profileId)
    if (sessionPassword && (isMissingEntitlementError(message) || isNotFoundError(message))) {
      await authenticate(reason, { allowDeviceCredential: true })
      resetSessionTimeout()
      return sessionPassword
    }
    throw new Error(message)
  }
}
