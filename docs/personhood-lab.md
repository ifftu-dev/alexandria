# Personhood Lab

Personhood Lab is an opt-in Android debug experiment that generates and verifies
a proof from a bundled synthetic fixture. It does not accept identity documents,
issue credentials, establish uniqueness, or change permissions. An unlocked
profile can also bind a fresh synthetic proof to its account and save a private
diagnostic receipt. Production issuer trust and real document input remain
disabled.

The panel appears on the onboarding welcome screen and in Settings → Advanced.
No profile is required for the onboarding test. Calls use the normal local IPC
bridge; `personhood_lab_status` and `personhood_lab_action` deliberately do not
require a profile. All four `personhood_receipt_*` commands require the current
unlocked profile session.

## Private synthetic receipts

With the test key cached, open Settings → Advanced and select **Create private
synthetic receipt**. The app creates a signed 120-second challenge, runs the
native synthetic prover, independently verifies the BN254 Groth16 proof in Rust,
and atomically consumes the challenge while storing a signed local receipt.
The UI lists the latest 20 receipts for the unlocked account and network.

The challenge binds the account DID and current Ed25519 key, network, purpose,
local verifier, policy digest, circuit, random challenge nonce, random session
nonce, and issue/expiry times. Its JCS body is hashed with SHA-256 and then the
pinned SDK's Keccak-256/right-shift convention. The native worker receives only
the expected signal hash and nullifier seed; it still reads the bundled test
fixture. The account signs the exact challenge, proof, and nine public signals.
The backend selects the pinned verification key and policy independently.

The local verifier key is domain-separated from the account key using HKDF.
This is a **synthetic diagnostic**, not an externally trusted attestation of
personhood. Its clock and database belong to the device owner. Only the bundled
test issuer and exact historical fixture timestamp are accepted. The synthetic
policy permits that fixture for ten years from its timestamp; it is not the
proposed seven-day freshness policy for a future real-input pilot. Receipt
validity is at most 24 hours and is also bounded by document and policy expiry.
All four optional attribute outputs must remain zero.

Cancellation, leaving the panel, backgrounding, and profile locking stop work.
Acceptance rechecks expiry, the current key registry, pending challenge state,
and the profile session. A session admission gate protects the final commit
against locking. An accepted retry in the same unlock session returns the
existing receipt. A changed submission cannot consume the same challenge again;
old unlock sessions and other accounts cannot reuse it. Reopening the same
profile can list its existing receipts.

Migration 3 adds `personhood_private_challenges` to the encrypted profile DB.
It is excluded from cross-device sync and gossip and writes no credentials,
identity fields, reputation, or permission state. The row stores the signed
challenge, consumption state, submission digest, and signed receipt. Raw proofs,
public-signal arrays, witnesses, and synthetic input files are not retained for
receipt runs. The separate benchmark still retains its successful output.
The developer profile is limited to five preparations per minute and 100 stored
receipts; cancelled/expired non-receipt rows are pruned on later preparations.
Receipts are retained until the profile is deleted. Removing the test key does
not delete the encrypted receipt history.

The implementation is in `crates/alexandria-personhood`, the profile-scoped
`commands/personhood_receipts.rs`, and the existing Android lab worker bridge.
No production issuer roots, real-input parser, network receipt service, or
account privilege integration are enabled.

## Build an isolated Android app

Run from the `alexandria/` repository. Prerequisites are the repository's Rust
toolchain, the `aarch64-linux-android` Rust target, Node dependencies, Git, CMake,
Ninja, Android SDK/NDK, cargo-tauri, and Java 21. The tested Mac used NDK
26.3.11579264 with Android API 28 and the installed Homebrew OpenJDK 21. Java 25
failed during Gradle configuration in this environment.

```sh
export JAVA_HOME=/opt/homebrew/opt/openjdk@21/libexec/openjdk.jdk/Contents/Home
export ANDROID_HOME="$HOME/Library/Android/sdk"
export NDK_HOME="$ANDROID_HOME/ndk/26.3.11579264"
rustup target add aarch64-linux-android
npm ci
python3 scripts/prepare-personhood-lab.py --ndk "$NDK_HOME" --check
export PERSONHOOD_LAB_BUNDLE="$PWD/target/personhood-lab"
./scripts/android-build.sh --debug --apk --features personhood-lab
```

The preparation script downloads and checks the circuit WASM and verification
key, builds the ARM64 native worker, and stages its libraries and synthetic
assets. It does not download the 612 MB proving key. `bundle.json` records staged
file hashes. An existing artifact directory can be supplied with `--artifacts`;
its files are still checked against the pinned hashes.

