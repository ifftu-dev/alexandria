let sessionToken: string | null = null

export function getProfileSessionToken(): string | null {
  return sessionToken
}

export function setProfileSessionToken(token: string | null): void {
  sessionToken = token
}
