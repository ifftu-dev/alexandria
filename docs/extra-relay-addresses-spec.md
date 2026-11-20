# Extra relays: dialable addresses

Status: active
Updated: 2026-11-21
Triage: ready-for-agent
Repos: alexandria

## Problem Statement

The `p2p.extra_relays` setting (community relays: `{ peer_id, host, port }`)
never connects. Every extra-relay address is built as
`/dns4/{host}/...`, for bootstrap, circuit listen and circuit dial, even
when the host is an IP address. The swarm has no DNS transport, so the TCP
and QUIC transports reject each of those addresses and nothing dials the
relay. Built-in relays are unaffected: bootstrap also dials the network
profile's `fallback_ips`, and their circuit reservations ride that
connection.

Found by the relay's staging rehearsal (`alexandria-relay`
`docs/relay-transports-spec.md`): a dev build with the staging relay added
never dialled it, which blocked rehearsal items 3 and 7.

## Solution

Resolve each extra relay's host when the setting is applied (at
`p2p_start` and on save), and build every extra-relay address from the
resulting IPs as `/ip4` or `/ip6`, the way built-in relays use
`fallback_ips`.

- An IP literal (IPv6 with or without brackets) is used as is.
- A DNS name goes through the system resolver (`tokio::net::lookup_host`,
  i.e. `getaddrinfo`), which works on every platform. libp2p's DNS
  transport is not added: its resolver reads the system resolver
  configuration file, which Android and iOS do not have.
- Up to four distinct IPs per relay, in the resolver's order; each lookup
  times out after 10 s.
- Bootstrap dials TCP and QUIC on every resolved IP. Circuit listen and
  circuit dial use one address per relay, its first IP, keeping relay
  pressure proportional to the number of relays.
- A host that does not resolve keeps its relay in the set (its PeerId still
  counts as a relay) with no addresses, and is logged; it is resolved again
  the next time the setting is applied.

## Out of Scope

- Re-resolving while the node runs: the setting is applied at start and on
  save, as before.
- Built-in relay addresses, which already work.

## Testing Decisions

Unit tests in `p2p::discovery`: IP literals need no lookup; `localhost`
resolves through the system resolver; a `.invalid` host gets no
addresses; bootstrap, circuit listen and circuit dial addresses use the
resolved IPs, never `/dns4`. Manual: a dev build with the staging relay
added gets a reservation from it (relay rehearsal items 3 and 7).

## Notes