Install `src-tauri/gen/android/app/build/outputs/apk/universal/debug/app-universal-debug.apk`
with `adb -s "$DEVICE" install -r <apk>`, where `DEVICE` is the serial from
`adb devices -l`. The app is labelled **Alexandria Lab**, with application ID
`org.alexandria.node.personhoodlab`. Its storage is separate from ordinary
Alexandria and from the earlier standalone `org.alexandria.personhoodlab` app.

The Rust entry points require Android, debug assertions, and the explicit
`personhood-lab` Cargo feature. The Android controller additionally requires a
debug build, the bundle opt-in, and a packaged worker. Release source sets do not
include the worker or fixture. Without these gates the UI remains hidden and
mutation commands refuse to run. Unset `PERSONHOOD_LAB_BUNDLE` when returning to
ordinary builds.

## Download, run, cancel, and remove

1. Open Alexandria Lab and leave the lab panel visible.
2. Select **Download test key · 612 MB**. This is an explicit network download.
3. Select **Run synthetic test** once the key has been checked.
4. Use **Cancel** to stop work or **Remove test key** while idle to delete the
   lab's key, partial download, and synthetic run files.

The key is 612,082,146 bytes (612.1 MB decimal, approximately 583.7 MiB).
Downloads stream to app-private storage, retain partial bytes on cancellation,
and resume with a checked HTTP range. A server returning a full response causes
a clean restart. Redirects, inconsistent ranges, unexpected lengths, and
checksum mismatches are rejected. The full SHA-256 is checked before the key is
promoted to the cache and before every proof. Downloading requires room for the
remaining bytes plus a 64 MiB reserve.

The foreground app keeps the screen awake during work. Backgrounding the app,
leaving the panel, locking a profile, or receiving a handled memory warning
cancels the job. A 500 ms memory poll also cancels proving if Android reports low
memory or less than 1.5 GiB available. The prover runs in a child process that can
be forcibly terminated; Android parent-death handling kills it if the app dies.
Failed and cancelled proof output is removed. Successful synthetic output stays
in `files/personhood-lab/run/` for developer inspection until the next run or
cache removal. App restart retains the key but does not automatically restart
work. The controller has the app's network permission; it is not a network
sandbox.

## Pinned inputs and native implementation

`tools/personhood-prover/artifact-manifest.json` records URLs, sizes, and SHA-256
digests. The circuit, proving key, and verification key are the published
Anon Aadhaar v2.0.0 artifacts:

| File | SHA-256 |
| --- | --- |
| `aadhaar-verifier.wasm` | `4894923663fafda95beb70abca4112ea97a62058c9a46419a274f04236f10d4f` |
| `circuit_final.zkey` | `0d443ea85279b0370b53202f5e768ca9098cfcb5bb14f8ca8323d2266822c492` |
| `vkey.json` | `40f2ea24b56ffe2b6e6e3578053cbda15e773cb560697bac883a404077fcf177` |

The worker uses exact `rust-witness = 0.1.6` and `rust-rapidsnark = 0.1.4`
dependencies with a committed Cargo lockfile. Its build script pins w2c2 to
`9de3c2be5a4ed8ef5fdbd536e445120594fb8530` and initializes that revision's pinned
submodules before transpiling the selected WASM. This changes the witness and
proving execution path, not the circuit or the published proving key.

The bundled `synthetic-input.json` was prepared from the upstream test fixture
at revision `4dad918761cfb1d7d5ed9918dcd796d0cb23ae82`,
`packages/circuits/assets/test.json`, using the test certificate recorded in the
artifact manifest. It uses fixed test nullifier seed 1234 and signal 12345, with
attribute disclosure disabled. These values provide no production account or
replay binding.

## Verification

The download tests compile the actual production Kotlin transfer implementation
on the JVM without building Tauri or installing an Android SDK:

```sh
JAVA_HOME=/opt/homebrew/opt/openjdk@21/libexec/openjdk.jdk/Contents/Home \
  bash src-tauri/gen/android/gradlew -p tools/personhood-tests test --no-daemon
```

`.github/workflows/personhood-lab.yml` runs these tests and the pinned ARM64
worker build, formatting, and Clippy on relevant pushes and pull requests. It
downloads the small circuit and verification-key artifacts, not the proving key.
The main CI workflow also enables `personhood-lab` in Rust lint/tests and the
Android compile check. Full APK and physical-device lifecycle tests remain
separate from these hosted CI checks.

```sh
cargo fmt --all --check
cargo test -p alexandria-node commands::personhood_lab --lib
cargo clippy -p alexandria-node --all-targets --features personhood-lab -- -D warnings
npx vue-tsc -b --noEmit
npm test -- src/composables/usePersonhoodLab.test.ts src/components/settings/PersonhoodLabPanel.test.ts
npm run check:tauri-commands
npm run i18n:no-raw-text
```

The preparation command's `--check` additionally runs worker formatting and
Android Clippy with warnings denied. After a full Android build, Kotlin tests
and the device test APK can be built from `src-tauri/gen/android/` without
rebuilding Rust:

