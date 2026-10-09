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
import { createServer } from 'node:http'
import { decodeEncodedList, didKeyToPublicKey, isListUrl, jcs, loadInput, resolveStatusLists, verifyCredential } from './verify-credential.mjs'

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

test('recognises which status list references can be fetched', () => {
  assert.equal(isListUrl('https://cloud.example/status-lists/did:key:z6Mk/1'), true)
  assert.equal(isListUrl('http://127.0.0.1:8080/status-lists/did:key:z6Mk/1'), true)
  assert.equal(isListUrl('http://cloud.example/status-lists/did:key:z6Mk/1'), false)
  assert.equal(isListUrl('urn:alexandria:status-list:did:key:z6Mk:1'), false)
})

test('fetches a URL-named status list, checks it is the issuer’s, and reads the bit', async () => {
  // 07-revoked carries the signed list document in its store; serve it over
  // HTTP under a URL, and point the credential at that URL. The document's
  // own id must be the URL, so the stored one (a URN) is a stand-in that must
  // be rejected, and a fresh fixture is built from the bundle shape instead.
  const bundle = JSON.parse(readFileSync(join(import.meta.dirname, 'fixtures/status-list-bundle.json'), 'utf8'))
  const { credentials, keyRegistry, statusLists: offline } = loadInput(bundle)
  const listVc = bundle.status_list_credentials[0]
  const pathOf = (id) => new URL(id).pathname
  const served = new Map()
  const server = createServer((request, response) => {
    const document = served.get(request.url)
    if (!document) { response.writeHead(404); response.end('{}'); return }
    response.writeHead(200, { 'content-type': 'application/vc' })
    response.end(JSON.stringify(document))
  })
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve))
  const origin = `http://127.0.0.1:${server.address().port}`
  try {
    // Rewrite both the credential reference and the list id to the live origin.
    const rebase = (s) => s.replace(bundle.origin, origin)
    const live = structuredClone(listVc); live.id = rebase(live.id); live.credentialSubject.id = rebase(live.credentialSubject.id)
    served.set(pathOf(live.id), live)
    const vcs = credentials.map((vc) => { const c = structuredClone(vc); c.credentialStatus.statusListCredential = rebase(c.credentialStatus.statusListCredential); c.credentialStatus.id = rebase(c.credentialStatus.id); return c })
    assert.equal(Object.keys(offline).length, 1, 'the export carries the list offline too; the fetch path below ignores it')

    // The rebased list document was signed under its original id, so its
    // signature no longer verifies: a list that is not what the issuer signed
    // is left out and the credential stays pending.
    const lists = {}
    let problems = await resolveStatusLists(vcs, lists)
    assert.equal(problems.length, 1)
    assert.match(problems[0], /signature/)
    assert.match(verifyCredential(vcs[0], { keyRegistry, statusLists: lists }).status, /^pending/)

    // Serve the document exactly as signed, at its own URL.
    served.set(pathOf(listVc.id), listVc)
    const fetchAt = (url, init) => fetch(rebase(url), init)
    const lists2 = {}
    problems = await resolveStatusLists(credentials, lists2, { fetch: fetchAt })
    assert.deepEqual(problems, [])
    const results = credentials.map((vc) => verifyCredential(vc, { keyRegistry, statusLists: lists2 }))
    assert.deepEqual(results.map((r) => r.status), ['revoked', 'active'])
    assert.deepEqual(results.map((r) => r.decision), ['reject', 'accept'])

    // A list signed by somebody else for the same URL is refused.
    const forged = structuredClone(listVc); forged.issuer = credentials[1].credentialSubject.id
    served.set(pathOf(listVc.id), forged)
    const lists3 = {}
    problems = await resolveStatusLists(credentials, lists3, { fetch: fetchAt })
    assert.equal(problems.length, 1)
    assert.match(problems[0], /not issued by the credential issuer/)

    // A missing list is a problem, not a silent pass.
    served.clear()
    const lists4 = {}
    problems = await resolveStatusLists(credentials, lists4, { fetch: fetchAt })
    assert.match(problems[0], /404/)
    assert.match(verifyCredential(credentials[0], { keyRegistry, statusLists: lists4 }).status, /not fetched/)
  } finally {
    server.close()
  }
})
