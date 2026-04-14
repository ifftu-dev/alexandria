# Alexandria: Free Knowledge, Verified Skills, No Gatekeepers

## A vision for public-interest education infrastructure

---

## The Problem

Among all the resources available on Earth, none are as powerful — or as impactful — as human potential. Our collective survival and advancement depend entirely on how we nurture and empower each other.

Yet access to quality education remains one of the most unevenly distributed resources on the planet. Economic inequality gates who can learn, where they can learn, and whether that learning is recognised. Degrees from prestigious institutions open doors that raw ability alone cannot. Resumes reward pedigree over proof. And the systems meant to democratise knowledge — from MOOCs to open courseware — still ultimately funnel learners toward credentials they must pay for, controlled by institutions they must trust.

The core issue is not a lack of content. It is that the infrastructure for recognising and verifying what people know remains centralised, opaque, and exclusionary.

Even well-intentioned platforms reproduce this dependency. Any education system that relies on servers — hosted databases, cloud APIs, institutional backends — inherits a single point of failure and an implicit trust assumption. Someone runs the infrastructure. Someone pays for hosting. Someone can shut it down.

---

## What Alexandria Does

Alexandria is a free, open-source, decentralized learning application that runs as a native desktop and mobile app. Every user runs a full node — a single binary that contains the entire platform: database, content store, peer-to-peer networking, wallet, and user interface. There are no servers, no Docker containers, and no external databases.

The application runs on macOS, Linux, Windows, iOS, and Android. It provides courses, assessments, classrooms, live tutoring, and APIs — all distributed through a peer-to-peer network (libp2p) so that no single organisation controls access.

But content delivery is only the beginning. Alexandria's real contribution is what happens after someone learns.

### Skills Replace Resumes

Traditional resumes are proxy documents. They summarise claims about education and experience but provide no verifiable evidence of what someone can actually do. Alexandria replaces this with **verifiable, skill-based credentials** — stored locally on the learner's device, optionally minted on a public blockchain (Cardano), and independently verifiable by anyone without relying on the platform.

When a learner demonstrates proficiency (through assessments, projects, and peer-attested evidence), they earn a **SkillProof**: a cryptographic credential that records exactly what they know, at what Bloom's taxonomy level, backed by weighted evidence with confidence scores.

### Reputation Is Earned, Not Declared

Instructors, assessors, and content authors build reputation not by self-promotion but through the **measurable progress of their learners**. If your students demonstrably improve, your reputation reflects that — scoped to the specific skill and level you teach, exposed as a distribution with confidence bounds rather than a single score.

This means a brilliant calculus instructor isn't automatically assumed to be a brilliant statistics instructor. Expertise is granular, evidence-based, and transparent. Reputation snapshots can be anchored on-chain as CIP-68 soulbound tokens for independent verification.

### Assessment Integrity Is Built In

The Sentinel anti-cheat system monitors assessment integrity through multi-signal behavioral fingerprinting: a keystroke autoencoder, a mouse trajectory CNN, and a face embedder (LBP histograms). All three models are hand-written in TypeScript with zero external ML dependencies, trained on-device during a calibration wizard.

Raw behavioral data — keystrokes, mouse movements, video frames — **never leaves the device**. Only derived integrity scores (0.0–1.0) and categorical flags are stored and transmitted. This is enforced by the code architecture, not by policy.

### Governance Belongs to the Qualified

Platform decisions — from curriculum standards to assessment policies — are made through **decentralised governance** where voting power derives from demonstrated expertise, not from wealth, seniority, or title. The governance structure mirrors the knowledge taxonomy: each subject area has its own DAO, elected from its highest-impact contributors.

Elections use 2/3 supermajority voting. Taxonomy updates are committee-gated and ratified via the P2P governance topic. Governance events are broadcast, signed, and verifiable.

### No Single Point of Failure

Course content lives in iroh, a BLAKE3 content-addressed blob store on each device. Credentials can be minted on Cardano (Conway era). Identity is derived from a 24-word BIP-39 mnemonic — the same Ed25519 key serves as Cardano payment key, libp2p peer identity, and message signing key. Peer discovery happens through a private Kademlia DHT with relay-based NAT traversal.

