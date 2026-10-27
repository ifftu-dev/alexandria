# User guide: profiles, onboarding and your data

Status: live
Updated: 2026-10-27

How to set up Alexandria for the first time, share a device between several
people, and find or remove your data. How profiles are isolated internally is
in [multi-user profiles](multi-user-profiles.md).

> Alexandria is alpha software. Do not use it with real credentials, real
> funds or sensitive data.

## First-time onboarding

1. Launch the app. You see the onboarding screen.
2. Everyone is a **learner**. Add **Instructor** and/or **Parent/Guardian** if
   they apply; you can change this later in Settings.
3. Give a **birthdate**. It stays on the device and is never published. If you
   are under 18 you will be asked to invite a guardian at the end.
4. Pick a profile name / username. It is shown on the profile picker and can
   be renamed.
5. Create a password. It encrypts this profile's vault.
6. A 24-word BIP-39 recovery phrase is generated (CIP-1852 derivation), and
   payment and stake addresses are derived (preprod testnet).
7. **Back up the recovery phrase.** It is the identity and wallet for this
   profile.
8. Learners then set a **goal** (an exam, a curriculum, a job role or a job
   description) that resolves to a target skill graph, and can upload a
   **resume or transcript** to bootstrap the skills they already have.
9. The P2P node starts and connects to the network.

What happens next depends on your roles:

| Role | Where you land |
|---|---|
| Minor (under 18) | A holding screen with a single-use invite code. A parent installs Alexandria on **their own device**, creates a Parent profile and enters the code. When they accept (both devices online), your profile unlocks automatically |
| Instructor | Instructor mode, with the course composer and dashboards. Toggle learner/instructor mode with `Cmd/Ctrl + Shift + M` |
| Parent/Guardian | The guardian dashboard, showing each linked child's activity |

Later launches open the **profile picker**: an avatar grid of every profile on
the device. Pick one and enter its password.

## Switching users

- **Avatar menu** (top right) → **Switch user** locks the current profile and
  returns to the picker.
- **Keyboard:** `Cmd/Ctrl + Shift + U` from anywhere.
- **Add another profile** from the picker's **Add user** tile. It runs
  onboarding again with a new name and password.

Each profile is fully isolated: its own vault, encrypted database, content
cache, plugins and network identity. Switching shuts the current profile's
services down and starts the next one's; any live tutoring session ends.

To use the same identity on another device, restore the recovery phrase into a
new profile on that device. It appears to the network as a separate peer.

## Where your data is stored

| OS | App data directory |
|---|---|
| macOS | `~/Library/Application Support/org.alexandria.node/` |
| Linux | `~/.local/share/org.alexandria.node/` (or `$XDG_DATA_HOME/org.alexandria.node/`) |
| Windows | `%APPDATA%\org.alexandria.node\` |

`alexandria path` prints the active profile's directory on any platform, and
`alexandria --json path` adds the app data root (see the
[CLI](../cli/README.md#diagnostics-and-cleanup)).

Inside it, every profile owns its own tree:

| File or directory | Holds |
|---|---|
| `profiles_index.json` | The public list the picker reads before any vault is unlocked: network id, display names, avatars, colours, timestamps. **No keys, DIDs or stake addresses.** An index from an older format or a different network is refused |
| `profiles/<uuid>/vault/` | The profile's encrypted vault (Stronghold on desktop, AES-256-GCM + Argon2id on mobile) |
| `profiles/<uuid>/alexandria.db` | The profile's SQLCipher database, keyed from its password |
| `profiles/<uuid>/iroh/` | Content-addressed blob store (course content, profiles) and node secret |
| `profiles/<uuid>/plugins/` | Installed plugin bundles |
| `profiles/<uuid>/videocache/` | Materialised video files for playback |

## Starting over

Delete one profile from the picker. To wipe **every** profile on the device:

```bash
alexandria db reset --force
```

or delete the app data directory above by hand.

### Older installs

- **Single-vault layout.** The pre-profile layout is not supported and is
  never migrated. The app leaves legacy top-level `alexandria.db`,
  `stronghold/` or `vault/`, `iroh/`, `plugins/` and `videocache/` entries
  untouched, reports them, and starts onboarding for a fresh profile. Move
  them aside once you have exported anything you need.
- **Unencrypted database.** The app refuses to open an unencrypted legacy
  `alexandria.db` and shows an error rather than touching it. Move that file
  aside (or delete it) and run onboarding again.
