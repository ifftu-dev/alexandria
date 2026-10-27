# Alexandria docs

Start with the [project README](../README.md) for what Alexandria is and the quick start. Every page below opens with a `Status:` line; [CONTRIBUTING.md](../CONTRIBUTING.md#documentation) explains the values.

## Use it

| Doc | What it covers |
|---|---|
| [user-guide.md](user-guide.md) | Onboarding, switching users, where your data lives |
| [jev-integration.md](jev-integration.md) | Turning on the optional AI checks |
| [demo/](demo/setup.md) | The assessment → credential → Cloud demo: [setup](demo/setup.md), [runbook](demo/runbook.md), [presenter notes](demo/presenter-notes.md) |

## Build and release it

| Doc | What it covers |
|---|---|
| [building.md](building.md) | Requirements, prerequisites, desktop dev and builds, optional Cardano setup |
| [building-mobile.md](building-mobile.md) | iOS and Android builds |
| [../cli/README.md](../cli/README.md) | The `alexandria` developer CLI |
| [releasing.md](releasing.md) | CI/CD workflows, cutting a release, required secrets |
| [desktop-ui-validation.md](desktop-ui-validation.md) | Automated and native-package checks for desktop UI changes |
| [project-structure.md](project-structure.md) | Where the code lives |

## Understand it

| Doc | What it covers |
|---|---|
| [vision.md](vision.md) | Why Alexandria exists |
| [architecture.md](architecture.md) | How the pieces fit together |
| [protocol-specification.md](protocol-specification.md) | Wire protocols, gossip topics, credential formats |
| [skills-and-reputation.md](skills-and-reputation.md) | Taxonomy, assessments, credentials, reputation |
| [network-profiles.md](network-profiles.md) | Bundled network profiles and trust roots |
| [multi-user-profiles.md](multi-user-profiles.md) | Several users on one device |
| [settings.md](settings.md) | Synced and device-local settings |
| [database-schema.md](database-schema.md) | Per-profile SQLite tables |
| [plugins.md](plugins.md) | Community plugins and graders |
| [sentinel.md](sentinel.md) | Assessment and interview integrity |
| [interview-assistant.md](interview-assistant.md) | In-app interview support |
| [enterprise-boundary.md](enterprise-boundary.md) | How the commercial Enterprise Edition sits outside this AGPL app |
| [personhood-lab.md](personhood-lab.md) | Experimental proof-of-personhood |
| [username-registry.md](username-registry.md) | DHT usernames and relay receipts |
| [stake-pubkey-registry.md](stake-pubkey-registry.md) | Stake-address → public-key registry |
| [deeplink/README.md](deeplink/README.md) | Shared links that open straight into the app |

## Operate it

| Doc | What it covers |
|---|---|
| [default-network-genesis.md](default-network-genesis.md) | The preprod demo's single-operator governance genesis |
| [stake-pubkey-registry-runbook.md](stake-pubkey-registry-runbook.md) | Pre-launch registry ceremony |
| [sentinel-runbook.md](sentinel-runbook.md) | Retraining or rolling back the bundled paste classifier |

## Specs and proposals

Open work, tracked as described in [agents/issue-tracker.md](agents/issue-tracker.md).

| Doc | What it covers |
|---|---|
| [ai-chat-spec.md](ai-chat-spec.md) | AI chat and tutoring |
| [personhood-trust-proposal.md](personhood-trust-proposal.md) | Personhood issuer trust and account binding |
| [sentinel-federation.md](sentinel-federation.md) | Federated Sentinel threat model and privacy budget |
| [upstream/](upstream/) | Bug reports drafted for upstream crates, not yet filed |

## Decisions and audits

- [adr/](adr/README.md): architecture decisions, including the cross-repo ones.
- [audits/](audits/): [security](audits/security-2026.md), [performance](audits/performance-2026.md), [monorepo security index](audits/monorepo-security-2026-08.md), [docs](audits/docs-2026-10.md).

## Removed docs

Finished plans were deleted on 2026-10-27; their decisions live in the ADRs. Read any of them with `git show <commit>:docs/<name>.md`.

| Doc | Last commit | Now covered by |
|---|---|---|
| `assessment-remediation-plan.md` | `bc776f72` | ADRs 0001–0003 |
| `rebuild-execution-plan.md` | `bd8a9714` | ADRs 0001–0004 |
| `instructor-studio-mcp-plan.md` | `254e2dc9` | `crates/alexandria-mcp` |
| `vc-migration.md` | `fcfc047a` | [ADR 0001](adr/0001-vc-first-credentials.md) |
| `headless-dependency-map.md` | `564ffa82` | [ADR 0004](adr/0004-tauri-free-runtime-crate-boundary.md) |
| `sentinel-gaze.md`, `sentinel-adversarial-priors.md` | `92074c69` | [sentinel.md](sentinel.md) |
| `push-notifications-rfc.md` | `c9ed7d5f` | Superseded by the FCM notifications work |
| `could-do.md` | `505237a5` | — |