If Alexandria the organisation disappeared tomorrow, every learner's credentials would remain verifiable on-chain, every piece of content would remain in the iroh stores of every node that pinned it, and every reputation record would remain intact.

### Offline-First by Design

Every operation works without network access. The local SQLite database (66 tables), iroh content store, and encrypted vault provide complete functionality offline. Sync is opportunistic — when connectivity returns, nodes exchange updates via GossipSub topics and cross-device sync.

### Mobile Is a First-Class Node

iOS and Android are not thin clients. The mobile app is a fully functional node — same P2P networking, content storage, and wallet as desktop. Multi-device support works via shared BIP-39 mnemonic with encrypted sync. Biometric unlock (Face ID / Touch ID) is supported via platform APIs.

---

## Why This Matters

### For Policymakers

- Portable, verifiable credentials give governments a tool for workforce measurement that doesn't depend on institutional self-reporting.
- Credential inflation is structurally reduced — you can't buy a SkillProof; you have to demonstrate competence with weighted, confidence-scored evidence.
- Public funding can target skills directly rather than routing through institutions as intermediaries.
- The entire system runs without servers — no data centre to regulate, no company to subpoena for learner data.

### For Educators

- Create and publish at zero cost. No platform fees, no hosting charges, no revenue splits.
- Your impact is visible and verifiable. Reputation is computed from learner outcomes, not student ratings or follower counts.
- Shape the platform you teach on. High-impact educators earn governance roles in the subject areas they contribute to.
- Classrooms and live tutoring (video, audio, screenshare) are built into the platform.

### For Employers and Recruiters

- Query for capabilities, not keywords. Search for candidates who have verified proficiency in distributed systems at the "analyze" level — not candidates who listed "distributed systems" on a resume.
- Reduce pedigree bias. Credentials carry evidence of what someone can do, regardless of where they learned it.
- Verify before you interview. Every on-chain credential is independently checkable against a public blockchain.
- Assessment integrity is built in. Sentinel scores surface whether evidence was gathered under monitored conditions.

### For Learners

- You own your credentials. They live on your device. No platform can revoke them. No institution can gate access to your own record.
- You control your privacy. You choose what to mint on-chain and what to keep local.
- You learn for free. Not freemium. Not free-with-ads. Free.
- Your identity is self-sovereign. A 24-word mnemonic is your account — no email, no password recovery service, no OAuth provider.
- Your app works offline. Everything functions without connectivity. Sync happens when you're ready.

---

## How Alexandria Sustains Itself

Alexandria is structured as a non-profit. Learning content, credentials, and reputation data are free — permanently and unconditionally.

Revenue comes from two sources that don't compromise the mission:

1. **Recruitment services and enterprise tools** for the private sector — skill-based candidate discovery, workforce analytics, and integration APIs. These operate through the same query system available to everyone, with the constraint that learner data is never sold and all queries respect learner-controlled privacy settings.

2. **LMS services for academic institutions** — custom deployments with administrative dashboards and institutional integrations. Critically, all credentials generated within institutional deployments remain learner-owned and portable.

The platform also accepts grants, donations, and impact investment — but never investment that conditions access to content, biases governance, or introduces proprietary restrictions.

---

## The Shift

Alexandria is not another MOOC. It is an attempt to rebuild the infrastructure layer beneath education — the layer that determines how learning is recognised, how teaching quality is measured, how expertise governs decisions, and how credentials flow between people and institutions.

By putting a full node on every device — desktop and mobile — and eliminating servers entirely, the platform's properties (credential ownership, censorship resistance, privacy) are guaranteed by architecture, not by policy. There is no server to shut down, no database to seize, no API to throttle.

The bet is simple: **if you make knowledge free and make proof of knowledge verifiable, the systems built on top — hiring, governance, funding, collaboration — get fairer by default.**

---

## Get Involved

Alexandria is open-source, under active development, and looking for contributors across every discipline — engineers, educators, designers, policymakers, researchers, and learners.

- **Reference implementation**: github.com/ifftu-dev/alexandria
- **Relay server**: github.com/ifftu-dev/alexandria-relay
- **Technical specification**: See the Alexandria Protocol Specification (companion document)
- **Author**: Pratyush Pundir
