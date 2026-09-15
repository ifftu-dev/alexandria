# Network Profiles

**Status:** Version-1 preprod profile implemented in the main app; coordinated
wire-protocol and service migration remains in progress (N01)
**Last updated:** 2026-09-15

## Purpose

A network profile gives one reviewable identity to an Alexandria deployment.
It prevents relay addresses, receipt issuers, Cardano parameters, founder keys,
cloud endpoints, committee coordinates, and protocol namespaces from drifting
across unrelated constants.

The current profile is bundled at:

```text
src-tauri/resources/networks/preprod.json
```

`src-tauri/src/network_profile.rs` parses and validates it. The app validates
the profile and its embedded resource identities during Tauri setup, before it
opens the profile manager or activates a user profile.

## Version-1 schema

| Field | Meaning | Current preprod state |
|---|---|---|
| `schema_version` | Parser compatibility version | `1` |
| `network_id` | Immutable application-network identity | `preprod` |
| `profile_revision` | Revision of this network's configuration | `1` |
| `cardano_network` / `cardano_network_magic` | Ledger selection | Cardano preprod / `1` |
| `relays` | Relay PeerId, DNS name, port, fallback IPs, and registry HTTPS origin | Mumbai and Frankfurt Fly.io relays |
| `receipt_issuer_keys` | Relay PeerIds trusted to sign username receipts | Both configured relays |
| `stake_registry_founder_keys` | Named Ed25519 keys that verify the bootstrap registry | Three founder public keys |
| `signed_bootstrap_registry_identity` | Expected schema and SHA-256 of the bundled signed registry | Bound to `bootstrap_registry.json` |
| `subject_qualification_policy_digests` | Approved subject-policy identities | Empty until T03 |
| `cloud_https_origin` / `cloud_service_id` | Optional Alexandria Cloud identity | Disabled (`null`) |
| `governance_locator` / committee fields | Optional governance/committee service identity | Disabled |
| `protocol_namespace` | Namespace intended for all network protocols | `/alexandria/preprod` |
| `optional_governance_anchor_address` | Optional Cardano governance anchor | Disabled |

The cloud fields remaining unset is deliberate. The hosted demo may later opt
into cloud custody of its organization signing key, but the cloud service,
custody protocol, OIDC provider, and deployment budget are separate prerequisites.
The OIDC provider is intentionally undecided.

## Validation and fail-closed behavior

The parser rejects:

- a profile over 64 KiB;
- duplicate JSON keys or unknown fields;
- unsupported schema versions or revision zero;
- malformed identifiers, PeerIds, Ed25519 keys, and SHA-256 digests;
- duplicate relay, receipt-issuer, or founder identities;
- relay receipt issuers that are not configured relays;
- invalid DNS names, zero ports, non-public fallback IPs, or non-HTTPS origins;
- a protocol namespace other than `/alexandria/<network_id>`;
- partially configured cloud or governance service groups;
- placeholder values; and
- a bundled bootstrap-registry digest that differs from the profile.

An invalid profile or resource identity prevents application setup. This is a
network activation boundary, not a warning-only preference.

## Immutable local profile binding

`profiles_index.json` format version 2 stores `network_id` on every
`ProfileSummary`. The profile manager:

1. rejects version-1 indexes before deserializing their rows;
2. rejects unknown future index versions;
3. validates every stored row against the embedded network on open;
4. requires creation and mnemonic-restore requests to name that network; and
5. exposes no operation that changes an existing profile's network.

This avoids opening the same identity database against a different set of
relays, status publishers, governance roots, or ledger parameters by accident.
A future cross-network move must be an explicit migration with its own data and
credential semantics.

## Consumers implemented in the main app

The main app currently reads the profile for:

- authoritative relay discovery and public fallback addresses;
- relay registry HTTPS origins;
- username receipt-issuer trust;
- stake-registry founder verification keys;
- the embedded bootstrap-registry digest;
- the optional governance anchor; and
- DHT provider-record key namespacing.

Public relay-registry queries use HTTPS. User-supplied extra relays may help
transport connectivity but do not become authoritative registry HTTP sources.

## Coordinated work still required

The main app still advertises the historical unscoped GossipSub,
request-response, Identify, and Kademlia protocol IDs such as
`/alexandria/catalog/1.0` and `/alexandria/vc-fetch/1.0`. The relay and monitoring
services expect those IDs too. Changing only one component would partition or
misreport the network.

N01 must therefore switch these in one reviewed compatibility plan:

1. main-app GossipSub topics;
2. main-app request-response, Identify, and Kademlia protocol IDs;
3. relay protocol names and registry behavior;
4. monitoring subscriptions, labels, and health interpretation;
5. any DNS/bootstrap transport changes and mobile fallback behavior; and
6. deployment ordering plus rollback rules.

Until that synchronized slice lands, `/alexandria/preprod` is enforced for DHT
provider-record keys only. Documentation and code must not describe full wire
isolation as complete.

## Updating a profile

Any profile change that affects trust or interoperability must be reviewed with
the resources it names. At minimum:

1. update the profile and affected embedded resource together;
2. recompute resource digests over the final exact bytes;
3. run the network-profile and bootstrap-registry tests;
4. run `cargo fmt -- --check` and `cargo clippy -- -D warnings`;
5. test existing profile-index rejection and same-network open/create/restore;
6. verify each configured HTTPS service and expected identity; and
7. coordinate protocol-affecting changes across the app, relay, and monitoring
   repositories before deployment.

There is no signed remote profile-update mechanism yet. Changing an embedded
trust root requires a new application release.
