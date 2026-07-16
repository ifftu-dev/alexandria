# Sentinel Federation — Threat Model & Privacy Budget

> **Status:** Option B shipped. Option A still design-phase. This document frames the decisions made (and remaining) so the privacy guarantees in [sentinel.md](sentinel.md) stay enforceable.
>
> **Goal:** let Sentinel's AI models improve across the user base — keystroke patterns of legitimate humans become sharper, novel cheat techniques get learned once and spread — without any single user's behavioral data being recoverable from what we publish.
>
> **What shipped (Option B):**
> - Labeled adversarial-prior pipeline ([sentinel-adversarial-priors.md](sentinel-adversarial-priors.md) §Phases 1–7)
> - DAO-signed ONNX classifier-weights distribution ([sentinel-adversarial-priors.md](sentinel-adversarial-priors.md) §Phase 8)
> - Operator safety valves: kill switch, version blocklist, per-signal opt-out ([sentinel-adversarial-priors.md](sentinel-adversarial-priors.md) §Phase 9, [sentinel-runbook.md](sentinel-runbook.md))
> - Three-layer content-addressed re-verification, resolver timeout, bytes size caps, mobile gate, SHA-pinned bundled model ([sentinel.md](sentinel.md) §Runtime Model Updates)
> - On-device ONNX inference for the paste classifier ([sentinel.md](sentinel.md) §AI Models)
>
> **What hasn't shipped (Option A):** per-user gradient sharing, DP-SGD, secure aggregation, Cardano-stake-gated submissions — see §10. Also pending: real DAO threshold signature (placeholder Blake2b in `compute_prior_signature`) and real-world FPR measurement (synthetic-only holdout today).

---

## 0. TL;DR for decision-makers

- **Naive "weights on Iroh" is a privacy regression.** Per-user gradient deltas leak training inputs (gradient-inversion attacks) and reveal participation (membership-inference attacks). Just publishing `.json` weight files under a DAO key *does not* deliver the guarantee most people mean by "no user data is tied to anyone."
- **To get the guarantee we want, we need five things:** local differential privacy (DP-SGD with a documented ε budget), secure/trimmed aggregation, Sybil resistance tied to Cardano stake, poisoning defense, and a decision that the **face embedder is never federated** under any scheme.
- **Two viable shapes exist:**
  - **Option A — Federated learning (per-user deltas, DP-noised, aggregated).** Higher accuracy, real research/engineering cost (months), meaningful residual risk. **Not shipped.**
  - **Option B — Federated adversarial priors (DAO publishes labeled cheat patterns; users train privately against them).** Weaker in theory, *much* stronger privacy, ships in weeks, no per-user leakage surface. **Shipped.**
- **Outcome so far:** Option B's paste classifier ratification pipeline is live. A synthetic-only v1 model hits TPR=1.0 / FPR=0.0 on the synthetic holdout; real-world holdout data will move those numbers and is the trigger for revisiting Option A.

---

## 1. Assets

What we're actually protecting, in priority order:

| # | Asset | Why it matters |
|---|-------|----------------|
| A1 | **Raw typing rhythm (per-user digraph timings)** | Re-identifiable across accounts/sites; considered biometric in EU/CA law |
| A2 | **Raw mouse trajectories** | Re-identifiable; can reveal physical/motor disabilities |
| A3 | **Face embeddings (LBP histograms)** | Identity-equivalent. Sharing these is effectively building a face registry. |
| A4 | **Per-user model gradients** | Leak A1/A2/A3 via gradient inversion |
| A5 | **Membership (who participated when)** | Reveals who uses Alexandria and when they're taking assessments |
| A6 | **Classifier accuracy itself** | Public good — if poisoned, honest learners get false-positive flagged |
| A7 | **Label distribution** (what counts as "cheat" over time) | Leaking this tells attackers which attacks aren't caught yet |

A3 and A5 are the ones people underestimate — they're the reason "no user data in the weights" isn't automatically true.

---

## 2. Adversaries

