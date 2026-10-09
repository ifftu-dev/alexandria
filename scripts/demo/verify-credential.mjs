#!/usr/bin/env node
// Verify an Alexandria credential with nothing but Node's standard library.
//
// No Alexandria code, no Alexandria server, no npm packages. The whole check
// is two standard primitives — JCS (RFC 8785) and Ed25519 (RFC 8032) — plus
// the did:key encoding (multibase base58btc, multicodec 0xed01). The
// algorithm is the one published with the test vectors in
// crates/alexandria-verify/tests/vectors/README.md; this file is a second,
// independent implementation of it.
//
//   node scripts/demo/verify-credential.mjs <file.json> [--at 2026-10-09T00:00:00Z]
//
// <file.json> may be: a bare credential, an array of credentials, or the
// bundle the app exports from Credentials → Export (which also carries the
// issuer key registry and status lists, so revocation can be checked offline).
//
// Output, per credential: who issued it, to whom, what it claims, whether the
// signature verifies against the issuer's did:key, whether it is in its
// validity window, and its revocation status if the bundle supplies the list.
// Three separate answers on purpose; never one green tick.

import { readFileSync } from 'node:fs'
import { createPublicKey, verify as cryptoVerify } from 'node:crypto'

// ---------- JCS (RFC 8785) ----------
export function jcs(value) {
  if (value === null || typeof value === 'boolean') return JSON.stringify(value)
  if (typeof value === 'number') {
    if (!Number.isFinite(value)) throw new Error('JCS: non-finite number')
    return JSON.stringify(value) // ES6 Number::toString, which is what RFC 8785 specifies
  }
  if (typeof value === 'string') return JSON.stringify(value)
  if (Array.isArray(value)) return `[${value.map(jcs).join(',')}]`
  if (typeof value === 'object') {
    // Keys sort by UTF-16 code units, which is JavaScript's default string order.
    const keys = Object.keys(value).sort()
    return `{${keys.map((k) => `${JSON.stringify(k)}:${jcs(value[k])}`).join(',')}}`
  }
  throw new Error(`JCS: unsupported value ${typeof value}`)
}

// ---------- base58btc + did:key ----------
const ALPHABET = '123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz'
export function base58Decode(text) {
  let n = 0n
  for (const ch of text) {
    const i = ALPHABET.indexOf(ch)
    if (i < 0) throw new Error(`base58: bad character ${ch}`)
    n = n * 58n + BigInt(i)
  }
  const bytes = []
  while (n > 0n) { bytes.unshift(Number(n & 0xffn)); n >>= 8n }
  for (const ch of text) { if (ch === '1') bytes.unshift(0); else break }
  return Uint8Array.from(bytes)
}

/** did:key:z6Mk… → 32-byte Ed25519 public key. */
export function didKeyToPublicKey(did) {
  const m = /^did:key:z([1-9A-HJ-NP-Za-km-z]+)$/.exec(did)
  if (!m) throw new Error(`not a did:key: ${did}`)
  const decoded = base58Decode(m[1])
  if (decoded.length !== 34 || decoded[0] !== 0xed || decoded[1] !== 0x01) {
    throw new Error('did:key is not an Ed25519 key (expected multicodec 0xed01)')
  }
  return decoded.slice(2)
}

function b64urlDecode(s) {
  return new Uint8Array(Buffer.from(s.replace(/-/g, '+').replace(/_/g, '/'), 'base64'))
}

function ed25519Verify(publicKey, message, signature) {
  // Raw Ed25519 public key → SubjectPublicKeyInfo DER, which node:crypto accepts.
  const spkiPrefix = Buffer.from('302a300506032b6570032100', 'hex')
  const key = createPublicKey({ key: Buffer.concat([spkiPrefix, Buffer.from(publicKey)]), format: 'der', type: 'spki' })
  return cryptoVerify(null, Buffer.from(message), key, Buffer.from(signature))
}

// ---------- the check ----------
/**
 * Verify one credential.
 * @param {object} vc the signed credential
 * @param {object} [context] { now: Date, keyRegistry: [{did,key_id,public_key_hex,valid_from,valid_until}], statusLists: { [listId]: Uint8Array } }
 */
