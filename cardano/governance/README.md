# Alexandria Governance Smart Contracts

On-chain governance for the Alexandria learning platform, implementing whitepaper Section 4. Written in [Aiken](https://aiken-lang.org) v1.1.21 targeting Plutus v3 (Conway era).

## Validators

| Validator | Purpose |
|-----------|---------|
| `dao_registry` | Stores DAO state UTxOs (SubjectField and Subject DAOs). Enforces membership and parameter updates |
| `dao_minting` | Minting policy for DAO state tokens. One token per DAO, held at the registry |
| `election` | Election lifecycle: nomination → voting → finalization. Enforces quorum, deadlines, and seat allocation |
| `proposal` | Proposal lifecycle: draft → approve → vote → resolve. Supermajority and quorum enforcement |
| `reputation_minting` | CIP-68 soulbound reputation token minting. Reference + user token pairs anchored to skill proofs |
| `soulbound` | Spending validator for soulbound tokens. Only the platform can update; holders cannot transfer |
| `vote_minting` | Vote receipt token minting. One receipt per voter per election/proposal to prevent double-voting |

## Library Modules

| Module | Purpose |
|--------|---------|
| `types` | All on-chain types: DAO datums, election/proposal state, reputation metadata, redeemers |
| `reputation` | CIP-68 token name construction, reference token helpers |
| `state_token` | DAO state token name construction and UTxO lookup |
| `utils` | Quorum checks, majority/supermajority, deadline validation, top-N candidate selection |

## Building

Requires [Aiken](https://aiken-lang.org/installation-instructions) v1.1.21+.

```sh
aiken build
```

Produces `plutus.json` — the CIP-57 blueprint containing all validator scripts.

## Testing

```sh
aiken check
```

7 unit tests covering quorum, majority, supermajority, and top-N selection logic.

## Architecture

- **DAO hierarchy** mirrors the skill taxonomy: one DAO per Subject Field, one per Subject
- **Elections** select council members from nominees who hold sufficient SkillProofs
- **Proposals** require council approval before community vote
- **Reputation tokens** are CIP-68 soulbound NFTs — the reference token holds metadata (skill, proficiency, confidence), the user token is non-transferable
- **Vote receipts** prevent double-voting without requiring on-chain voter rolls

See `docs/whitepaper-consolidated-v0.0.1.md` Section 4 for the full governance specification.
