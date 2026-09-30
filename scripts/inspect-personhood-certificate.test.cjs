const assert = require('node:assert/strict');
const { test } = require('node:test');
const fs = require('node:fs');
const path = require('node:path');
const { X509Certificate, createHash } = require('node:crypto');
const { inspect, inspectWithHash } = require('./inspect-personhood-certificate.cjs');

const pem = fs.readFileSync(path.join(__dirname, '../crates/alexandria-personhood/tests/fixtures/testCertificate.pem'));
const der = new X509Certificate(pem).raw;

test('fixture matches the pinned public test certificate with its original CRLF encoding', () => {
  const manifest = JSON.parse(fs.readFileSync(path.join(__dirname, '../tools/personhood-prover/artifact-manifest.json')));
  const pin = manifest.find(item => item.name === 'testCertificate.pem');
  const original = Buffer.from(pem.toString().replace(/\n/g, '\r\n'));
  assert.equal(original.length, pin.bytes);
  assert.equal(createHash('sha256').update(original).digest('hex'), pin.sha256);
  const parsed = inspect(pem);
  assert.equal(parsed.modulus.length, 256);
  assert.equal(parsed.packed.length, 9);
  assert.deepEqual(inspect(der).packed, parsed.packed);
  assert.deepEqual(inspect(original).packed, parsed.packed);
});

test('inspection refuses oversized, malformed, concatenated and trailing data', () => {
  for (const bytes of [
    Buffer.alloc(0), Buffer.alloc(16385), Buffer.from('not a certificate'),
    Buffer.concat([pem, pem]), Buffer.concat([der, Buffer.from([0])]),
    Buffer.concat([pem, Buffer.from('ignored')]),
  ]) assert.throws(() => inspect(bytes));
  const nonAscii = Buffer.from(pem);
  nonAscii[40] |= 128;
  assert.throws(() => inspect(nonAscii), /ASCII/);
});

test('both Poseidon implementations reproduce the circuit fixture issuer hash', async () => {
  const modules = process.env.PERSONHOOD_NODE_MODULES;
  assert.ok(modules, 'Set PERSONHOOD_NODE_MODULES to a directory containing circomlibjs 0.1.7');
  const result = await inspectWithHash(pem, modules);
  assert.equal(result.circuit_key_hash, '15134874015316324267425466444584014077184337590635665158241104437045239495873');
  assert.equal(result.matches_synthetic_issuer, true);
  assert.equal(result.matches_legacy_sdk_issuer, false);
  assert.equal(result.trust_activated, false);
});