| Code | Adversary | Capability |
|------|-----------|------------|
| **Adv-Curious** | Any honest-but-curious peer | Reads all Iroh blobs we publish; runs offline analysis |
| **Adv-Target** | Attacker with partial info on a specific learner | Wants to confirm whether learner X contributed which behavior |
| **Adv-Poison** | Malicious contributor | Publishes crafted gradient deltas to degrade classifier or install backdoors |
| **Adv-Sybil** | Attacker with N fake identities | Outvotes honest contributors in aggregation |
| **Adv-Collude** | k colluding peers pool observations | Breaks aggregation privacy that assumed independence |
| **Adv-Global** | Passive global observer (nation-state, large ISP) | Traffic analysis on the Iroh network |
| **Adv-Cheat** | The learner being monitored | Wants Sentinel to *miss* their cheating; may know internal weights (public by construction) |

**Out of scope (explicit):** Adv-Global (no traffic-analysis defense beyond Iroh's own), >50% coordinated Adv-Collude (federated learning doesn't pretend to survive this), TEE/hardware-rooted attestation.

---

## 3. Attack surface

Concrete attacks we have to either mitigate or explicitly accept:

### 3.1 Gradient inversion (Adv-Curious, Adv-Target)
**Reference:** *Deep Leakage from Gradients* (Zhu et al. 2019), *Inverting Gradients* (Geiping et al. 2020). For small models and batch size = 1, attackers have reconstructed the exact training input with high fidelity. Our 4→8→4→8→4 keystroke AE is *exactly* the small-model regime these attacks target.
**Mitigation path:** DP-SGD with per-sample gradient clipping + Gaussian noise (ε budget documented in §5). Minimum batch size ≥ 16 per submission.

### 3.2 Membership inference (Adv-Target)
**Reference:** Shokri et al. 2017. Attacker queries the public model and learns whether a given record was in training.
**Mitigation:** same DP noise covers this (DP is the strongest known defense against MIA), plus: participation is already public because submissions are signed by Cardano stake key — so we're only defending the *content*, not the *fact*, of contribution. That framing needs to be explicit in the user-facing opt-in.

### 3.3 Model poisoning (Adv-Poison)
**Variants:**
- **Untargeted** — random noise injection to crater accuracy
- **Targeted** — flip classifier output for a specific input (e.g., "my bot's mouse pattern looks human")
- **Backdoor** — train a trigger: any input with property X bypasses the classifier
**Mitigation:** Byzantine-robust aggregation (trimmed mean / median-of-means / Krum). Any one of these tolerates ~⌊n/4⌋ malicious contributors per round. None of them handles a majority-malicious quorum.

### 3.4 Sybil flooding (Adv-Sybil)
Without identity binding, one attacker = unbounded contributors, which breaks every Byzantine-robust aggregator.
**Mitigation:** require each submission to be signed by a Cardano stake key with non-trivial stake (threshold TBD). Rate-limit per stake key per epoch. Cost-of-attack = cost of stake × rate limit.

### 3.5 Face-embedding aggregation (Adv-Collude)
LBP histograms are near-unique per person. Even DP-noised, averaging hundreds of user embeddings produces something closer to "a face registry" than "statistical patterns." This is the hard line.
**Mitigation:** **don't federate the face model.** No ε small enough makes this safe at scale. Face stays local-only, as in the current design.

