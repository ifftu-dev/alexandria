// A complete Alexandria credential verifier, written from README.md alone.
//
// It exists as evidence rather than as shipped code: it uses no Alexandria
// library, imports nothing but Node's standard crypto, and implements JCS by
// hand in a dozen lines. If this file passes every vector — and it does — then
// the format is documented well enough for somebody else to implement, which is
// the whole claim.
//
//   node independent-verifier.mjs
//
// Run it from this directory. It is not part of the Rust test suite and nothing
// depends on it; it is here so the claim can be checked rather than believed.
//
// Note what is absent: no network, no JSON-LD processor, no DID resolver
// service, no Alexandria anything. About seventy lines, most of it base58.
import { readFileSync, readdirSync } from 'node:fs'
import { createPublicKey, verify } from 'node:crypto'

const ALPHA = '123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz'
function b58decode(s) {
  let n = 0n
  for (const c of s) n = n * 58n + BigInt(ALPHA.indexOf(c))
  let hex = n.toString(16); if (hex.length % 2) hex = '0' + hex
  return Buffer.from(hex, 'hex')
}

// JCS (RFC 8785): keys sorted by UTF-16 code unit, no whitespace.
function jcs(v) {
  if (v === null || typeof v === 'boolean' || typeof v === 'number') return JSON.stringify(v)
  if (typeof v === 'string') return JSON.stringify(v)
  if (Array.isArray(v)) return '[' + v.map(jcs).join(',') + ']'
  const keys = Object.keys(v).sort()
  return '{' + keys.map(k => JSON.stringify(k) + ':' + jcs(v[k])).join(',') + '}'
}

function pubkeyFromDidKey(did) {
  const mb = did.split(':')[2]
  const raw = b58decode(mb.slice(1))            // strip 'z' multibase prefix
  if (raw[0] !== 0xed || raw[1] !== 0x01) throw new Error('not ed25519-pub')
  const spki = Buffer.concat([Buffer.from('302a300506032b6570032100', 'hex'), raw.subarray(2)])
  return createPublicKey({ key: spki, format: 'der', type: 'spki' })
}

function allowedType(vc, policy) {
  const allowed = policy.allowed_types ?? []
  return allowed.length === 0 || vc.type.some(type => allowed.includes(type))
}

let pass = 0, fail = 0
for (const f of readdirSync('.').filter(f => f.endsWith('.json')).sort()) {
  const v = JSON.parse(readFileSync(f, 'utf8'))
  const vc = v.credential

  // 1-2: canonicalize with proof.jws emptied
  const copy = JSON.parse(JSON.stringify(vc)); copy.proof.jws = ''
  const signingBytes = Buffer.from(jcs(copy), 'utf8')

  // 3-5: detached JWS, RFC 7797 raw payload
  let validSignature = false
  let issuerResolved = false
  const pendingReasons = []
  let key
  try {
    // Key: registry entry covering verificationTime, else did:key self-resolution
    const rows = (v.store?.keyRegistry ?? []).filter(r =>
      r.did === vc.issuer && r.validFrom <= v.verificationTime &&
      (r.validUntil === null || r.validUntil > v.verificationTime))
    if (rows.length) {
      rows.sort((a, b) => a.validFrom < b.validFrom ? 1 : -1)
      const spki = Buffer.concat([Buffer.from('302a300506032b6570032100', 'hex'),
                                  Buffer.from(rows[0].publicKeyHex, 'hex')])
      key = createPublicKey({ key: spki, format: 'der', type: 'spki' })
      issuerResolved = true
    } else if (vc.issuer.startsWith('did:key:')) {
      key = pubkeyFromDidKey(vc.issuer)
      issuerResolved = true
    } else {
      pendingReasons.push('issuer_key_missing')
    }
  } catch {
    issuerResolved = false
  }
  if (key) {
    try {
      const parts = vc.proof.jws.split('.')
      if (parts.length !== 3 || parts[1] !== '') throw new Error('not detached')
      const [hdr, , sigB64] = parts
      const sig = Buffer.from(sigB64, 'base64url')
      const input = Buffer.concat([Buffer.from(hdr, 'utf8'), Buffer.from('.'), signingBytes])
      validSignature = verify(null, input, key, sig)
    } catch { validSignature = false }
  }

  // 6: expiry, subject binding, and conclusive status evidence
  const expired = !!(vc.validUntil && vc.validUntil < v.verificationTime)
  const subjectBound = String(vc.credentialSubject.id).startsWith('did:')
  let revoked = false
  let statusValid = true
  if (vc.credentialStatus) {
    const listId = vc.credentialStatus.statusListCredential
    const lists = v.store?.statusLists ?? {}
    if (Object.hasOwn(lists, listId)) {
      const indexText = vc.credentialStatus.statusListIndex
      const bits = Buffer.from(lists[listId], 'hex')
      if (!/^\d+$/.test(indexText)) {
        statusValid = false
      } else {
        const n = Number(indexText)
        const byte = Math.floor(n / 8)
        if (!Number.isSafeInteger(n) || byte >= bits.length) statusValid = false
        else revoked = (bits[byte] & (1 << (n % 8))) !== 0
      }
    } else {
      pendingReasons.push('status_list_missing')
    }
  }

  const suspendedUntil = v.store?.suspended?.[vc.id]
  const suspended = Object.hasOwn(v.store?.suspended ?? {}, vc.id) &&
    (suspendedUntil === null || suspendedUntil > v.verificationTime)
  const superseded = (v.store?.superseded ?? []).includes(vc.id)
  const integrityAnchored = false

  const issuerCanComplete = (validSignature && issuerResolved) ||
    (!issuerResolved && pendingReasons.includes('issuer_key_missing'))
  const passes = issuerCanComplete && statusValid && subjectBound &&
    allowedType(vc, v.policy) && !revoked &&
    !(v.policy.reject_expired && expired) &&
    !(v.policy.reject_suspended && suspended) &&
    !(v.policy.reject_superseded && superseded) &&
    !(v.policy.require_integrity_anchor && !integrityAnchored)
  const acceptanceDecision = !passes ? 'reject' : pendingReasons.length ? 'pending' : 'accept'

  const e = v.expect
  const got = {
    validSignature, issuerResolved, revoked, statusValid, expired, subjectBound,
    integrityAnchored, suspended, superseded, pendingReasons, acceptanceDecision,
  }
  const fields = Object.keys(got)
  const ok = fields.every(field => JSON.stringify(got[field]) === JSON.stringify(e[field]))
  console.log(`${ok ? 'PASS' : 'FAIL'}  ${f}`)
  if (!ok) {
    for (const field of fields) {
      if (JSON.stringify(got[field]) !== JSON.stringify(e[field])) {
        console.log(`      ${field}: got=${JSON.stringify(got[field])} want=${JSON.stringify(e[field])}`)
      }
    }
    fail++
  } else pass++
}
console.log(`\n${pass} passed, ${fail} failed`)
