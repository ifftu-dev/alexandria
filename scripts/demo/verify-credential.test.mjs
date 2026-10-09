// node --test scripts/demo/verify-credential.test.mjs
//
// The standalone verifier must agree with the published vectors on every
// case it can judge from a document alone, and must say "pending", not
// "accept", when evidence it was not given would be needed.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync, readdirSync } from 'node:fs'
import { join } from 'node:path'
import { gzipSync } from 'node:zlib'
import { decodeEncodedList, didKeyToPublicKey, jcs, loadInput, verifyCredential } from './verify-credential.mjs'

const VECTORS = join(import.meta.dirname, '../../crates/alexandria-verify/tests/vectors')
const vector = (name) => JSON.parse(readFileSync(join(VECTORS, name), 'utf8'))

function contextOf(v) {
  const statusLists = {}
  for (const [id, hex] of Object.entries(v.store?.statusLists ?? {})) statusLists[id] = new Uint8Array(Buffer.from(hex, 'hex'))
  const keyRegistry = (v.store?.keyRegistry ?? []).map((k) => ({
    did: k.did, key_id: k.keyId, public_key_hex: k.publicKeyHex, valid_from: k.validFrom, valid_until: k.validUntil ?? null,
  }))
  return { now: new Date(v.verificationTime), keyRegistry, statusLists }
}

test('JCS sorts keys, keeps numbers in ES6 form and escapes strings', () => {
  assert.equal(jcs({ b: 1, a: [true, null, 'x"y'], c: { z: 0.5, y: 1e21 } }), '{"a":[true,null,"x\\"y"],"b":1,"c":{"y":1e+21,"z":0.5}}')
})

test('did:key resolves to a 32-byte Ed25519 key', () => {
  const key = didKeyToPublicKey('did:key:z6MkvwtuLDu19TcdGubq4U8c81JmgULwCSrdExbBCCZ6oVQT')
  assert.equal(key.length, 32)
  assert.throws(() => didKeyToPublicKey('did:web:example.com'))
})

test('agrees with every published vector on signature, expiry and revocation', () => {
  const files = readdirSync(VECTORS).filter((f) => /^\d\d-.*\.json$/.test(f))
  assert.ok(files.length >= 12)
  for (const file of files) {
    const v = vector(file)
    const r = verifyCredential(v.credential, contextOf(v))
    assert.equal(r.signature === 'valid', v.expect.validSignature, `${file}: signature`)
    assert.equal(r.validity === 'expired', v.expect.expired, `${file}: expiry`)
    assert.equal(r.status === 'revoked', v.expect.revoked, `${file}: revocation`)
    // Policy-only outcomes (allowed types, permissive expiry, suspension) are
    // the verifier's caller's business; on everything else the decision agrees.
    const policyOnly = file.startsWith('09-') || file.startsWith('10-')
    if (!policyOnly) assert.equal(r.decision, v.expect.acceptanceDecision, `${file}: decision (${r.reasons.join('; ')})`)
  }
})

test('a one-byte change to the claim breaks the signature', () => {
  const v = vector('01-valid.json')
  const good = verifyCredential(v.credential, contextOf(v))
  assert.equal(good.decision, 'accept')
  const forged = structuredClone(v.credential)
  forged.credentialSubject.score = 1.0
  const bad = verifyCredential(forged, contextOf(v))
  assert.equal(bad.signature, 'invalid')
  assert.equal(bad.decision, 'reject')
})

test('an issuer whose key cannot be found is pending, not forged', () => {
  const v = vector('12-missing-issuer-key.json')
  const r = verifyCredential(v.credential, contextOf(v))
  assert.match(r.signature, /^unverified/)
  assert.equal(r.decision, 'pending')
})

test('a missing status list is pending, never accepted', () => {
  const v = vector('07-revoked.json')
  const withoutList = verifyCredential(v.credential, { now: new Date(v.verificationTime) })
  assert.equal(withoutList.signature, 'valid')
  assert.match(withoutList.status, /^pending/)
  assert.equal(withoutList.decision, 'pending')
})

test('decodes a Bitstring Status List encodedList and reads bits from the left', () => {
  const bits = Buffer.from('0040000000000000', 'hex')
  const encoded = 'u' + gzipSync(bits).toString('base64').replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '')
  assert.deepEqual(Buffer.from(decodeEncodedList(encoded)), bits)
  assert.throws(() => decodeEncodedList('zNope'))
})

test('reads the app export bundle shape, including its raw status lists', () => {
  const v = vector('07-revoked.json')
  const bundle = {
    format_version: 'alexandria-credential-bundle/1.0',
    credentials: [v.credential, vector('01-valid.json').credential],
    key_registry: [],
    status_lists: [{ list_id: 'urn:uuid:status-list-1', issuer_did: v.credential.issuer, version: 1, status_purpose: 'revocation', bits_b64: Buffer.from('0040000000000000', 'hex').toString('base64'), bit_length: 64 }],
  }
  const { credentials, statusLists } = loadInput(bundle)
  assert.equal(credentials.length, 2)
  const [revoked, fine] = credentials.map((c) => verifyCredential(c, { now: new Date(v.verificationTime), statusLists }))
  assert.equal(revoked.status, 'revoked')
  assert.equal(revoked.decision, 'reject')
  assert.equal(fine.decision, 'accept')
  assert.throws(() => loadInput({ hello: 'world' }))
})
