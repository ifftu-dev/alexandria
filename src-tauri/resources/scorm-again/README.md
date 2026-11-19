# scorm-again (vendored)

Third-party code, not Alexandria's. These are unmodified build files of
[scorm-again](https://github.com/jcputney/scorm-again), the SCORM 1.2 and
SCORM 2004 run-time library, distributed under the MIT licence in `LICENSE`.

| | |
|---|---|
| Version | 3.5.0, published to npm on 2026-10-26 |
| Source | `github.com/jcputney/scorm-again`, tag `3.5.0`, commit `602a247ff4e55a6b6d06402e53b0a0bf816d8e9f` |
| Tarball | `https://registry.npmjs.org/scorm-again/-/scorm-again-3.5.0.tgz` |
| Tarball integrity | `sha512-zA+pEpj9CshatiqS6riAEhnEEktBkHrC6sk039uyP8B4n5qWcB7I3sSPrmiAYrWz+JlKuVLHcFlaTNa/3ofR/w==` |
| Provenance | npm publishes a build attestation for this version naming the tag and commit above |

| File here | File in the tarball | Global it defines |
|---|---|---|
| `scorm12.min.js` | `package/dist/scorm12.min.js` | `Scorm12API` |
| `scorm2004.min.js` | `package/dist/scorm2004.min.js` | `Scorm2004API` |
| `LICENSE` | `package/LICENSE` | |

`SHA256SUMS` pins all three. The two scripts are classic scripts, not modules:
each assigns its global through a top-level `this`, so load them with a plain
`<script src>`.

## Rules

- Nothing fetches this library at build or run time. The files here are what
  ships. They are meant to be embedded by the handler that serves package
  content, so `.gitattributes` keeps their bytes exact on every platform.
- Do not edit the files in place. If a fix cannot wait for upstream, keep it as
  a patch file in this directory, list it below with the upstream issue or pull
  request, and regenerate `SHA256SUMS`.
- Each script ends with a `sourceMappingURL` comment. The maps upstream ships
  are empty, so they are not vendored. A browser asks for them only with
  developer tools open.

## Checking the files

```sh
cd src-tauri/resources/scorm-again
shasum -a 256 -c SHA256SUMS
```

`tests/desktop-ui/scorm-runtime.spec.ts` makes the same check and runs a
SCORM 1.2 session against `scorm12.min.js` in Chromium and WebKit.

## Updating

1. Download the new tarball from the npm registry and check its integrity
   value against the registry metadata.
2. Replace the three files from the tarball, then update the table above and
   `SHA256SUMS` (`shasum -a 256 LICENSE scorm12.min.js scorm2004.min.js > SHA256SUMS`).
3. Run `npm run test:desktop-ui`.

## Patches

None.
