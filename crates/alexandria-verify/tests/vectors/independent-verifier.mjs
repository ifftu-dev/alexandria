// A complete Alexandria credential verifier, written from README.md alone.
//
// It exists as evidence rather than as shipped code: it uses no Alexandria
// library, imports nothing but Node's standard crypto, and implements JCS by
// hand in a dozen lines. The proof is the W3C Data Integrity `eddsa-jcs-2022`
// cryptosuite, so the only primitives are JCS, SHA-256 and Ed25519.
// If this file passes every vector — and it does — then
// the format is documented well enough for somebody else to implement, which is
// the whole claim. It also implements the strict bounded JSON limits by hand
// and checks them against the exact bytes in `limits/`.
//
//   node crates/alexandria-verify/tests/vectors/independent-verifier.mjs
//
// It reads the vectors beside it, so it runs from any directory. It is not
// part of the Rust test suite, but CI runs it on every backend change and
// fails the build if any vector disagrees: a second implementation that
// nothing checks proves nothing, and the claim is only worth making if a
// divergence is caught rather than believed away.
//
// Note what is absent: no network, no JSON-LD processor, no DID resolver
// service, no Alexandria anything. It is intentionally small and auditable.
import { readFileSync, readdirSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { createHash, createPublicKey, verify } from 'node:crypto'

// Vector paths resolve against this file, not the working directory.
const here = path => fileURLToPath(new URL(path, import.meta.url))

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

// Organisation identity: a lowercase DNS name of at least two labels.
function normaliseDomain(domain) {
  const d = String(domain).trim().toLowerCase()
  if (d.length > 253 || !d.includes('.')) return null
  const label = /^[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?$/
  return d.split('.').every(l => label.test(l)) ? d : null
}

// The descriptor the caller resolved from the domain's TXT record must pass
// the same checks a reader of the record applies before it backs anything.
function descriptorValid(d) {
  if (d.format !== 'alexandria-org/1' || normaliseDomain(d.domain) !== d.domain) return false
  const name = String(d.name)
  if (!name || name.trim() !== name || [...name].length > 120 || /\p{Cc}/u.test(name)) return false
  if (!Array.isArray(d.keys) || d.keys.length === 0 || d.keys.length > 8) return false
  return d.keys.every(k => {
    try { return String(k).startsWith('did:key:') && !!pubkeyFromDidKey(k) } catch { return false }
  })
}

function organisationBacking(issuerId, issuerDomain, descriptor) {
  if (!descriptor) return { outcome: 'descriptor_not_supplied' }
  if (!descriptorValid(descriptor)) return { outcome: 'invalid_descriptor' }
  if (typeof issuerDomain === 'string' && normaliseDomain(issuerDomain) !== descriptor.domain) {
    return { outcome: 'wrong_domain' }
  }
  if (!descriptor.keys.includes(issuerId)) return { outcome: 'key_not_listed' }
  return { outcome: 'verified', name: descriptor.name, domain: descriptor.domain }
}

let pass = 0, fail = 0
for (const f of readdirSync(here('.')).filter(f => f.endsWith('.json')).sort()) {
  const v = JSON.parse(readFileSync(here(f), 'utf8'))
  const vc = v.credential

  // 1-2: eddsa-jcs-2022 hash data = SHA-256(JCS(proof config)) || SHA-256(JCS(document))
  const document = JSON.parse(JSON.stringify(vc)); delete document.proof
  const proofConfig = { ...vc.proof, '@context': vc['@context'] }; delete proofConfig.proofValue
  const hashData = Buffer.concat([
    createHash('sha256').update(Buffer.from(jcs(proofConfig), 'utf8')).digest(),
    createHash('sha256').update(Buffer.from(jcs(document), 'utf8')).digest(),
  ])

  // 3-5: the issuer's key, then the multibase proofValue over the hash data.
  // `issuer` is the DID, or an object whose `id` is the DID (VC 2.0).
  const issuerId = typeof vc.issuer === 'string' ? vc.issuer : String(vc.issuer?.id)
  let validSignature = false
  let issuerResolved = false
  const pendingReasons = []
  let key
  try {
    // Key: a did:key is its key, whatever the registry says; any other DID
    // method is looked up in the registry.
    const row = (v.store?.keyRegistry ?? []).find(r => r.did === issuerId)
    if (issuerId.startsWith('did:key:')) {
      key = pubkeyFromDidKey(issuerId)
      issuerResolved = true
    } else if (row) {
      const spki = Buffer.concat([Buffer.from('302a300506032b6570032100', 'hex'),
                                  Buffer.from(row.publicKeyHex, 'hex')])
      key = createPublicKey({ key: spki, format: 'der', type: 'spki' })
      issuerResolved = true
    } else {
      pendingReasons.push('issuer_key_missing')
    }
  } catch {
    issuerResolved = false
  }
  if (key) {
    try {
      const p = vc.proof
      if (p.type !== 'DataIntegrityProof' || p.cryptosuite !== 'eddsa-jcs-2022' ||
          p.proofPurpose !== 'assertionMethod') throw new Error('unsupported proof')
      // The verification method must be the issuer's: `<issuer DID>#<fragment>`.
      const [controller, fragment] = String(p.verificationMethod).split('#')
      if (controller !== issuerId || !fragment) throw new Error('method not controlled by issuer')
      if (!String(p.proofValue).startsWith('z')) throw new Error('not multibase base58btc')
      const sig = b58decode(p.proofValue.slice(1))
      if (sig.length !== 64) throw new Error('not an Ed25519 signature')
      validSignature = verify(null, hashData, key, sig)
    } catch { validSignature = false }
  }

  // 6: expiry, subject binding, and conclusive status evidence
  const expired = !!(vc.validUntil && vc.validUntil < v.verificationTime)
  const subjectBound = String(vc.credentialSubject.id).startsWith('did:')
  // `credentialStatus` is one entry or a list of them. Each entry's purpose
  // says which fact its list's bit decides; other purposes are skipped.
  const facts = { revocation: false, suspension: false }
  const missingReason = { revocation: 'status_list_missing', suspension: 'suspension_list_missing' }
  let statusValid = true
  const entries = vc.credentialStatus == null ? []
    : Array.isArray(vc.credentialStatus) ? vc.credentialStatus : [vc.credentialStatus]
  for (const entry of entries) {
    const purpose = entry.statusPurpose
    if (!Object.hasOwn(facts, purpose)) continue
    const indexText = entry.statusListIndex
    if (!/^\d+$/.test(indexText)) { statusValid = false; continue }
    const listId = entry.statusListCredential
    const lists = v.store?.statusLists ?? {}
    if (Object.hasOwn(lists, listId)) {
      const bits = Buffer.from(lists[listId], 'hex')
      const n = Number(indexText)
      const byte = Math.floor(n / 8)
      // Bitstring Status List: bit 0 of each byte is the most significant.
      if (!Number.isSafeInteger(n) || byte >= bits.length) statusValid = false
      else if (((bits[byte] >> (7 - (n % 8))) & 1) === 1) facts[purpose] = true
    } else if (!pendingReasons.includes(missingReason[purpose])) {
      pendingReasons.push(missingReason[purpose])
    }
  }
  const revoked = facts.revocation
  const suspended = facts.suspension
  const superseded = (v.store?.superseded ?? []).includes(vc.id)
  const integrityAnchored = false

  // 7: a coverage statement named by relatedResource must be the one the
  // integrity assertion names by digest. The digest is the address's last
  // part, so this needs no statement registry and no BLAKE3.
  const statementDigest = id => {
    const urn = 'urn:alexandria:coverage-statement:'
    let digest = null
    if (id.startsWith(urn)) digest = id.slice(urn.length)
    else {
      const at = id.lastIndexOf('/coverage-statements/')
      if (at >= 0 && id.slice(0, at).includes('://')) digest = id.slice(at + '/coverage-statements/'.length)
    }
    return digest && !digest.includes('/') ? digest : null
  }
  const namedDigest = vc.integrity?.coverageStatementDigest ?? null
  const coverageStatementMismatch = (vc.relatedResource ?? [])
    .map(r => statementDigest(String(r.id)))
    .some(d => d !== null && d !== namedDigest)

  // 8: answer custody. A self-issued credential (issuer is the subject)
  // cannot state `issuer` custody or carry a hosted attempt entry; a hosted
  // attempt entry must be the only one, say the issuer held the answers and
  // graded, and sit beside an assertion stating `issuer` custody. An
  // assertion that states no custody reads as `device`.
  const selfIssued = issuerId === String(vc.credentialSubject.id)
  const custody = vc.integrity ? (vc.integrity.answerCustody ?? 'device') : null
  const hostedEntries = (vc.evidence ?? []).filter(e =>
    e.type === 'HostedAttemptEvidence' ||
    (Array.isArray(e.type) && e.type.includes('HostedAttemptEvidence')))
  const levels = ['local', 'companion_attested', 'device_attested']
  const hostedValid = hostedEntries.length === 1 && (e =>
    typeof e.attemptId === 'string' && e.answerCustody === 'issuer' &&
    e.gradedBy === 'issuer' && typeof e.attestation?.outcome === 'string' &&
    levels.includes(e.verifiedAssuranceLevel))(hostedEntries[0])
  const hasHosted = hostedEntries.length > 0
  const custodyDefects = [
    [selfIssued && custody === 'issuer', 'issuer_custody_self_issued'],
    [selfIssued && hasHosted, 'hosted_attempt_self_issued'],
    [hasHosted && !hostedValid, 'hosted_attempt_invalid'],
    [hasHosted && custody !== 'issuer', 'hosted_attempt_custody_mismatch'],
  ].filter(([found]) => found).map(([, code]) => code)

  const issuerCanComplete = (validSignature && issuerResolved) ||
    (!issuerResolved && pendingReasons.includes('issuer_key_missing'))
  const passes = issuerCanComplete && statusValid && subjectBound &&
    allowedType(vc, v.policy) && !revoked &&
    !(v.policy.reject_expired && expired) &&
    !(v.policy.reject_suspended && suspended) &&
    !(v.policy.reject_superseded && superseded) &&
    !(v.policy.require_integrity_anchor && !integrityAnchored) &&
    !coverageStatementMismatch && custodyDefects.length === 0
  const acceptanceDecision = !passes ? 'reject' : pendingReasons.length ? 'pending' : 'accept'

  // coverageStatementMismatch and custodyDefects are omitted from a result
  // while false or empty.
  const e = { coverageStatementMismatch: false, custodyDefects: [], ...v.expect }
  const got = {
    validSignature, issuerResolved, revoked, statusValid, expired, subjectBound,
    integrityAnchored, suspended, superseded, coverageStatementMismatch,
    custodyDefects, pendingReasons, acceptanceDecision,
  }
  // Organisation backing: only vectors that state an expectation test it.
  if (v.expectOrganisationBacking) {
    got.organisationBacking = organisationBacking(issuerId, vc.issuer?.domain, v.organisationDescriptor)
    e.organisationBacking = v.expectOrganisationBacking
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

const manifest = JSON.parse(readFileSync(here('limits/manifest.json'), 'utf8'))
for (const c of manifest.cases) {
  const got = strictParse(readFileSync(here(`limits/${c.file}`)), manifest.limits)
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

const endorsements = JSON.parse(readFileSync(here('endorsements/manifest.json'), 'utf8'))
for (const c of endorsements.cases) {
  const got = endorsementOutcome(readFileSync(here(`endorsements/${c.file}`)), endorsements)
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
    const bytes = readFileSync(here(`endorsements/${file}`))
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
