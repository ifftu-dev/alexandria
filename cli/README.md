# `alexandria` — developer CLI

The `alexandria` CLI works on the same encrypted store the app uses. It is
deliberately **not** general dev tooling: `scripts/check.sh`, `npm run check`
and `cargo tauri` own building and linting. The CLI covers what nothing else
does: the vault and its credentials, role assessments, the mobile device
pickers, environment diagnostics, demo-genesis tooling and the Sentinel
synthetic-data generator.

```bash
cargo install --path cli    # from the repository root; installs `alexandria`
alexandria --help
```

## Global flags

Every command accepts three global flags:

| Flag | Purpose |
|------|---------|
| `--json` | Emit the result as JSON on stdout. Human output goes to stderr, so `alexandria --json … > out.json` captures exactly the result document. Failures emit `{"error": …}` on stderr. |
| `--password-file <PATH>` | Read the vault password from a file instead of prompting, for CI and scripted runs. |
| `--profile <ID\|NAME>` | Which profile's data to use: a profile id, an id prefix, or a display name. Defaults to the most recently unlocked profile, which is the one the app opens. |

Commands that touch the vault or database operate on one profile's data under
`profiles/<uuid>/`. `alexandria doctor` reports which profile was selected; an
unrecognised `--profile` lists the ones that exist. The app must have been
launched and a profile created (through onboarding) before vault or database
commands work.

## Commands

| Command | What it does |
|---|---|
| `tui` | Interactive terminal UI |
| `credentials` (alias `vc`) | List, issue, revoke, export and verify Verifiable Credentials |
| `vp` | Verify Verifiable Presentations |
| `role-assessment` (alias `ra`) | Organisations, role assessments and role credential issuance |
| `governance` | Create a single-operator demo genesis, or verify a public genesis offline |
| `run` | Build and run the app on iOS or Android, with a device picker |
| `db` | Database status, migrations and reset |
| `doctor` | Diagnose the project, app data, toolchain and mobile prerequisites |
| `path` | Print the selected profile's data directory |
| `clean` | Remove build artifacts and/or app data |
| `synth-sentinel` | Generate synthetic adversarial-prior data for the Sentinel paste classifier |
| `completions <SHELL>` | Print a shell completion script |

### Interactive UI

```bash
alexandria tui
```

Tabs for credentials, role assessments, organisations, the database,
verification, diagnostics and logs. Select a row and act on it rather than
copying a URN between commands. Press `?` for the key list on the active tab.

### Credentials

```bash
alexandria credentials list                           # every credential in the store
alexandria credentials list --subject did:key:z6Mk…   # filter by subject
alexandria credentials list --skill rust-basics       # filter by skill

alexandria credentials get urn:uuid:…                 # one credential as JSON
alexandria credentials issue --request req.json       # mint and sign (`-` reads stdin)
alexandria credentials import bundle.json             # verify and store

alexandria credentials revoke urn:uuid:… --reason "issued in error"
alexandria credentials suspend urn:uuid:… --until 2026-12-01T00:00:00Z [--reason …]
alexandria credentials reinstate urn:uuid:…

alexandria credentials export --out bundle.json       # §20.4 survivability bundle
alexandria credentials verify bundle.json             # offline; `-` reads stdin
alexandria credentials verify bundle.json --at 2026-01-01T00:00:00Z
```

- Revocation is permanent and written to the credential's status list.
  Suspension is reversible; prefer it when the outcome is not settled.
- `issue` reads a JSON `IssueCredentialRequest` instead of a wall of flags, so
  the full shape (credential type, claim, evidence refs, supersession,
  integrity binding and policy) stays expressible. It unlocks the vault for the
  issuer signing key, so a credential minted here is indistinguishable from one
  the app mints.
- `verify --at` checks the bundle as of that moment: expiry, open suspension
  windows, and which issuer key was current (so a credential signed before a
  key rotation still verifies at a time that key was valid).

### Presentations

```bash
alexandria vp verify envelope.json --audience acme-corp
```

The audience is required: a presentation built for one verifier is rejected at
another. The verdict is a result, not a command failure; branch on `accepted`
in the JSON form.

### Role assessments

