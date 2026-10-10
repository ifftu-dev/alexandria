const fs = require('node:fs');
const path = require('node:path');
const crypto = require('node:crypto');
const assert = require('node:assert/strict');
const [modules, vectorPath] = process.argv.slice(2);
if (!modules || !vectorPath) throw new Error('usage: node verify-personhood-vector.cjs NODE_MODULES VECTOR');
const { keccak256 } = require(path.resolve(modules, '@ethersproject/keccak256'));
const vector = JSON.parse(fs.readFileSync(vectorPath));
function canonical(value) {
  if (Array.isArray(value)) return '[' + value.map(canonical).join(',') + ']';
  if (value !== null && typeof value === 'object') return '{' + Object.keys(value).sort().map(key => JSON.stringify(key) + ':' + canonical(value[key])).join(',') + '}';
  return JSON.stringify(value);
}
const bytes = Buffer.from(canonical(vector.challenge.body));
const signal = crypto.createHash('sha256').update('alexandria/personhood-signal/v1\0').update(bytes).digest();
const result = (BigInt(keccak256(signal)) >> 3n).toString();
assert.equal(result, vector.signal_hash);
assert.equal(crypto.createHash('sha256').update(canonical(vector.policy)).digest('hex'), vector.challenge.body.policy_digest);
const key = crypto.createPublicKey({ key: Buffer.concat([Buffer.from('302a300506032b6570032100', 'hex'), Buffer.from(vector.policy.verifier_public_key, 'hex')]), format: 'der', type: 'spki' });
assert(crypto.verify(null, Buffer.concat([Buffer.from('alexandria/personhood-challenge/v1\0'), bytes]), key, Buffer.from(vector.challenge.signature, 'hex')));
console.log(JSON.stringify({ jcs_policy_digest: true, sdk_signal_hash: true, ed25519_challenge_signature: true }));
