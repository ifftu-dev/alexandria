#!/usr/bin/env node
// Verify an Alexandria credential with nothing but Node's standard library.
//
// No Alexandria code, no Alexandria server, no npm packages. The credential
// is a W3C Verifiable Credential (Data Model 2.0) secured with a Data
// Integrity proof, cryptosuite `eddsa-jcs-2022`, so the whole check is three
// standard primitives — JCS (RFC 8785), SHA-256 and Ed25519 — plus the
// did:key encoding (multibase base58btc, multicodec 0xed01). Revocation is a
// Bitstring Status List. The algorithm is the W3C one, restated with the test
// vectors in crates/alexandria-verify/tests/vectors/README.md; this file is an
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
import { createHash, createPublicKey, verify as cryptoVerify } from 'node:crypto'
import { gunzipSync } from 'node:zlib'

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

  // 2-5. eddsa-jcs-2022: hashData = SHA-256(JCS(proof config)) || SHA-256(JCS(document)),
  // proofValue = multibase base58btc of Ed25519(hashData).
  const proof = vc.proof
  if (!proof || typeof proof !== 'object') result.reasons.push('credential has no proof')
  else if (proof.type !== 'DataIntegrityProof' || proof.cryptosuite !== 'eddsa-jcs-2022') {
    result.reasons.push(`unsupported proof: ${proof.type} / ${proof.cryptosuite ?? 'no cryptosuite'}`)
  } else if (proof.proofPurpose !== 'assertionMethod') result.reasons.push('proofPurpose is not assertionMethod')
  else if (typeof proof.verificationMethod !== 'string' || proof.verificationMethod.split('#')[0] !== vc.issuer || !proof.verificationMethod.split('#')[1]) {
    result.reasons.push('verificationMethod is not controlled by the issuer')
  } else if (publicKey) {
    try {
      const document = structuredClone(vc); delete document.proof
      const config = { ...proof, '@context': vc['@context'] }; delete config.proofValue
      const hashData = Buffer.concat([
        createHash('sha256').update(Buffer.from(jcs(config), 'utf8')).digest(),
        createHash('sha256').update(Buffer.from(jcs(document), 'utf8')).digest(),
      ])
      if (typeof proof.proofValue !== 'string' || !proof.proofValue.startsWith('z')) throw new Error('proofValue is not multibase base58btc')
      const signature = base58Decode(proof.proofValue.slice(1))
      if (signature.length !== 64) throw new Error('proofValue is not a 64-byte Ed25519 signature')
      if (ed25519Verify(publicKey, hashData, signature)) result.signature = 'valid'
      else result.reasons.push('signature does not verify against the issuer key')
    } catch (e) { result.reasons.push(`signature: ${e.message}`) }
  }

  // 6. Validity window and subject binding.
  const from = vc.validFrom ? new Date(vc.validFrom) : null
  const until = vc.validUntil ? new Date(vc.validUntil) : null
  if (from && Number.isNaN(from.getTime())) result.reasons.push('validFrom is not a date')
  else if (from && now < from) { result.validity = 'not yet valid'; result.reasons.push('validFrom is in the future') }
  else if (until && now >= until) { result.validity = 'expired'; result.reasons.push(`expired at ${vc.validUntil}`) }
  else result.validity = until ? `valid until ${vc.validUntil}` : 'valid, no expiry'
  const subjectBound = typeof result.subject === 'string' && result.subject.startsWith('did:')
  if (!subjectBound) result.reasons.push('credentialSubject.id is not a DID, so the credential is not bound to a holder')

  // Bitstring Status List, if the bundle carries it. Bit n = byte n/8, bit n%8 from the most significant bit.
  const status = vc.credentialStatus
  if (!status) result.status = 'no status list named'
  else {
    const list = context.statusLists?.[status.statusListCredential]
    const index = Number.parseInt(status.statusListIndex, 10)
    if (!list) result.status = 'pending: status list not supplied (export a bundle to include it)'
    else if (!Number.isInteger(index) || index < 0 || index >= list.length * 8) { result.status = 'invalid status reference'; result.reasons.push('statusListIndex is out of range') }
    else result.status = (list[index >> 3] >> (7 - (index & 7))) & 1 ? 'revoked' : 'active'
  }

  const rejected =
    result.signature === 'invalid' || !subjectBound || result.validity === 'expired' || result.validity === 'not yet valid'
    || result.status === 'revoked' || result.status === 'invalid status reference'
  const pending = result.signature.startsWith('unverified') || result.status.startsWith('pending')
  result.decision = rejected ? 'reject' : pending ? 'pending' : 'accept'
  return result
}

/** `encodedList` → bitstring: multibase base64url (`u`) of GZIP bytes. */
export function decodeEncodedList(encoded) {
  if (typeof encoded !== 'string' || !encoded.startsWith('u')) throw new Error('encodedList is not multibase base64url')
  const compressed = Buffer.from(encoded.slice(1).replace(/-/g, '+').replace(/_/g, '/'), 'base64')
  return new Uint8Array(gunzipSync(compressed, { maxOutputLength: 1 << 20 }))
}

/**
 * Accept a bare credential, an array, or an exported bundle. A bundle's
 * `BitstringStatusListCredential` documents are verified against their
 * issuer's did:key before their bits are trusted; raw `status_lists` rows are
 * read only when no signed list credential covers that list id.
 */
export function loadInput(json) {
  if (Array.isArray(json)) return { credentials: json, keyRegistry: [], statusLists: {} }
  if (json && typeof json.format_version === 'string' && Array.isArray(json.credentials)) {
    const statusLists = {}
    const keyRegistry = json.key_registry ?? []
    for (const listVc of json.status_list_credentials ?? []) {
      const checked = verifyCredential(listVc, { keyRegistry })
      const subject = listVc.credentialSubject ?? {}
      if (checked.signature === 'valid' && subject.type === 'BitstringStatusList' && typeof listVc.id === 'string') {
        statusLists[listVc.id] = decodeEncodedList(subject.encodedList)
      }
    }
    for (const row of json.status_lists ?? []) {
      if (!(row.list_id in statusLists)) statusLists[row.list_id] = new Uint8Array(Buffer.from(row.bits_b64, 'base64'))
    }
    return { credentials: json.credentials, keyRegistry, statusLists }
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
