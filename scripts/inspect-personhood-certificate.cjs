const fs = require('node:fs');
const path = require('node:path');
const { X509Certificate, createHash } = require('node:crypto');

const SYNTHETIC_ISSUER = '15134874015316324267425466444584014077184337590635665158241104437045239495873';
const LEGACY_ISSUER = '18063425702624337643644061197836918910810808173893535653269228433734128853484';

function inspect(bytes) {
  if (bytes.length === 0 || bytes.length > 16384) throw new Error('Certificate size is outside the inspection limit');
  let der = bytes;
  if (bytes.subarray(0, 27).toString() === '-----BEGIN CERTIFICATE-----') {
    if (bytes.some(byte => byte > 127)) throw new Error('PEM must be ASCII');
    const pem = bytes.toString('ascii');
    const match = /^-----BEGIN CERTIFICATE-----\r?\n([A-Za-z0-9+/=\r\n]+)-----END CERTIFICATE-----\s*$/.exec(pem);
    if (!match) throw new Error('Expected exactly one PEM certificate');
    const body = match[1].replace(/[\r\n]/g, '');
    der = Buffer.from(body, 'base64');
    if (der.toString('base64') !== body) throw new Error('Invalid certificate base64');
  }
  const certificate = new X509Certificate(der);
  if (!certificate.raw.equals(der)) throw new Error('Trailing certificate data');
  if (certificate.publicKey.asymmetricKeyType !== 'rsa') throw new Error('Circuit requires an RSA key');
  const jwk = certificate.publicKey.export({ format: 'jwk' });
  const modulus = Buffer.from(jwk.n, 'base64url');
  const exponent = BigInt('0x' + Buffer.from(jwk.e, 'base64url').toString('hex'));
  if (modulus.length !== 256 || modulus[0] < 128 || exponent !== 65537n) {
    throw new Error('Circuit requires RSA-2048 with exponent 65537');
  }
  const value = BigInt('0x' + modulus.toString('hex'));
  const mask = (1n << 121n) - 1n;
  const limbs = Array.from({ length: 17 }, (_, index) => (value >> BigInt(index * 121)) & mask);
  const packed = Array.from({ length: 9 }, (_, index) => limbs[index * 2] + ((limbs[index * 2 + 1] ?? 0n) << 121n));
  return { certificate, modulus, packed };
}

async function inspectWithHash(bytes, modules) {
  const library = path.resolve(modules, 'circomlibjs');
  if (JSON.parse(fs.readFileSync(path.join(library, 'package.json'))).version !== '0.1.7') {
    throw new Error('Inspection requires circomlibjs 0.1.7');
  }
  const { buildPoseidon, buildPoseidonReference } = require(library);
  const { certificate, modulus, packed } = inspect(bytes);
  const fast = await buildPoseidon();
  const reference = await buildPoseidonReference();
  const circuitKeyHash = fast.F.toObject(fast(packed)).toString();
  if (reference.F.toObject(reference(packed)).toString() !== circuitKeyHash) {
    throw new Error('Poseidon implementations disagree');
  }
  return {
    certificate_sha256: createHash('sha256').update(certificate.raw).digest('hex'),
    subject: certificate.subject,
    issuer: certificate.issuer,
    valid_from: new Date(certificate.validFrom).toISOString(),
    valid_until: new Date(certificate.validTo).toISOString(),
    rsa_bits: 2048,
    rsa_exponent: 65537,
    modulus_hex: modulus.toString('hex'),
    circuit_key_hash: circuitKeyHash,
    matches_synthetic_issuer: circuitKeyHash === SYNTHETIC_ISSUER,
    matches_legacy_sdk_issuer: circuitKeyHash === LEGACY_ISSUER,
    trust_activated: false,
    checks_not_performed: ['certificate_chain', 'revocation_status', 'qr_signing_purpose', 'current_document_compatibility'],
  };
}

module.exports = { inspect, inspectWithHash };
if (require.main === module) {
  const [modules, ...certificates] = process.argv.slice(2);
  if (!modules || certificates.length === 0) {
    console.error('usage: node scripts/inspect-personhood-certificate.cjs NODE_MODULES CERTIFICATE...');
    process.exitCode = 1;
  } else {
    (async () => {
      const results = [];
      for (const file of certificates) {
        const descriptor = fs.openSync(file, 'r');
        let bytes;
        try {
          if (!fs.fstatSync(descriptor).isFile()) throw new Error('Expected a certificate file');
          bytes = Buffer.alloc(16385);
          const count = fs.readSync(descriptor, bytes, 0, bytes.length, 0);
          bytes = bytes.subarray(0, count);
        } finally {
          fs.closeSync(descriptor);
        }
        results.push({ file, ...await inspectWithHash(bytes, modules) });
      }
      console.log(JSON.stringify(results, null, 2));
    })().catch(error => { console.error(error.message); process.exitCode = 1; });
  }
}
