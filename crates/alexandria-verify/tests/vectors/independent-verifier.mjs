// A complete Alexandria credential verifier, written from README.md alone.
//
// It exists as evidence rather than as shipped code: it uses no Alexandria
// library, imports nothing but Node's standard crypto, and implements JCS by
// hand in a dozen lines. If this file passes every vector — and it does — then
// the format is documented well enough for somebody else to implement, which is
// the whole claim. It also implements the strict bounded JSON limits by hand
// and checks them against the exact bytes in `limits/`.
//
//   node independent-verifier.mjs
//
// Run it from this directory. It is not part of the Rust test suite and nothing
// depends on it; it is here so the claim can be checked rather than believed.
//
// Note what is absent: no network, no JSON-LD processor, no DID resolver
// service, no Alexandria anything. It is intentionally small and auditable.
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
// Untrusted input limits (README.md, "Untrusted input limits"). Each case is
// exact raw bytes: a verifier must accept them, or refuse them for the same
// reason, before any typed decoding or signature work.
function strictParse(bytes, limits) {
  if (bytes.length > limits.maxBytes) return 'too_large'
  let text
  try {
    text = new TextDecoder('utf-8', { fatal: true, ignoreBOM: true }).decode(bytes)
  } catch {
    return 'invalid'
  }
  const MAX_SAFE = 2n ** 53n - 1n
  const NUMBER = /-?(0|[1-9]\d*)(\.\d+)?([eE][+-]?\d+)?/y
  const ESCAPES = { '"': '"', '\\': '\\', '/': '/', b: '\b', f: '\f', n: '\n', r: '\r', t: '\t' }
  class Refusal { constructor(kind) { this.kind = kind } }
  const refuse = kind => { throw new Refusal(kind) }
  let i = 0

  const skipWhitespace = () => { while (i < text.length && ' \t\n\r'.includes(text[i])) i++ }
  const hex4 = () => {
    const digits = text.slice(i, i + 4)
    if (!/^[0-9a-fA-F]{4}$/.test(digits)) refuse('invalid')
    i += 4
    return parseInt(digits, 16)
  }
  const string = () => {
    if (text[i] !== '"') refuse('invalid')
    i++
    let out = ''
    for (;;) {
      if (i >= text.length) refuse('invalid')
      const c = text[i++]
      if (c === '"') break
      if (c === '\\') {
        const e = text[i++]
        if (Object.hasOwn(ESCAPES, e)) out += ESCAPES[e]
        else if (e === 'u') {
          const unit = hex4()
          if (unit >= 0xd800 && unit <= 0xdbff) {
            if (text.slice(i, i + 2) !== '\\u') refuse('invalid')
            i += 2
            const low = hex4()
            if (low < 0xdc00 || low > 0xdfff) refuse('invalid')
            out += String.fromCharCode(unit, low)
          } else if (unit >= 0xdc00 && unit <= 0xdfff) refuse('invalid')
          else out += String.fromCharCode(unit)
        } else refuse('invalid')
      } else if (c < ' ') refuse('invalid')
      else out += c
    }
    if (Buffer.byteLength(out, 'utf8') > limits.maxStringBytes) refuse('string_too_long')
    return out
  }
  const number = () => {
    NUMBER.lastIndex = i
    const m = NUMBER.exec(text)
    if (!m) refuse('invalid')
    i += m[0].length
    if (!m[2] && !m[3]) {
      const n = BigInt(m[0])
      if ((n < 0n ? -n : n) > MAX_SAFE) refuse('unsafe_number')
    } else {
      const x = Number(m[0])
      // A literal that overflows a double is malformed, not merely unsafe.
      if (!Number.isFinite(x)) refuse('invalid')
      if (Math.abs(x) > Number(MAX_SAFE)) refuse('unsafe_number')
    }
  }
  const value = depth => {
    skipWhitespace()
    const c = text[i]
    if (c === '[' || c === '{') {
      if (depth + 1 > limits.maxDepth) refuse('too_deep')
      i++
      skipWhitespace()
      if (c === '[') {
        if (text[i] === ']') { i++; return }
        let count = 0
        for (;;) {
          value(depth + 1)
          if (count === limits.maxArrayLen) refuse('too_many_elements')
          count++
          skipWhitespace()
          if (text[i] === ',') { i++; continue }
          if (text[i] === ']') { i++; return }
          refuse('invalid')
        }
      }
      if (text[i] === '}') { i++; return }
      const keys = new Set()
      for (;;) {
        skipWhitespace()
        const key = string()
        if (keys.has(key)) refuse('duplicate_key')
        if (keys.size === limits.maxObjectEntries) refuse('too_many_entries')
        keys.add(key)
        skipWhitespace()
        if (text[i] !== ':') refuse('invalid')
        i++
        value(depth + 1)
        skipWhitespace()
        if (text[i] === ',') { i++; continue }
        if (text[i] === '}') { i++; return }
        refuse('invalid')
      }
    }
    if (c === '"') { string(); return }
    for (const literal of ['true', 'false', 'null']) {
      if (text.startsWith(literal, i)) { i += literal.length; return }
    }
    number()
  }

  try {
    value(0)
    skipWhitespace()
    if (i !== text.length) refuse('invalid')
    return 'accept'
  } catch (error) {
    if (error instanceof Refusal) return error.kind
    throw error
  }
}

