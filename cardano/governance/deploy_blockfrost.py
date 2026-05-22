#!/usr/bin/env python3
"""Node-free reference-script deployer for preprod.

Builds reference-script-output transactions with `cardano-cli ... build-raw`
(offline), signs them, and submits via the Blockfrost API — no local
cardano-node required. UTxOs + protocol params come from Blockfrost.

Env: BLOCKFROST_PROJECT_ID, DEPLOYER_SIGNING_KEY (.skey path), DEPLOYER_ADDRESS.
"""
import json
import os
import subprocess
import sys
import tempfile
import time
import urllib.request

BF = "https://cardano-preprod.blockfrost.io/api/v0"
PID = os.environ["BLOCKFROST_PROJECT_ID"]
SKEY = os.environ["DEPLOYER_SIGNING_KEY"]
ADDR = os.environ["DEPLOYER_ADDRESS"]
HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.join(HERE, "build", "deploy")
os.makedirs(OUT, exist_ok=True)
CPUB = 4310  # coins_per_utxo_size

# title in plutus.json -> short name -> script_refs.rs const
VALIDATORS = {
    "dao_minting.dao_minting.mint": "dao_minting",
    "dao_registry.dao_registry.spend": "dao_registry",
    "election.election.spend": "election",
    "proposal.proposal.spend": "proposal",
    "vote_minting.vote_minting.mint": "vote_minting",
    "reputation_minting.reputation_minting.mint": "reputation_minting",
    "soulbound.soulbound.spend": "soulbound",
    "completion.completion_minting.mint": "completion_minting",
    "challenge_escrow.challenge_escrow.spend": "challenge_escrow",
}

# Two batches, each funded by one pure-ADA UTxO, each under the 16 KB tx limit.
BATCHES = [
    ["proposal", "election", "dao_registry"],
    ["reputation_minting", "completion_minting", "soulbound",
     "vote_minting", "dao_minting", "challenge_escrow"],
]
FEE = 2_000_000  # flat 2 ADA — well above min fee for these sizes


def bf_get(path):
    req = urllib.request.Request(f"{BF}/{path}", headers={"project_id": PID})
    with urllib.request.urlopen(req) as r:
        return json.load(r)


def cbor_wrap_hex(compiled_hex: str) -> str:
    b = bytes.fromhex(compiled_hex)
    n = len(b)
    if n < 24:
        hdr = bytes([0x40 | n])
    elif n < 256:
        hdr = bytes([0x58, n])
    elif n < 65536:
        hdr = bytes([0x59, n >> 8, n & 0xFF])
    else:
        hdr = bytes([0x5A]) + n.to_bytes(4, "big")
    return (hdr + b).hex()


def extract_scripts():
    data = json.load(open(os.path.join(HERE, "plutus.json")))
    sizes, files = {}, {}
    done = set()
    for v in data["validators"]:
        t = v["title"]
        if t in VALIDATORS and VALIDATORS[t] not in done:
            name = VALIDATORS[t]
            done.add(name)
            wrapped = cbor_wrap_hex(v["compiledCode"])
            path = os.path.join(OUT, f"{name}.plutus")
            json.dump(
                {"type": "PlutusScriptV3",
                 "description": f"Alexandria governance: {name}",
                 "cborHex": wrapped},
                open(path, "w"))
            files[name] = path
            sizes[name] = len(wrapped) // 2
    return sizes, files


def min_utxo(script_bytes: int) -> int:
    # (output overhead + script bytes) * coinsPerUtxoByte, + 2 ADA buffer,
    # rounded up to whole ADA.
    base = (script_bytes + 220) * CPUB
    return ((base // 1_000_000) + 1) * 1_000_000 + 2_000_000


def run(args):
    p = subprocess.run(args, capture_output=True, text=True)
    if p.returncode != 0:
        raise RuntimeError(f"{' '.join(args[:3])}...: {p.stderr.strip()}")
    return p.stdout.strip()


def deploy_batch(names, utxo, sizes, files):
    txin = f"{utxo['tx_hash']}#{utxo['tx_index']}"
    input_lov = int(next(a["quantity"] for a in utxo["amount"] if a["unit"] == "lovelace"))
    locks = {n: min_utxo(sizes[n]) for n in names}
    total_lock = sum(locks.values())
    change = input_lov - total_lock - FEE
    if change < 1_000_000:
        raise RuntimeError(f"insufficient: input {input_lov} < locks {total_lock} + fee")

    body = os.path.join(OUT, "_body.json")
    signed = os.path.join(OUT, "_signed.json")
    args = ["cardano-cli", "conway", "transaction", "build-raw", "--tx-in", txin]
    for n in names:
        args += ["--tx-out", f"{ADDR}+{locks[n]}",
                 "--tx-out-reference-script-file", files[n]]
    args += ["--tx-out", f"{ADDR}+{change}", "--fee", str(FEE), "--out-file", body]
    run(args)
    run(["cardano-cli", "conway", "transaction", "sign", "--tx-body-file", body,
         "--signing-key-file", SKEY, "--testnet-magic", "1", "--out-file", signed])
    txid_out = run(["cardano-cli", "conway", "transaction", "txid", "--tx-file", signed])
    # Newer cardano-cli emits JSON {"txhash": "..."}; older emits bare hex.
    txid = json.loads(txid_out)["txhash"] if txid_out.lstrip().startswith("{") else txid_out

    cbor_hex = json.load(open(signed))["cborHex"]
    if os.environ.get("DRY_RUN"):
        print(f"  [dry-run] built+signed {txid} ({len(cbor_hex)//2}B), lock={total_lock/1e6:.1f} change={change/1e6:.1f} — NOT submitted")
        return {n: f"{txid}#{i}" for i, n in enumerate(names)}
    raw = os.path.join(OUT, "_signed.cbor")
    open(raw, "wb").write(bytes.fromhex(cbor_hex))
    req = urllib.request.Request(
        f"{BF}/tx/submit", data=open(raw, "rb").read(),
        headers={"project_id": PID, "Content-Type": "application/cbor"}, method="POST")
    try:
        with urllib.request.urlopen(req) as r:
            submitted = r.read().decode().strip().strip('"')
    except urllib.error.HTTPError as e:
        raise RuntimeError(f"submit failed: {e.read().decode()}")
    assert submitted == txid, f"txid mismatch {submitted} != {txid}"
    return {n: f"{txid}#{i}" for i, n in enumerate(names)}


def main():
    print(f"Deployer: {ADDR}")
    sizes, files = extract_scripts()
    utxos = [u for u in bf_get(f"addresses/{ADDR}/utxos?count=100")
             if all(a["unit"] == "lovelace" for a in u["amount"])]
    utxos.sort(key=lambda u: -int(u["amount"][0]["quantity"]))
    if len(utxos) < len(BATCHES):
        sys.exit(f"need {len(BATCHES)} pure-ADA UTxOs, have {len(utxos)}")

    results = {}
    for i, names in enumerate(BATCHES):
        print(f"\n--- Batch {i+1}: {', '.join(names)} ---")
        r = deploy_batch(names, utxos[i], sizes, files)
        results.update(r)
        for n, ref in r.items():
            print(f"  {n}: {ref}")

    json.dump(results, open(os.path.join(OUT, "deployment_results.json"), "w"), indent=2)
    print("\n=== REF UTXOS (paste into script_refs.rs) ===")
    for n, ref in results.items():
        h, idx = ref.split("#")
        print(f'{n}: ("{h}", {idx})')


if __name__ == "__main__":
    main()