export function verifyCredential(vc, context = {}) {
  const now = context.now ?? new Date()
  const result = {
    id: vc.id ?? null,
    issuer: vc.issuer ?? null,
    subject: vc.credentialSubject?.id ?? null,
    claim: vc.credentialSubject ? {
      skillId: vc.credentialSubject.skillId ?? null,
      level: vc.credentialSubject.level ?? null,
      score: vc.credentialSubject.score ?? null,
    } : null,
    signature: 'invalid',
    validity: 'unknown',
    status: 'unknown',
    reasons: [],
  }

  // 1. Issuer key: a registry entry covering `now`, else did:key self-resolution.
  let publicKey = null
  const registry = (context.keyRegistry ?? []).filter((k) => k.did === vc.issuer)
  const covering = registry.find((k) => new Date(k.valid_from) <= now && (!k.valid_until || now < new Date(k.valid_until)))
  if (covering) publicKey = Buffer.from(covering.public_key_hex, 'hex')
  else {
    try { publicKey = didKeyToPublicKey(vc.issuer) } catch (e) {
      // Not self-resolving and no registry entry: the signature cannot be
      // judged either way. That is pending, not a forgery.
      result.signature = 'unverified: issuer key not available'
      result.reasons.push(`issuer: ${e.message}`)
    }
  }

  // 2-5. Detached JWS over JCS of the document with proof.jws emptied.
  const jws = vc.proof?.jws
  if (publicKey && typeof jws === 'string') {
    const parts = jws.split('.')
    if (parts.length !== 3 || parts[1] !== '') result.reasons.push('proof.jws is not a detached JWS (header..signature)')
    else {
      const copy = structuredClone(vc)
      copy.proof.jws = ''
      const signingInput = Buffer.concat([Buffer.from(parts[0], 'utf8'), Buffer.from('.'), Buffer.from(jcs(copy), 'utf8')])
      try {
        if (ed25519Verify(publicKey, signingInput, b64urlDecode(parts[2]))) result.signature = 'valid'
        else result.reasons.push('signature does not verify against the issuer key')
      } catch (e) { result.reasons.push(`signature: ${e.message}`) }
    }
  } else if (!jws) result.reasons.push('credential has no proof.jws')

  // 6. Validity window and subject binding.
  const from = vc.validFrom ? new Date(vc.validFrom) : null
  const until = vc.validUntil ? new Date(vc.validUntil) : null
  if (from && Number.isNaN(from.getTime())) result.reasons.push('validFrom is not a date')
  else if (from && now < from) { result.validity = 'not yet valid'; result.reasons.push('validFrom is in the future') }
  else if (until && now >= until) { result.validity = 'expired'; result.reasons.push(`expired at ${vc.validUntil}`) }
  else result.validity = until ? `valid until ${vc.validUntil}` : 'valid, no expiry'
  const subjectBound = typeof result.subject === 'string' && result.subject.startsWith('did:')
  if (!subjectBound) result.reasons.push('credentialSubject.id is not a DID, so the credential is not bound to a holder')

  // Status list, if the bundle carries it. Bit n = byte n/8, bit n%8, little-endian within the byte.
  const status = vc.credentialStatus
  if (!status) result.status = 'no status list named'
  else {
    const list = context.statusLists?.[status.statusListCredential]
    const index = Number.parseInt(status.statusListIndex, 10)
    if (!list) result.status = 'pending: status list not supplied (export a bundle to include it)'
    else if (!Number.isInteger(index) || index < 0 || index >= list.length * 8) { result.status = 'invalid status reference'; result.reasons.push('statusListIndex is out of range') }
    else result.status = (list[index >> 3] >> (index & 7)) & 1 ? 'revoked' : 'active'
  }

  const rejected =
    result.signature === 'invalid' || !subjectBound || result.validity === 'expired' || result.validity === 'not yet valid'
    || result.status === 'revoked' || result.status === 'invalid status reference'
  const pending = result.signature.startsWith('unverified') || result.status.startsWith('pending')
  result.decision = rejected ? 'reject' : pending ? 'pending' : 'accept'
  return result
}

/** Accept a bare credential, an array, or an exported bundle. */
export function loadInput(json) {
  if (Array.isArray(json)) return { credentials: json, keyRegistry: [], statusLists: {} }
  if (json && typeof json.format_version === 'string' && Array.isArray(json.credentials)) {
    const statusLists = {}
    for (const row of json.status_lists ?? []) statusLists[row.list_id] = new Uint8Array(Buffer.from(row.bits_b64, 'base64'))
    return { credentials: json.credentials, keyRegistry: json.key_registry ?? [], statusLists }
  }
  if (json && json.proof) return { credentials: [json], keyRegistry: [], statusLists: {} }
  throw new Error('input is not a credential, an array of credentials, or an alexandria-credential-bundle')
}

function main(argv) {
  const file = argv.find((a) => !a.startsWith('--'))
  if (!file) {
    console.error('usage: node verify-credential.mjs <credential-or-bundle.json> [--at <ISO time>]')
    return 2
  }
  const atIndex = argv.indexOf('--at')
  const now = atIndex >= 0 ? new Date(argv[atIndex + 1]) : new Date()
  const { credentials, keyRegistry, statusLists } = loadInput(JSON.parse(readFileSync(file, 'utf8')))
  let worst = 0
  for (const vc of credentials) {
    const r = verifyCredential(vc, { now, keyRegistry, statusLists })
    console.log(`\n${r.id ?? '(no id)'}`)
    console.log(`  issuer     ${r.issuer}`)
    console.log(`  subject    ${r.subject}`)
    if (r.claim) console.log(`  claim      ${r.claim.skillId}  level ${r.claim.level}  score ${r.claim.score}`)
    console.log(`  signature  ${r.signature}`)
    console.log(`  validity   ${r.validity}`)
    console.log(`  status     ${r.status}`)
    console.log(`  decision   ${r.decision.toUpperCase()}`)
    for (const reason of r.reasons) console.log(`             - ${reason}`)
    if (r.issuer) console.log(`  resolve    https://dev.uniresolver.io/#${r.issuer}  (third-party did:key resolver)`)
    worst = Math.max(worst, r.decision === 'accept' ? 0 : r.decision === 'pending' ? 1 : 3)
  }
  console.log(`\nChecked ${credentials.length} credential(s) at ${now.toISOString()} with node:crypto only.`)
  return worst
}

if (process.argv[1] && import.meta.url === new URL(`file://${process.argv[1]}`).href) {
  process.exitCode = main(process.argv.slice(2))
}