```bash
alexandria role-assessment org create "Acme Corp" --owner addr1… [--did did:key:…]
alexandria role-assessment org list [--owner addr1…]
alexandria role-assessment org get <org-id>

alexandria role-assessment create --request role.json   # `-` reads stdin
alexandria role-assessment list [--org <org-id>]
alexandria role-assessment get <id>
alexandria role-assessment status <id> published        # draft | published | archived
alexandria role-assessment issue <id> --subject did:key:z6Mk… --session <integrity-session-id>
```

The headless half of the role-assessment surface, for callers with no Tauri
window. `issue` binds the role credential to the integrity session the
assessment was sat under, so the issuance is auditable back to that run.

### Governance (demo genesis)

```bash
alexandria governance create-demo-genesis \
  --network-profile src-tauri/resources/networks/preprod.json \
  --issuer did:key:… --private-dir <new-dir-outside-repo> --out genesis.json
alexandria governance verify-genesis genesis.json [--expected-dao-id <id>]
```

`create-demo-genesis` generates seven founder key sets controlled by **one**
operator and writes raw secret keys into `--private-dir`, which must be a new
directory outside the repository. Never use it to establish independently
governed production trust. `verify-genesis` checks every founder signature and
the canonical bytes without touching secrets. Neither needs a profile. See
[default network genesis](../docs/default-network-genesis.md).

### Running on mobile

```bash
alexandria run ios                            # pick from available simulators
alexandria run ios --device "iPhone 17 Pro"   # skip the picker
alexandria run ios --physical                 # target a connected iPhone
alexandria run ios --open                     # open in Xcode instead
alexandria run ios --release

alexandria run android                        # pick from available AVDs and devices
alexandria run android --device "Pixel_9_Pro"
alexandria run android --open                 # open in Android Studio
```

Without `--physical`, the iOS picker lists simulators only and installs with
`xcrun simctl`, bypassing Tauri's device auto-detection (which overrides
simulator targeting when a phone is plugged in). Android runs set up the NDK
cross-compilation environment automatically. For desktop, use
`cargo tauri dev` (see [building](../docs/building.md)).

### Database and data

```bash
alexandria db status        # tables, row counts, migration version, size
alexandria db migrate       # run pending schema migrations
alexandria db reset --force # delete ALL app data on this device
```

Each profile's database is SQLCipher-encrypted with a key derived from that
profile's vault password (Argon2id + HKDF-SHA256); the CLI prompts for it, or
reads `--password-file`. The CLI applies the app's own migration list.
`db reset --force` removes the whole app data directory, every profile
included; use the profile picker's per-profile delete to remove one user.

### Diagnostics and cleanup

```bash
alexandria doctor              # project, app data, toolchain, mobile prerequisites
alexandria doctor --no-mobile  # skip the Android/iOS checks

alexandria path                # the selected profile's data directory
alexandria --json path         # also the app data root and profile label

alexandria clean build         # remove target/, dist/, .vite cache
alexandria clean data --force  # remove app data
alexandria clean all --force   # both
```

`doctor` always exits 0: a missing Android NDK is a fact to report, not a
failure of the command. Branch on `ok` in the JSON form.

The CLI uses the OS app-data directory (see
[where your data is stored](../docs/user-guide.md#where-your-data-is-stored));
it does not read the app's `ALEXANDRIA_DATA_DIR` override.

### Sentinel synthetic data

```bash
alexandria synth-sentinel generate --label <class> --out blob.json [--count 10000] [--seed 1] [--pretty]
alexandria synth-sentinel generate-all --out-dir tools/sentinel-train/priors [--seed-base 1]
alexandria synth-sentinel generate-holdout --out-dir tools/sentinel-train/holdout
alexandria synth-sentinel stats --input blob.json
```

Deterministic per seed. The blob schema is in
[Sentinel → Synthetic training blobs](../docs/sentinel.md#synthetic-training-blobs);
see also the [Sentinel runbook](../docs/sentinel-runbook.md) and
[`tools/sentinel-train`](../tools/sentinel-train/README.md).

### Shell completions

```bash
alexandria completions zsh > ~/.zfunc/_alexandria
alexandria completions bash > /etc/bash_completion.d/alexandria
```

Supports bash, zsh, fish, elvish and powershell.