```sh
./gradlew :app:testUniversalDebugUnitTest :app:assembleUniversalDebugAndroidTest \
  -PabiList=arm64-v8a -ParchList=arm64 -PtargetList=aarch64 \
  -x :app:rustBuildUniversalDebug -x :app:rustBuildArm64Debug
adb -s "$DEVICE" install -r app/build/outputs/apk/androidTest/universal/debug/app-universal-debug-androidTest.apk
adb -s "$DEVICE" shell am instrument -w \
  -e waitForActivitiesToComplete false \
  -e class org.alexandria.node.PersonhoodLabDeviceTest \
  org.alexandria.node.personhoodlab.test/androidx.test.runner.AndroidJUnitRunner
```

Keep `JAVA_HOME` and `PERSONHOOD_LAB_BUNDLE` from the build environment exported.
The device test deletes only this lab's cache, downloads the public key, checks
cancel/resume across Activity recreation, cancels proving explicitly and on
backgrounding, injects a memory-warning callback, then generates two proofs.
It calls the native controller directly; frontend unit tests cover UI lifecycle
and action ordering. Evidence is written to
`files/personhood-lab-device-tests.json` and the last proof to
`files/personhood-lab/run/proof.{results,proof,public}.json`, accessible with
`adb exec-out run-as org.alexandria.node.personhoodlab cat <path>`.

The runner flag is required because closing Tauri's final Activity exits the
host process before instrumentation can report its result. The suite leaves the
Activity stopped and allows the runner to report before Android ends the
instrumented process. An initial run completed all eight recorded checks but
reported `Process crashed` during final Activity closure, with a native
destroyed-mutex diagnostic; Android recorded process exit status 0. This suite
does not claim to validate native runtime shutdown after final Activity closure.

The milestone review reproduced the same exit and native diagnostic with an
idle-Activity probe that invoked **zero** lab actions. Android again recorded
`EXIT_SELF`, status 0; no prover child remained. The vendored Tao Android event
loop explicitly calls `std::process::exit` after `run_return`
(`patches/tao/src/platform_impl/android/mod.rs`), and final Activity destruction
dispatches a window-destroyed event (`ndk_glue.rs`). This explains why an in-process
instrumentation runner loses its host. The mutex diagnostic's precise native
origin remains unresolved; the probe shows that proving is not required to
trigger it. The test harness adjustment is not a fix for that diagnostic.

Back at the repository root, independently verify the exported proof and reject
each mutated public signal:

```sh
npm install --prefix target/personhood-verifier --no-save snarkjs@0.7.6
node scripts/verify-personhood-lab.cjs \
  target/personhood-verifier/node_modules \
  "$PERSONHOOD_LAB_BUNDLE/assets/personhood-lab/vkey.json" \
  target/personhood-lab-results/integrated
```

The last argument is the exported file prefix: it expects `integrated.proof.json`
and `integrated.public.json`. Exporting a fixture proof does not export identity
documents; only the bundled synthetic input is supported.

### Results and limits

Host validation passed: two Rust gate/action tests, six frontend tests, five
Kotlin download-response tests, strict TypeScript, Rust formatting, host and
worker Clippy, command registration, and the raw-text guard. The ARM64 Android
debug APK built and installed successfully.

On 2026-09-30, the integrated app ran on a OnePlus 11 5G (CPH2447, 16 GB RAM,
user-reported OxygenOS 16.0.5) over wireless ADB. The first controlled run retained
8,912,977 partial download bytes and completed resume plus checksum in 58.0 s.
Both proofs verified natively:

| Run | Total including key check | Native worker | Worker peak RSS |
| --- | --- | --- | --- |
| 1 | 6.301 s | 5.882 s | 802.4 MiB |
| 2 | 6.339 s | 5.919 s | 804.9 MiB |

The exported second proof independently verified with snarkjs 0.7.6, and all nine
single-public-signal mutations were rejected. These memory figures measure the
prover child process, not the combined Android app and WebView footprint.

After the runner adjustment, the complete device suite passed in 86.041 s
(`OK (1 test)`, eight recorded checks). Its two successful proofs took 6.342 s and
6.356 s including key checks, with 804.9–805.0 MiB worker peak RSS. The exported
final proof also passed independent snarkjs verification and all nine signal
mutation checks.

A further run was started by tapping **Run synthetic test** in the actual Vue
panel after relaunch. The panel showed successful verification in 5.97 s native
time, 6.4 s total, and 802 MiB worker peak RSS. Its exported proof independently
verified, with all nine mutated signals rejected. Local evidence and screenshots
are retained under the ignored `target/personhood-lab-results/` directory.

