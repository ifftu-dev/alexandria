const fs = require('node:fs');
const path = require('node:path');
const assert = require('node:assert/strict');

async function main() {
  const [moduleDir, vkeyPath, ...prefixes] = process.argv.slice(2);
  if (!moduleDir || !vkeyPath || prefixes.length === 0) {
    throw new Error('usage: node verify.cjs NODE_MODULES VKEY OUTPUT_PREFIX...');
  }
  const snarkjs = require(path.resolve(moduleDir, 'snarkjs'));
  const vkey = JSON.parse(fs.readFileSync(vkeyPath));
  const results = [];
  let referenceSignals;
  for (const prefix of prefixes) {
    const proof = JSON.parse(fs.readFileSync(`${prefix}.proof.json`));
    const signals = JSON.parse(fs.readFileSync(`${prefix}.public.json`));
    assert.equal(await snarkjs.groth16.verify(vkey, signals, proof), true);
    if (referenceSignals) assert.deepEqual(signals, referenceSignals);
    referenceSignals = signals;
    for (let i = 0; i < signals.length; i++) {
      const tampered = [...signals];
      tampered[i] = (BigInt(tampered[i]) + 1n).toString();
      assert.equal(await snarkjs.groth16.verify(vkey, tampered, proof), false);
    }
    results.push({ prefix, snarkjs_verified: true, tampered_signals_rejected: signals.length });
  }
  console.log(JSON.stringify({ snarkjs: require(path.resolve(moduleDir, 'snarkjs/package.json')).version, results }, null, 2));
}

main().then(() => process.exit(0)).catch(error => {
  console.error(error);
  process.exit(1);
});
