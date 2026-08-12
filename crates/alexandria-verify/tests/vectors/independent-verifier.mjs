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

let pass = 0, fail = 0
for (const f of readdirSync('.').filter(f => f.endsWith('.json')).sort()) {
  const v = JSON.parse(readFileSync(f, 'utf8'))
  const vc = v.credential

  // 1-2: canonicalize with proof.jws emptied
  const copy = JSON.parse(JSON.stringify(vc)); copy.proof.jws = ''
  const signingBytes = Buffer.from(jcs(copy), 'utf8')

  // 3-5: detached JWS, RFC 7797 raw payload
  let validSignature = false
  try {
    const [hdr, mid, sigB64] = vc.proof.jws.split('.')
    if (mid !== '') throw new Error('not detached')
    const sig = Buffer.from(sigB64, 'base64url')
    const input = Buffer.concat([Buffer.from(hdr, 'utf8'), Buffer.from('.'), signingBytes])
    // Key: registry entry covering verificationTime, else did:key self-resolution
    const rows = (v.store?.keyRegistry ?? []).filter(r =>
      r.did === vc.issuer && r.validFrom <= v.verificationTime &&
      (r.validUntil === null || r.validUntil > v.verificationTime))
    let key
    if (rows.length) {
      rows.sort((a, b) => a.validFrom < b.validFrom ? 1 : -1)
      const spki = Buffer.concat([Buffer.from('302a300506032b6570032100', 'hex'),
                                  Buffer.from(rows[0].publicKeyHex, 'hex')])
      key = createPublicKey({ key: spki, format: 'der', type: 'spki' })
    } else {
      key = pubkeyFromDidKey(vc.issuer)
    }
    validSignature = verify(null, input, key, sig)
  } catch { validSignature = false }

  // 6: expiry, subject binding, status list
  const expired = !!(vc.validUntil && vc.validUntil < v.verificationTime)
  const subjectBound = String(vc.credentialSubject.id).startsWith('did:')
  let revoked = false
  if (vc.credentialStatus) {
    const bitsHex = v.store?.statusLists?.[vc.credentialStatus.statusListCredential]
    if (bitsHex) {
      const bits = Buffer.from(bitsHex, 'hex')
      const n = parseInt(vc.credentialStatus.statusListIndex, 10)
      revoked = ((bits[n >> 3] ?? 0) & (1 << (n & 7))) !== 0
    }
  }

  const e = v.expect
  const ok = validSignature === e.validSignature && expired === e.expired &&
             subjectBound === e.subjectBound && revoked === e.revoked
  console.log(`${ok ? 'PASS' : 'FAIL'}  ${f}`)
  if (!ok) {
    console.log(`      got sig=${validSignature} exp=${expired} bound=${subjectBound} rev=${revoked}`)
    console.log(`      want sig=${e.validSignature} exp=${e.expired} bound=${e.subjectBound} rev=${e.revoked}`)
    fail++
  } else pass++
}
console.log(`\n${pass} passed, ${fail} failed`)