The subsequent milestone review passed all 198 frontend tests and expanded the
Kotlin suite to seven cases, adding wrong-content/correct-size key rejection and
cancellation during checksum verification. The rebuilt ARM64 worker passed
formatting and Clippy and produced the same staged bundle hashes as the device-
tested build. The transpiler build now records its pinned revision and discards
stale cached builds on a revision change; the preparation script fixes its target
directory and removes inherited `RAPIDSNARK_LIB_DIR` overrides.

Workspace Clippy with `--all-targets --features personhood-lab -- -D warnings`
passed. The full Rust workspace rerun passed with **1,712 tests passed and four
ignored**, using:

```sh
cargo test --workspace --features personhood-lab -- --test-threads=4
```

The initial default-concurrency run failed the existing
`content_store::resolver::tests::resolve_blake3_fetches_from_peer_before_url`
test with `NotFound`. That test passed in isolation and in the complete rerun;
its code was not changed. Both edited workflow files passed `actionlint`. Hosted
GitHub Actions execution is still pending a push; these are local validation
results.

The real-input design is described in
[Personhood issuer trust and account binding](personhood-trust-proposal.md).
Its synthetic account-binding and private-receipt subset is implemented above;
production issuer trust and real document input remain proposed and disabled.

This is a developer experiment, not a personhood credential implementation.
Testing a simulated memory callback does not establish survival under a real
Android low-memory kill. Results on a 16 GB phone do not establish suitability
for lower-memory phones. Real issuer trust, real document validation, production verification authority,
uniqueness policy, privacy review for real inputs, and a broader device matrix
remain outside this version. Account/session binding and one-use challenges are
implemented only for the synthetic diagnostic flow.

## Receipt protocol verification

```sh
cargo test -p alexandria-personhood
cargo test -p alexandria-node --features personhood-lab profile::scope
npm test -- src/composables/usePersonhoodReceipts.test.ts
```

The committed fixtures include a native challenge-bound proof and cross-language
vector. With the feasibility benchmark's pinned Node dependencies available:

```sh
node scripts/verify-personhood-vector.cjs "$NODE_MODULES" \
  crates/alexandria-personhood/tests/fixtures/bound.vector.json
node scripts/verify-personhood-lab.cjs "$NODE_MODULES" \
  crates/alexandria-personhood/assets/vkey.json \
  crates/alexandria-personhood/tests/fixtures/bound
```

The independent JavaScript check covers JCS policy digest, the SDK signal hash,
and Ed25519 challenge signature. Rust and snarkjs 0.7.6 both accept the native
proof and reject mutations to all nine public signals. Rust tests also cover
account/context substitution, strict field encodings, malformed points,
ambiguous JSON, expiry/revocation, cancellation, transaction rollback, and
idempotent consumption.

The Android receipt instrumentation suite creates two disposable synthetic
profiles and removes only those profiles during cleanup. Start with the lab key
cached and any existing profile locked. Pass the network ID from the embedded
network profile rather than choosing an arbitrary network:

```sh
adb -s "$PHONE" shell am instrument -w \
  -e waitForActivitiesToComplete false \
  -e networkId preprod \
  -e class org.alexandria.node.PersonhoodReceiptDeviceTest \
  org.alexandria.node.personhoodlab.test/androidx.test.runner.AndroidJUnitRunner
```

It exercises the real WebView → Tauri IPC → native prover → Rust verifier →
encrypted database path. The test checks unchanged account state, idempotent
retry, cancellation before start, persistence after unlock, stale session
rejection, lock during proving, and cross-profile isolation. It does not test
real identity documents or establish production trust.

Receipt implementation host checks on 2026-09-30:

- Full workspace run with `--features personhood-lab -- --test-threads=4`:
  1,718 passed, 4 ignored. Subsequent focused checks covered the final retry and
  input-validation changes: 7 protocol/storage tests and 4 app personhood tests.
- All 202 frontend tests, strict `vue-tsc -b --noEmit`, workspace formatting,
  workspace Clippy with `--all-targets --features personhood-lab -- -D warnings`,
  and a normal build without the lab feature passed.
- Command registration: 354 registered, 309 invoked, 45 allowlisted. All four
  receipt commands are profile-scoped. The generated schema check and i18n raw
  text check also passed.
- The Java 21 ARM64 debug APK and receipt instrumentation APK built successfully.

On the OnePlus 11 5G (CPH2447, 16 GB RAM, OxygenOS 16.0.5), the integrated
receipt instrumentation suite passed in 67.669 seconds. All eight checks listed
above passed, and the final assertion confirmed that the receipt proof/witness
working directory had been removed. The suite deleted its two disposable
profiles. The proving key remains cached. The initial attempt stopped at the
missing-key precondition; downloading and checksum-verifying the pinned key
allowed the full run to complete. Evidence is retained locally under the ignored
`target/personhood-lab-results/receipts/` directory.
