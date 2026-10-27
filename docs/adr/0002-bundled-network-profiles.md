# 0002. Networks are bundled profiles, and each user profile is bound to one

Status: implemented
Date: 2026-09-15

## Context

Relay PeerIds and addresses, username-receipt issuers, Cardano parameters, stake-registry founder keys, Cloud and committee endpoints, and the protocol namespace were separate constants spread across modules. They could drift apart, and nothing stopped a user profile created against one deployment from being opened against another deployment's relays, trust roots or ledger.

## Decision

A network is one reviewable JSON document bundled into the build: `src-tauri/resources/networks/preprod.json`, parsed and validated by `src-tauri/src/network_profile.rs`.

- **It owns the network's identity and coordinates**: `network_id`, `profile_revision`, Cardano network and magic, relays (PeerId, DNS name, port, fallback IPs, registry HTTPS origin), receipt-issuer keys, stake-registry founder keys, the bootstrap-registry digest, pinned subject-qualification policy digests, optional Cloud, governance and committee coordinates, the default governance anchor, and `protocol_namespace`.
- **No other copy.** Code and docs read values from the profile; they don't hard-code copies of them.
- **Validation is fail-closed.** Size and depth limits, duplicate keys, unknown fields, placeholders, malformed keys or digests, non-HTTPS origins, and a namespace other than `/alexandria/<network_id>` are all rejected. An invalid profile stops application setup.
- **User profiles are bound to `network_id` for life.** `profiles_index.json` stores `network_id` on every profile. The app and CLI share `profile::manager::load_network_index`, which refuses any profile bound to a different network. Creation and restore must name the embedded network, and no operation changes an existing profile's network.
- **The namespace is only partly applied.** `/alexandria/preprod` currently applies only to DHT provider-record keys (`SHA256("{protocol_namespace}.providers")` in `p2p/discovery.rs`). GossipSub topics and request-response, Identify and Kademlia protocol IDs are still unscoped (`/alexandria/<name>/1.0`). Changing them must be one coordinated app, relay and monitoring deployment.

## Consequences

- Changing a trust root, relay or endpoint means a reviewed edit to one file and a new app release. There is no signed remote profile update.
- Each build has one network. Moving a profile between networks would need an explicit migration with its own data and credential rules.
- The relay and monitoring repos must not keep their own copies of profile values; they should link to the profile.
- Full wire isolation between networks is not done. Docs and code must not claim it is.
- Details and the field table: [network-profiles.md](../network-profiles.md).
