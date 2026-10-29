# Node2Shell / HTTP2Shell research and repository recommendations

Status: historical research snapshot; recommendations reflect the inspection date.

**Researched:** 2026-10-25
**Scope:** CVE identity, upstream fixes, exploit evidence, and recommendations for
all five current mark3 repositories plus the IFFTU website. Recommendations only;
no implementation or deployment changes. Local findings below were supplied by
the parent agent's concurrent source audit. Deployed artifacts were not inspected.

## Verified identity and dates

The likely intended names are **Node2Shell** and **HTTP2Shell**. Both identify
**CVE-2026-111497**, a flaw in Google's native Brotli decoder. Imperva calls its
October 22 research Node2Shell; Aikido's October 23 article uses HTTP2Shell for
the same CVE's wider ecosystem exposure. These are research nicknames, not two
separate CVEs. Sources: [Imperva's own research](https://www.imperva.com/blog/node2shell-from-brotli-data-to-remote-code-execution/),
[Aikido's naming article](https://www.aikido.dev/blog/python-packages-vulnerable-node2shell-brotli-cve-2026-111497).

- **February 27, 2026:** upstream commit `0ed4268a4401abcb4472096340477cf4af7d1a2b`
  introduced the decoder correction. The CVE record's `datePublic` also names
  February 27; this must not be confused with the later security release or CVE
  publication. Sources: [Brotli commit](https://github.com/google/brotli/commit/0ed4268a4401abcb4472096340477cf4af7d1a2b),
  [Google CNA record](https://raw.githubusercontent.com/CVEProject/cvelistV5/main/cves/2026/111xxx/CVE-2026-111497.json).
- **October 21, 2026:** Node published its security advisory and fixed releases.
  Brotli published 1.0.10, 1.1.1, and 1.2.1 that evening, approximately 19:27 UTC.
  Sources: [Node advisory](https://nodejs.org/en/blog/vulnerability/october-2026-security-releases),
  [Brotli releases](https://github.com/google/brotli/releases),
  [release timestamps](https://api.github.com/repos/google/brotli/releases?per_page=3).
- **October 22, 2026, 13:35:41.975 UTC:** the CVE record was published. Its retrieved
  version was updated October 24. Google assigns CVSS 4.0 **9.2 Critical**; Node
  rates its advisory **High**. These are attributed assessments, not evidence that
  every application is exploitable. Sources: [CNA record](https://raw.githubusercontent.com/CVEProject/cvelistV5/main/cves/2026/111xxx/CVE-2026-111497.json),
  [Node advisory](https://nodejs.org/en/blog/vulnerability/october-2026-security-releases).

Google Brotli's GitHub security-advisory listing showed no published advisories
when checked. Its release notes and Google-authored CNA record nevertheless
explicitly document the CVE and fixes. [Advisory listing](https://github.com/google/brotli/security/advisories).

## Affected and fixed versions

| Component / release line | Affected upstream range | First fixed release |
| --- | --- | --- |
| Node.js 22 | 22.x before 22.23.4 | [22.23.4](https://nodejs.org/en/blog/release/v22.23.4) |
| Node.js 24 | 24.x before 24.22.1 | [24.22.1](https://nodejs.org/en/blog/release/v24.22.1) |
| Node.js 26 | 26.x before 26.12.1 | [26.12.1](https://nodejs.org/en/blog/release/v26.12.1) |
| Google Brotli 1.0 | `>=1.0.3, <1.0.10` | [1.0.10](https://github.com/google/brotli/releases/tag/v1.0.10) |
| Google Brotli 1.1 | `>=1.1.0, <1.1.1` | [1.1.1](https://github.com/google/brotli/releases/tag/v1.1.1) |
| Google Brotli 1.2 | `>=1.2.0, <1.2.1` | [1.2.1](https://github.com/google/brotli/releases/tag/v1.2.1) |

The Brotli ranges are the CNA's separate branch ranges: a blanket comparison
against 1.2.1 incorrectly flags fixed 1.0.10 and 1.1.1. The Node advisory covers
22/24/26; omission of an older line does not establish safety. Use a supported,
fixed line or verify the distributor's specific backport. [CNA record](https://raw.githubusercontent.com/CVEProject/cvelistV5/main/cves/2026/111xxx/CVE-2026-111497.json),
[Node advisory](https://nodejs.org/en/blog/vulnerability/october-2026-security-releases).

Node cherry-picked the fix. Its fixed 22.23.4 source still labels Brotli 1.1.0,
while fixed 24.22.1 and 26.12.1 label it 1.2.0. Therefore a Brotli version string
alone is insufficient to classify a patched Node build; verify the Node release
and patch provenance. Sources: [22 header](https://github.com/nodejs/node/blob/v22.23.4/deps/brotli/c/common/version.h),
[24 header](https://github.com/nodejs/node/blob/v24.22.1/deps/brotli/c/common/version.h),
[26 header](https://github.com/nodejs/node/blob/v26.12.1/deps/brotli/c/common/version.h),
[24 release patch entry](https://nodejs.org/en/blog/release/v24.22.1).

## Technical prerequisites and impact

Malformed block-switch input can be mistaken for incomplete input. The streaming
decoder continues adding bytes to an eight-byte internal buffer, corrupting
memory. Node documents possible process crashes, memory disclosure, and remote
code execution when an application decompresses attacker-supplied Brotli data
through `node:zlib`. [Node advisory](https://nodejs.org/en/blog/vulnerability/october-2026-security-releases).

The exposure condition is **untrusted input reaching a vulnerable decoder**.
HTTP/2 is not a prerequisite: this is a compression-library defect, not an
HTTP/2 protocol defect. Merely listening for HTTP requests, enabling TLS, or
encoding Brotli responses does not establish the required decoding path. This
is an inference from the decoder-specific advisory and upstream patch.
[Brotli patch](https://github.com/google/brotli/commit/0ed4268a4401abcb4472096340477cf4af7d1a2b).

Relevant paths include an application consuming a malicious Brotli response via
Node's built-in `fetch()`, and configured request middleware inflating incoming
Brotli bodies. Imperva tested both; a bare Node HTTP server does not automatically
inflate such bodies. Envoy independently limits its affected deployment scope
to configurations enabling its Brotli decompressor. Sources:
[Imperva tests](https://www.imperva.com/blog/node2shell-from-brotli-data-to-remote-code-execution/),
[Envoy advisory](https://github.com/envoyproxy/envoy/security/advisories/GHSA-3wvj-j6qm-62mr).

## Exploit evidence and uncertainty

- **Research demonstration:** Imperva reports code execution in official,
  uninstrumented Node 24.22.0 Linux arm64 containers through fetched responses and
  configured Express/body-parser inputs. It required multiple attempts; 24.22.1
  rejected the same inputs. This is the team's first-party report, not an
  independently reproduced result of this audit, and does not establish impact
  on every build. [Imperva report](https://www.imperva.com/blog/node2shell-from-brotli-data-to-remote-code-execution/).
- **Public PoC claims:** author `dimasma0305` reports network RCE in exact Linux
  x86-64 lab profiles, plus DoS in other configurations. The Express result
  depends on a pinned binary, worker/socket configuration, restart supervisor,
  and probabilistic address guessing. The README was checked at commit
  `452151d4945c3f5d9b58d9d25792d0d8aef4ab16` (October 25). The code was not run or
  independently validated. These claims do not establish a portable exploit.
  [Author's pinned README](https://github.com/dimasma0305/CVE-2026-111497-POC/blob/452151d4945c3f5d9b58d9d25792d0d8aef4ab16/README.md).
- **Vendor caution:** Red Hat states that no public, reliable RCE PoC is available
  and rates its products Important / CVSS 3.1 8.1, citing exploitation difficulty
  and platform hardening. That is Red Hat's assessment, not proof that research
  demonstrations are impossible. The differing statements leave general exploit
  maturity uncertain. [Red Hat assessment](https://access.redhat.com/security/cve/cve-2026-111497).
- **Observed attacks:** CISA's CVE enrichment assessed exploitation as `none` on
  October 23. The retrieved KEV feed, dated October 23 at 19:30:18.9209 UTC, had
  no entry for this CVE. Neither proves absence of real-world exploitation on
  October 25; no confirmed incident was established by this research.
  [CISA enrichment in the CVE record](https://raw.githubusercontent.com/CVEProject/cvelistV5/main/cves/2026/111xxx/CVE-2026-111497.json),
  [CISA KEV feed](https://www.cisa.gov/sites/default/files/feeds/known_exploited_vulnerabilities.json).

## Recommendations for mark3 and IFFTU

This section records the **parent agent's local audit**, including its subsequent
branch corrections. Current primary repositories are `alexandria`,
`alexandria-cloud`, `alexandria-monitoring`, `alexandria-relay`, and `website`.
`iroh-live-patched` is absent; [the app workspace manifest](../Cargo.toml) records
the live/iroh-moq/moq-media ownership in-tree. Worktrees are branch variants, not
additional deployed applications. Local Node was reported as 26.12.1 with Brotli
1.2.1; this does not attest CI, function, container, or customer runtimes.

| Repository | Local evidence | Recommended action |
| --- | --- | --- |
| `alexandria` | [CI](../.github/workflows/ci.yml) and related workflows use floating Node 24 and one Node 22; cached the remote default branch also has those settings. No engines constraint or shipped Node runtime identified. | Make build tooling select a consistent patched release, at least 24.22.1 or 22.23.4 for the corresponding line. Use an explicit version/floor and verify the resolved version; `check-latest` can supplement a floating selector. Add a matching supported `engines` range. Rebuild using patched tooling; this is not evidence of a public Node server. |
| `alexandria-cloud` | Current feature checkout and stale local default branch (`cb803f6`, October 20) have a floating Node 22 console build stage and CI. **Cached the remote default branch (`bb732ea`, October 25) already has `mirror.gcr.io/library/node:22.23.4-bookworm-slim` and CI 22.23.4.** Final image is Rust on Debian Bookworm with curl, without shipped Node. Sources: Dockerfile (`alexandria-cloud/Dockerfile` in the sibling repository), CI (`alexandria-cloud/.github/workflows/ci.yml` in the sibling repository); branch-specific evidence supplied by parent. | Carry the existing the remote default branch runtime fix into active branches, preserving user changes; avoid duplicating it. Pull a fresh patched base or verified digest and rebuild. Inspect the final image's `libbrotli1` separately and apply Debian's security update where needed. Neither current production nor live remote state was attested. |
| `alexandria-monitoring` | Dockerfile (`alexandria-monitoring/Dockerfile` in the sibling repository) uses Node 24.22.1-alpine for its frontend build; CI (`alexandria-monitoring/.github/workflows/ci.yml` in the sibling repository) uses 24.22.1. Final stage is Rust and installs ca-certificates only. | Preserve these already-patched Node settings and verify deployed artifact provenance. No additional Node version bump is required solely for this CVE. Inspect actual final-image packages before asserting absence of native Brotli. |
| `alexandria-relay` | Rust-only project; no Node runtime identified. Final Docker stage installs ca-certificates only. Sources: manifest (`alexandria-relay/Cargo.toml` in the sibling repository), Dockerfile (`alexandria-relay/Dockerfile` in the sibling repository). | No Node-specific change identified. Verify deployed base-image packages and any native Brotli decoder before declaring deployment-wide non-exposure; HTTP service presence alone is insufficient. |
| `website` | .nvmrc (`website/.nvmrc` in the sibling repository) and Netlify build config (`website/netlify.toml` in the sibling repository) pin 22.23.4; engines (`website/package.json` in the sibling repository) require `^22.23.4 || ^24.22.1 || ^26.12.1`; CI follows .nvmrc. early-access (`website/netlify/functions/early-access.ts` in the sibling repository) and enquiry (`website/netlify/functions/enquiry.ts` in the sibling repository) use server-side fetch to Plunk. | Keep existing build fixes. Verify the **managed function runtime** independently and obtain confirmation that its Node build is patched. Fixed Plunk destinations are not evidence of arbitrary attacker URL control, but outbound decoding remains a runtime assessment point. A patched build pin alone does not establish function-runtime remediation. |
| IFFTU website | Inspected separately from this workspace. Its Astro configuration has no server adapter; Netlify build configuration selects Node 22 and Lighthouse CI selects Node 24, with no .nvmrc or engines constraint. | Pin build Node to at least 22.23.4 and CI Node to at least 24.22.1, add corresponding version/engines controls, and rebuild. No public Node server was identified; the demonstrated gap is reproducible build-tool selection, not confirmed remote exposure of the static site. |

For Netlify functions, the default runtime follows the build's supported Node
major, with a fallback where necessary. A runtime override such as
`AWS_LAMBDA_JS_RUNTIME=nodejs24.x` must be configured through UI, CLI, or API,
**not `netlify.toml`**, then redeployed. This selects a managed major, not a
specific security patch: confirm actual runtime remediation with Netlify rather
than treating `NODE_VERSION` as proof. [Netlify function configuration](https://docs.netlify.com/build/functions/configuration/?fn-language=js).

For cloud's final Debian image, the dependency chain is
`curl` → `libcurl4` → `libbrotli1`; Bookworm's fixed Brotli package is
**1.0.9-2+deb12u1**, despite its older upstream version. Verify the installed
binary package version in the built and deployed image, rebuild against updated
security packages, and redeploy if needed. The audited healthcheck is
`curl -fsS` against loopback `/healthz` without `--compressed`; package presence
does not establish attacker-controlled decoder reachability. Sources:
[Debian curl dependencies](https://packages.debian.org/bookworm/curl),
[libcurl4 dependencies](https://packages.debian.org/bookworm/libcurl4),
[Debian CVE tracker](https://security-tracker.debian.org/tracker/CVE-2026-111497),
local Dockerfile (`alexandria-cloud/Dockerfile` in the sibling repository).

The app and cloud locks contain Rust `brotli` 8.0.4 and `brotli-decompressor`
5.0.3, with no `brotli-sys` found. These Rust package versions must **not** be
compared with Google's C-library affected ranges. The parent's successful
`cargo tree --offline --locked -e features -i brotli-decompressor` checks showed
`std`/`alloc-stdlib` without an unsafe decoder feature, through Tauri/tauri-utils
compression in the app and tower-http `compression-br` in cloud. These checks
cover the **host/default feature graph only**, not every target or feature set.
Cloud's audited use is outbound `CompressionLayer`; its reqwest configuration
disables default features and explicitly enables rustls-tls/json, not Brotli.
These are bounded source observations, not a blanket Rust or deployment safety
claim. Sources: [app lock](../Cargo.lock), cloud lock (`alexandria-cloud/Cargo.lock` in the sibling repository),
cloud manifest (`alexandria-cloud/Cargo.toml` in the sibling repository), parent feature-graph audit.

Two additional cloud worktrees already contain the
patched 22.23.4 configuration, consistent with cached the remote default branch. The older
website design worktree retains floating 22/24 settings. Preserve existing
security pins when merging these branches; do not classify branch drift as an
independent application or proof of production exposure. No live remote refresh,
container inspection, managed-runtime attestation, or production test was
performed for this note.

## Unrelated shell-quote advisories

The npm package `shell-quote` has separate command-injection flaws, not aliases
for this Brotli CVE. Its maintainer documents **CVE-2026-9277**, published May 22,
affecting 1.1.0–1.8.3 and fixed in 1.8.4, and **CVE-2026-102422**, published
September 29, affecting `>=1.8.4, <1.11.0` and fixed in 1.11.0. They concern
attacker-influenced tokens passed through `quote()` and subsequently to a shell.
Updating that npm package does not patch Node's native decoder. Sources:
[May maintainer advisory](https://github.com/ljharb/shell-quote/security/advisories/GHSA-w7jw-789q-3m8p),
[September maintainer advisory](https://github.com/ljharb/shell-quote/security/advisories/GHSA-pqg4-j6r4-53mv).

## Evidence limits

Upstream facts use maintainers, the assigning CNA, government/vendor records,
and researchers' own reports. Research claims are attributed, not certified by
this audit. No exploit was executed. Local recommendations use the parent audit
and remain conditional on actual installed versions, decoder reachability, and
deployment provenance. Only this requested research note was created; existing
documentation, code, configuration, and user edits were left untouched.