const manifest = JSON.parse(readFileSync('limits/manifest.json', 'utf8'))
for (const c of manifest.cases) {
  const got = strictParse(readFileSync(`limits/${c.file}`), manifest.limits)
  const ok = got === c.expect
  console.log(`${ok ? 'PASS' : 'FAIL'}  limits/${c.file}`)
  if (!ok) {
    console.log(`      got=${got} want=${c.expect}`)
    fail++
  } else pass++
}

// Course completion endorsements (README.md, "Course completion
// endorsements"). Strict bounded parsing first, then the binding, attestor
// authorization, key identity, and the domain-separated Ed25519 signature.
function b58encode(bytes) {
  let n = BigInt('0x' + Buffer.from(bytes).toString('hex'))
  let out = ''
  while (n > 0n) {
    out = ALPHA[Number(n % 58n)] + out
    n /= 58n
  }
  for (const byte of bytes) {
    if (byte !== 0) break
    out = '1' + out
  }
  return out
}

function endorsementOutcome(bytes, context) {
  const structural = strictParse(bytes, context.limits)
  if (structural !== 'accept') return structural
  const endorsement = JSON.parse(Buffer.from(bytes).toString('utf8'))
  if (jcs(endorsement.binding) !== jcs(context.expectedBinding)) return 'binding_mismatch'
  const authorized = context.policy.authorized_attestors
    .find(attestor => attestor.did === endorsement.attestor_did)
  if (!authorized || authorized.public_key_hex !== endorsement.attestor_public_key_hex) {
    return 'unauthorized_attestor'
  }
  if (!/^[0-9a-f]{64}$/.test(endorsement.attestor_public_key_hex)) return 'invalid_public_key'
  const rawKey = Buffer.from(endorsement.attestor_public_key_hex, 'hex')
  const did = 'did:key:z' + b58encode(Buffer.concat([Buffer.from([0xed, 0x01]), rawKey]))
  if (did !== endorsement.attestor_did) return 'identity_mismatch'
  if (!/^[0-9a-f]{128}$/.test(endorsement.signature_hex)) return 'invalid_signature'
  const key = createPublicKey({
    key: Buffer.concat([Buffer.from('302a300506032b6570032100', 'hex'), rawKey]),
    format: 'der',
    type: 'spki',
  })
  const message = Buffer.concat([
    Buffer.from(context.domain, 'utf8'),
    Buffer.from([0]),
    Buffer.from(jcs(context.expectedBinding), 'utf8'),
  ])
  return verify(null, message, key, Buffer.from(endorsement.signature_hex, 'hex'))
    ? 'valid'
    : 'invalid_signature'
}

const endorsements = JSON.parse(readFileSync('endorsements/manifest.json', 'utf8'))
for (const c of endorsements.cases) {
  const got = endorsementOutcome(readFileSync(`endorsements/${c.file}`), endorsements)
  const ok = got === c.expect
  console.log(`${ok ? 'PASS' : 'FAIL'}  endorsements/${c.file}`)
  if (!ok) {
    console.log(`      got=${got} want=${c.expect}`)
    fail++
  } else pass++
}
for (const t of endorsements.thresholds) {
  const counted = new Set()
  let rejected = 0
  for (const file of t.files) {
    const bytes = readFileSync(`endorsements/${file}`)
    const attestor = JSON.parse(bytes.toString('utf8')).attestor_did
    if (endorsementOutcome(bytes, endorsements) === 'valid' && !counted.has(attestor)) {
      counted.add(attestor)
    } else rejected++
  }
  const satisfied = counted.size >= endorsements.policy.required_attestors
  const ok = counted.size === t.validAttestors &&
    rejected === t.rejectedEndorsements && satisfied === t.satisfied
  console.log(`${ok ? 'PASS' : 'FAIL'}  endorsements threshold: ${t.description}`)
  if (!ok) {
    console.log(`      got valid=${counted.size} rejected=${rejected} satisfied=${satisfied}`)
    fail++
  } else pass++
}

console.log(`\n${pass} passed, ${fail} failed`)
process.exitCode = fail ? 1 : 0
