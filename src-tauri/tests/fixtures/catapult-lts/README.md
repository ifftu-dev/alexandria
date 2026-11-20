# CATAPULT LMS test suite: course structures

The `cmi5.xml` of each runtime package of the CATAPULT LMS Test Suite (the cmi5 project's published conformance packages), unmodified, from `github.com/adlnet/CATAPULT` at commit `806c0baaa0fa99c9cb4f398ad7eb21fc5461c6e4`, path `lts/pkg/src/<package>/cmi5.xml`. Apache License 2.0: see [LICENSE](LICENSE) and [NOTICE](NOTICE).

The host-seam conformance tests (`src-tauri/src/packages/attempts/cmi5/conformance.rs`) package each one with a stub `index.html`, import it through the app's import path, and replay over HTTP the requests that package's assignable unit (`lts/pkg/src/<package>/<package>.js`, built on cmi5.js 3) makes. The matrix is `src-tauri/src/packages/attempts/cmi5/conformance.md`.

The units' JavaScript is not committed or run: it needs CATAPULT's webpack build and a browser driving a Rust host. `101-one-thousand-aus` (remote units only) and the `20x` import packages are transcribed in `crates/alexandria-studio/tests/cmi5_import.rs`.
