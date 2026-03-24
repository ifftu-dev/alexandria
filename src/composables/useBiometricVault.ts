import { authenticate, checkStatus, getData, hasData, removeData, setData } from '@choochmeque/tauri-plugin-biometry-api'

const BIOMETRY_DOMAIN = 'org.alexandria.node'
const VAULT_PASSWORD_KEY = 'vault_password'
/** Auto-clear session password after 15 minutes of inactivity. */
const SESSION_TIMEOUT_MS = 15 * 60 * 1000
let sessionBiometricPassword: string | null = null
let sessionTimeout: ReturnType<typeof setTimeout> | null = null

function resetSessionTimeout() {
  if (sessionTimeout) clearTimeout(sessionTimeout)
  sessionTimeout = setTimeout(() => {
    sessionBiometricPassword = null
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

export async function biometricCredentialExists(): Promise<boolean> {
  try {
    const secure = await hasData({ domain: BIOMETRY_DOMAIN, name: VAULT_PASSWORD_KEY })
    return secure || sessionBiometricPassword !== null
  } catch {
    return sessionBiometricPassword !== null
  }
}

export async function storeVaultPasswordForBiometric(password: string): Promise<'secure' | 'session'> {
  try {
    await setData({
      domain: BIOMETRY_DOMAIN,
      name: VAULT_PASSWORD_KEY,
      data: password,
    })
    // Keep an in-memory copy for this app session as a resilience fallback
    // in case keychain retrieval is flaky in some runtime configurations.
    sessionBiometricPassword = password
    resetSessionTimeout()
    return 'secure'
  } catch (error) {
    const message = messageFromError(error)
    if (isMissingEntitlementError(message)) {
      sessionBiometricPassword = password
      resetSessionTimeout()
      return 'session'
    }
    throw new Error(message)
  }
}

export async function clearBiometricVaultPassword(): Promise<void> {
  try {
    await removeData({ domain: BIOMETRY_DOMAIN, name: VAULT_PASSWORD_KEY })
  } catch (error) {
    const message = messageFromError(error)
    if (!isMissingEntitlementError(message)) {
      throw new Error(message)
    }
  } finally {
    sessionBiometricPassword = null
    if (sessionTimeout) {
      clearTimeout(sessionTimeout)
      sessionTimeout = null
    }
  }
}

export async function getVaultPasswordViaBiometric(reason = 'Authenticate to unlock Alexandria'): Promise<string> {
  try {
    const result = await getData({
      domain: BIOMETRY_DOMAIN,
      name: VAULT_PASSWORD_KEY,
      reason,
    })
    resetSessionTimeout()
    return result.data
  } catch (error) {
    const message = messageFromError(error)
    if (sessionBiometricPassword && (isMissingEntitlementError(message) || isNotFoundError(message))) {
      await authenticate(reason, { allowDeviceCredential: true })
      resetSessionTimeout()
      return sessionBiometricPassword
    }
    throw new Error(message)
  }
}