### 3.6 Cheater-adjacent attacks (Adv-Cheat)
The learner being monitored already has white-box access to the model (weights are public by construction of federation). They can:
- Run the classifier locally and search input space for "what fools it"
- Train a generator that mimics the boundary of their own profile
**Mitigation:** this is inherent to any client-side anti-cheat and is the whole reason Sentinel is non-punitive by default (see sentinel.md §Design Principles #2). We don't pretend to solve it; we raise cost.

---

## 4. Assumptions

1. Iroh blob store provides eventual availability; content-addressed; no mutable servers
2. Cardano stake keys provide bounded Sybil resistance (cost = locked stake)
3. Majority of contributors (>75% per aggregation round) are honest
4. Clients can run DP-SGD on-device without GPU in reasonable time (our models are small enough — verified for the AE; CNN needs measurement)
5. Users understand that federation is opt-in and that *participation itself* is public even though *content* is noised
6. Alexandria's threat model doesn't require hiding from state-level adversaries

---

## 5. Privacy budget

The honest question is not "can we federate privately" but "how much privacy are we willing to spend to get how much accuracy."

### 5.1 Recommended starting point

| Parameter | Value | Rationale |
|-----------|-------|-----------|
| **Per-submission ε** | 1.0 | Typical for production DP-FL (Apple ≈ 2, Google FL ≈ 1–4, academic ≈ 0.1–1) |
| **δ** | 10⁻⁶ | Standard; < 1/n where n = plausible user count |
| **Gradient clip bound C** | 1.0 (L2 per-sample) | Conservative; larger = less noise needed but more leakage per-sample |
| **Noise mechanism** | Gaussian | Fits DP-SGD composition theorems |
| **Minimum batch per submission** | 16 | Below this, gradient inversion becomes trivially successful |
| **Lifetime ε per user** | 8.0 | Allows ~8 federation submissions per user at full budget before we stop accepting from them (or reset annually) |
| **Epoch length** | 1 week | How often the DAO publishes new central weights |
| **Min contributors per epoch** | 10 | Below this, don't aggregate — single-contributor averaging is meaningless |

### 5.2 What ε = 1 *actually buys*

Plain English for each guarantee claim:

- **Gradient inversion:** attacker can recover a *rough* distribution of the user's typing features, not specific digraphs. Good enough for re-identification? Probably not at ε=1 with batch ≥ 16, but this is an empirical question — we should run inversion attacks against our own submissions before shipping.
- **Membership inference:** ε=1 bounds the attacker's advantage at roughly `e^ε = 2.7x` better than random, which is detectable but not devastating. If we want MIA-hard, we need ε ≤ 0.1 and accept much worse accuracy.
- **Colluding peers:** DP composition is post-hoc, so k colluders observing each other's noised submissions still see only ε-bounded leakage *per contributor* — they don't get to pool noise away.

### 5.3 What ε = 1 *does not* buy

- Protection against an attacker who sees *multiple* submissions from the same user over time and fits a denoising model on the aggregate. Lifetime ε cap (§5.1) is the defense; it's crude.
- Protection against any attack if we miscount the budget (e.g., forget to include the initial AE weight seed in the ε accounting).
- Protection if we federate the face model. Just don't.

---

## 6. What we federate, and what we don't

| Component | Federate? | Rationale |
|-----------|-----------|-----------|
| Keystroke autoencoder (4→8→4→8→4, ~300 params) | **Yes, with DP-SGD** | Small, timing-only, ε-bound tractable; most to gain since each user is currently cold-start |
| Mouse CNN — dense layers only (~5k params) | **Yes, with DP-SGD** | Conv layers stay random per existing spec, so we avoid federating feature extractors |
| Mouse CNN — conv layers | **No** | Reservoir-computing design; federating defeats the point |
| Face embedder (LBP 944-D) | **No, ever** | Identity-equivalent; no ε makes aggregation safe |
| Rule-based thresholds | **Partially** | Aggregate *flag-frequency statistics* (counts, not gradients) — easy DP, low sensitivity. Lets the DAO tune thresholds against real false-positive rates without touching any user's data. |
| Device fingerprint | **No** | Already per-device by design |
| Anomaly-flag severity map | **No (governed)** | Lives in code; changes go through DAO-signed code updates, not federation |

---

## 7. Aggregation protocol (Option A details)

For the components we *do* federate:

### 7.1 Round structure

```
┌─ Epoch N ───────────────────────────────────────────────────────┐
│                                                                 │
│  [Client]                                                       │
│   1. Train locally for K steps on user's own session data       │
│   2. Compute gradient delta Δ = local_weights − central_N       │
│   3. Clip: Δ := Δ × min(1, C / ||Δ||₂)                          │
│   4. Add noise: Δ_priv := Δ + N(0, σ²I)   where σ from §5       │
│   5. Sign with Cardano stake key, publish as Iroh blob          │
│                                                                 │
│  [Any peer, deterministic]                                      │
│   6. Collect all submissions for epoch N                        │
│   7. Verify Sybil gate: stake ≥ threshold, rate limit OK        │
│   8. Discard top/bottom 25% by L2 norm (Byzantine defense)      │
│   9. Trimmed mean → central_{N+1}                               │
│  10. DAO-signature over central_{N+1}, publish under stable CID │
│                                                                 │
│  [Client]                                                       │
│  11. Pull central_{N+1} before next session                     │
└─────────────────────────────────────────────────────────────────┘
```

### 7.2 Aggregator trust

Step 6–10 must be deterministic and verifiable. Every honest peer reaches the same central_{N+1} given the same submission set; disagreement = one of the peers is lying, and the DAO committee resolves by majority over their independently-computed results. This is cheaper than full secure aggregation / MPC and adequate for our threat model.

### 7.3 Delivery

Central weights are published under a DAO-signed Iroh CID that's discoverable through the catalog (like courses). Clients check the signature and pull. Old central weights remain pinned — no forced upgrade, users can stay on an older version if they distrust a new release.

---

## 8. Option B: Federated Adversarial Priors (strongly recommended starting point)

Instead of federating user gradients, we federate *curated cheat examples*.

### 8.1 How it works

- A dedicated **Sentinel DAO** maintains a growing library of labeled attack patterns: mouse trajectories from scripted bots, keystroke rhythms from paste macros, face videos from common spoofing attacks, etc. (See §10 decision 6.)
- Anyone can propose a pattern; the Sentinel DAO votes to ratify.
- Each ratified example is published as an Iroh blob under the Sentinel DAO's signing key.
- Every client pulls the library and trains their local anomaly models with those examples as negative class.
- Each client's final model is still per-user, but they all share the same adversarial priors.

### 8.2 Why this is the right first move

| Concern | Option A (federated weights) | Option B (federated priors) |
|---------|-----------------------------|---------------------------|
| Per-user gradient leakage | Real risk, needs DP accounting | Zero — nothing per-user is published |
| Implementation complexity | 3–6 months engineering + research | 2–3 weeks (blob publishing + local training changes) |
| Covers novel attacks | Yes, in principle — if users happen to experience them and contribute | No — needs DAO to curate |
| Re-uses existing infra | Needs new aggregator code | Re-uses course-catalog pipeline |
| Reversible if we get it wrong | Hard — leaked data stays leaked | Easy — just stop pulling |

Option B's main weakness — "needs curation" — is actually a *feature* for Alexandria: the DAO already has governance structures to review and ratify content. Adding a "cheat pattern" category is one more catalog entry, not a whole new trust domain.

### 8.3 Where Option A earns its keep

Option A is worth it *only* if Option B's curated-priors approach empirically fails to lower false-positive rates below an agreed threshold (e.g., >5% honest-learner false-positives after 6 months of Option B). At that point, federated keystroke-AE training on real human data is the natural next step — and by then we'll have real telemetry to price the ε budget against.

---

## 9. Privacy guarantee rewrite (for `sentinel.md`)

If we ship Option B, sentinel.md's guarantee #4 and #6 stay essentially as-is, with one clarifying sentence each:

```diff
 4. **AI model weights are not biometric data**: Autoencoder/CNN weights
    encode statistical patterns of typing/movement, not recoverable input
    data. LBP embeddings cannot be reverse-engineered into face images.
+   Published *adversarial priors* (labeled cheat patterns) contain no
+   individual user data; they are curated by the DAO like any other
+   catalog content.

 6. **No server-side data**: All behavioral processing happens on-device.
    The Rust backend stores only numeric scores and categorical flags in
    local SQLite.
+   The DAO-published adversarial-prior library is read-only from each
+   client's perspective and carries no user identifiers.
```

If we ever ship Option A, guarantee #4 requires real rewording:

```diff
-4. **AI model weights are not biometric data**: Autoencoder/CNN weights
-   encode statistical patterns of typing/movement, not recoverable input
-   data. LBP embeddings cannot be reverse-engineered into face images.
+4. **Federated model contributions are (ε, δ)-differentially-private**:
+   For the keystroke autoencoder and mouse-CNN dense layers, per-user
+   gradient updates are clipped to L2 norm 1.0 and perturbed with
+   Gaussian noise calibrated to ε ≤ 1.0 per submission, with a lifetime
+   budget of ε ≤ 8.0 per stake key. The face embedder is never
+   federated; its 944-dimensional LBP histogram stays in device-local
+   storage. See `sentinel-federation.md` §5 for the budget derivation.
```

This is a *weaker* guarantee than the original — "statistical patterns, not recoverable" was an absolute claim; DP is a bounded-probabilistic claim. The trade is honest visibility for federation benefits. We should only make it if we actually want those benefits.

---

## 10. Decisions (locked 2026-04-18)

| # | Decision | Value |
|---|----------|-------|
| 1 | Approach | **Option B first**; revisit A only if B's false-positive rate proves unacceptable after ~6 months of real usage |
| 2 | Face embedder federation | **Never.** LBP histograms are identity-equivalent; no ε makes aggregation safe. Face stays strictly local-only under any future scheme. |
| 3 | Flagged / suspended session contributions | **Forfeited.** A session that ended in `flagged` or `suspended` status is not eligible to source a proposed prior or (under A) submit a gradient delta. Cheater data must not shape the classifier. |
| 4 | Sybil resistance | **Cardano stake-key signature + per-key rate limit, no minimum stake.** Rate limit is the Sybil defense; no stake threshold avoids two-class membership. Under Option B, Sybil is mostly absorbed by DAO ratification itself. |
| 5 | Opt-in granularity | **One-time global toggle** in settings, with a per-session "federating" indicator so users are reminded. |
| 6 | Curation | New **Sentinel DAO** (distinct from main Alexandria DAO, scoped to cheat-pattern governance). **Anyone proposes**, Sentinel DAO ratifies. Prerequisite: scaffolding the Sentinel DAO itself. |
| 7 | Label distribution publication | **Publish the ratified library**; Sentinel DAO maintains a **private evaluation holdout set** used for false-positive / accuracy measurement but never broadcast. Attackers see training patterns but not the eval criteria. |
| 8 | Retirement of old library / weight versions | **Pin forever.** Every ratified prior-library version stays on Iroh. Stubborn clients can resist a poisoned update by remaining on an earlier version. Storage is cheap relative to the audit trail. |
| 9 | Lifetime ε cap behavior (Option A only) | **Notify + annual Jan 1 reset.** When cumulative ε hits 8.0, dashboard surfaces "budget exhausted until January." Resets every calendar year; contributions resume automatically. Avoids the key-rotation incentive that a permanent cap would create. |

### 10.1 Notes on the decisions

- **Decision 9 vs. decision 4.** An annual reset is compatible with Sybil resistance because the rate limit still applies per-key, per-epoch — the annual reset just lets good-faith long-term contributors keep participating without rotating keys, which in turn keeps Sybil's cost-basis meaningful.
- **Decision 6 — Sentinel DAO scope.** Whether the Sentinel DAO uses the same governance token as the main Alexandria DAO or its own is a governance question we still need to answer before shipping, but it does not block the technical design.
- **Decision 7 — holdout integrity.** Whoever holds the holdout must not also be the one curating priors (split duties). Mechanism TBD — simplest is encrypted-at-rest holdout accessible only to a multi-sig subset of DAO members.

### 10.2 Is 8 contributions per user enough (if Option A is ever turned on)?

Not per-user — federated learning converges on *aggregate* signal. With N users × 8 annual contributions:

| User count | Annual contributions | Expected convergence under DP-SGD ε=1 |
|-----------|---------------------|---------------------------------------|
| ~100 | 800 | Marginal for keystroke AE; insufficient for mouse CNN |
| ~1,000 | 8,000 | AE converges well; CNN dense layers usable |
| ~10,000+ | 80,000+ | Comfortable for both models |

If Option A is revisited and user scale is small, two knobs are available without re-opening the privacy model:
1. Raise per-submission ε to 2.0, keep lifetime cap at 8 → 4 richer submissions per user per year. Faster convergence, same lifetime privacy.
2. Raise lifetime cap to 16 at ε=1 → 16 submissions/year. More signal, roughly Apple FL's per-day budget territory — defensible but less conservative.

These are *future* knobs. Option B, which we're shipping first, is indifferent to user count because it's not federated learning — it's curated labeled data consumed by each client locally.

---

## 11. What "compliance with the mission" looks like, concretely

The mission statement from sentinel.md §Design Principles:

> Privacy-first — All behavioral data is processed entirely on-device. Only numeric scores and categorical flags are stored in the local database.

Option B **complies as stated**: zero per-user data leaves the device, additions are read-only public priors.

Option A **complies only if** we:
1. Explicitly amend guarantee #4 to the (ε, δ)-DP form above
2. Implement DP-SGD with verified ε accounting, not "we added some noise"
3. Refuse to federate the face embedder under any circumstances
4. Publish the privacy budget, noise parameters, and aggregation source code for external review
5. Gate the whole thing behind per-user opt-in with honest language about what DP does and does not guarantee

Option A without any one of those is a mission regression disguised as a feature.

---

## 12. Implementation status (as of 2026-05-16)

| Capability | Status | Reference |
|------------|--------|-----------|
| Sentinel DAO scaffolding (migration 037) | ✅ shipped | `db/schema.rs`, `commands/sentinel_dao.rs` |
| `sentinel_priors` table (migration 038) | ✅ shipped | `commands/sentinel_priors.rs` |
| Holdout refs (migration 039) | ✅ shipped | `commands/sentinel_holdout.rs` |
| Per-snapshot AI score plumbing | ✅ shipped | `useSentinel.ts`, migration 044 |
| Synthetic-data generator | ✅ shipped | `alex synth-sentinel` subcommand, `cli/src/synth/` |
| Offline training kit | ✅ shipped | `tools/sentinel-train/{featurize,train,eval}.py` |
| ONNX paste classifier (bundled v1) | ✅ shipped | `src-tauri/resources/sentinel/paste-v1.onnx` (~4.6 KB / 4741 bytes), embedded via `include_bytes!`, SHA-pinned |
| Backend ML rewrite (tract + candle) | ✅ shipped | `sentinel::paste_classifier` (tract), `sentinel::keystroke_ae` + `sentinel::mouse_cnn` (candle). Frontend only buffers events. |
| Per-user model weights in SQLite | ✅ shipped | Migration 047 added `sentinel_user_models`. Encrypted at rest via sqlcipher. Replaces legacy localStorage. |
| iOS + Android build verified | ✅ iOS / ⏳ Android | `cargo tauri ios build` produces signed `.ipa`. Android blocked by NDK toolchain in CI env (pre-existing). |
| tract ONNX op + size caps | ✅ shipped | `MAX_DAO_MODEL_NODES = 256`, `MAX_DAO_MODEL_BYTES = 50 MiB` in `sentinel::paste_classifier::set_dao_session` |
| Operator-action atomic revert | ✅ shipped | Kill switch + version blocklist both call `paste_classifier::revert_to_bundled()` on activate |
| DAO-signed classifier-weights distribution (migration 045) | ✅ shipped | `ModelKind::PasteClassifierWeights`, `sentinel_get_active_paste_classifier` |
| Three-layer content-addressed re-verification | ✅ shipped | `verify_weights_candidate` checks DB ↔ envelope ↔ eval JSON |
| Resolver timeout + bytes size cap | ✅ shipped | 5 s timeout; 1 MiB envelope/eval; 50 MiB ONNX |
| Mobile gate retired | ✅ shipped | `pasteClassifierDisabled()` removed post-backend rewrite — pure-Rust ML runs everywhere Tauri does |
| Tauri CSP — `'wasm-unsafe-eval'` retired | ✅ shipped | Removed when ML moved off the WebView. No WASM in `script-src`. |
| Operator kill switch (migration 046) | ✅ shipped | `sentinel_set_kill_switch` / `sentinel_get_kill_switch` |
| Version blocklist (migration 046) | ✅ shipped | `sentinel_blocklist_version` / `sentinel_unblocklist_version` |
| Per-signal opt-out toggle | ✅ shipped | `sentinel_paste_classifier_enabled` localStorage flag |
| Operator runbook | ✅ shipped | `docs/sentinel-runbook.md` (5 procedures + incident template) |
| Synthetic-data golden hash regression test | ✅ shipped | `golden_hashes_match_synth_v2` in `cli/src/synth/generators.rs` |
| CI integrity checks | ✅ shipped | model SHA verify + golden-hash test + bundle-size budget |
| Threshold-sig over weights envelope | ⏳ placeholder | `compute_prior_signature` is Blake2b; threshold-sig replacement pending. Mitigated by default-off toggle + kill switch + blocklist + re-verify |
| Real-world holdout evaluation | ⏳ pending | Synthetic-only holdout achieves TPR=1.0 / FPR=0.0; real FPR unmeasured |
| Option A: per-user gradient sharing | ❌ not started | Months of work — DP-SGD, Cardano stake gating, Byzantine-robust agg |
| Option A: ε budget accounting | ❌ not started | Reset cadence locked at annual (decision 9), no implementation |
| Option A: secure aggregation | ❌ not started | Trimmed mean / median-of-means design only, no code |

The Option B path covers the immediate "improve the classifier without leaking user data" goal. Option A is the path forward if real-world holdout numbers show Option B isn't enough — re-opening that workstream requires re-validating §§5–7 first. Until threshold-sig + real-holdout land, the operator safety valves (kill switch, blocklist, per-signal toggle, default-off master toggle) are the authoritative escape hatches for production incidents — see [sentinel-runbook.md](sentinel-runbook.md).
