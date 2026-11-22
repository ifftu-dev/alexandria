# Plugin store: reviewed listings, paid plugins and licences

Status: draft
Updated: 2026-11-20
Triage: needs-triage
Repos: alexandria, alexandria-cloud

Plugins today are either built into the app or installed by hand from a file. This spec adds a store: a reviewed catalogue that course authors browse and install from, in which a plugin's author may charge for it. The course author is the only person who ever pays. Alexandria keeps 30% of every sale. Background: [plugins.md](plugins.md), [enterprise-boundary.md](enterprise-boundary.md), [vision.md](vision.md), [network-profiles.md](network-profiles.md), [course-access-spec.md](course-access-spec.md), [ADR 0007](adr/0007-did-key-never-rotates.md). Vocabulary: [GLOSSARY.md](../GLOSSARY.md).

## Problem Statement

- **There is nowhere to find a plugin.** Discovery over the network is partial. A course author who wants an element type the app does not ship has to be handed a bundle file.
- **A course cannot rely on a community plugin.** On enrolment a learner's device installs built-in plugins only, so a course that uses anything else does not work for its learners.
- **Nobody has looked at a community plugin before it runs.** Plugins are signed by their authors and sandboxed, and nothing else stands behind them.
- **Plugin authors have no way to be paid.** The manifest carries a donation link and nothing more, so there is little reason to build and maintain a good plugin.
- **Nothing in the app or the hosted service takes a payment.** The hosted service raises invoices to organisations; it has no checkout, no payouts and no split.

## Solution

- **A store of reviewed plugins.** Every listing, free or paid, is approved by Alexandria before it appears. A plugin that has not been reviewed can still be installed from a file, as today, and is never installed on a learner's device by a course.
- **The plugin author sets the price.** A plugin is free, or priced on any of three tiers: per use in a course, per course, per account. The author chooses which tiers to offer and what each costs.
- **The course author picks a tier and pays once.** There are no subscriptions. A licence does not expire and covers later versions of the same plugin.
- **Only the course author pays.** A learner is never shown a price or a way to pay, and enrolling costs nothing.
- **Drafts are free.** A course author can install any store plugin and build with it. A licence is needed only to publish.
- **A licence is a signed statement that travels with the course.** The author's app refuses to publish without it. A learner's app checks it, with no network, before it opens the plugin.
- **Alexandria keeps 30% of each sale** and the plugin author receives the rest.
- **Two ways to pay, card first.** The licence is the same whichever way it was paid for. Card payments ship first; paying in ADA follows.

## User Stories

### Course author

1. As a course author, I want to browse a store of plugins from inside the app, so that I can find an element type the app does not ship.
2. As a course author, I want every plugin in the store to have been reviewed, so that I am not the first person to run it.
3. As a course author, I want to see what a plugin does, its screenshots, the capabilities it asks for, the platforms it supports and whether its results can count towards a credential, so that I can judge it before installing.
4. As a course author, I want to see each price a plugin offers before I install it, so that cost is never a surprise at publish time.
5. As a course author, I want to install a paid plugin and use it in a draft without paying, so that I can find out whether it suits my course.
6. As a course author, I want to be told at publish time exactly which plugins need a licence and how many uses each has, so that I know what I owe.
7. As a course author, I want the app to show me the total for each tier on offer and point out the cheapest, so that I do not overpay.
8. As a course author, I want to pay once and never be charged again for the same licence, so that a course's cost is known when I publish it.
9. As a course author, I want a per-use licence to cover one element wherever it sits in the course, so that moving or rebuilding an element does not cost me again.
10. As a course author, I want a per-course licence to cover every use in that course, including ones I add later, so that I can keep editing.
11. As a course author, I want a per-account licence to cover every course I own, now and later, so that I can build on a plugin without counting.
12. As a course author, I want what I already paid for narrower licences taken off the price of a wider one, so that starting small is not punished.
13. As a course author, I want my licences on every device I have paired, and recoverable if I lose a device, so that I do not pay twice.
14. As a course author, I want a licence to keep working when the plugin is updated, repriced or withdrawn from sale, so that a published course does not break.
15. As a course author, I want a course that pinned a free version of a plugin to stay free if a later version becomes paid, so that a price change cannot hold my course to ransom.
16. As a course author, I want a co-instructor to be able to publish a course I licensed, so that a licence does not tie the course to my device.
17. As a course author, I want the store not to learn which course a licence is for, so that buying a licence does not reveal a private course.
18. As a course author, I want a refund when a plugin I paid for is removed for abuse, so that I do not carry the cost of someone else's misconduct.
19. As a course author on a phone, I want to see the licences I hold and be told plainly that buying happens on a computer, so that I am not left at a dead end.

### Plugin author

20. As a plugin author, I want to submit my plugin to the store from the app, so that course authors can find it.
21. As a plugin author, I want to list my plugin for free with no payment set-up at all, so that giving it away is easy.
22. As a plugin author, I want to price any of the three tiers and leave the others off, so that the pricing fits how my plugin is used.
23. As a plugin author, I want to change my prices and have the change apply to future sales only, so that I can adjust without breaking faith with past buyers.
24. As a plugin author, I want to know what the review checks and to be given the reason when a submission is refused, so that I can fix it.
25. As a plugin author, I want each new version reviewed and added to my existing listing, so that buyers get updates under the licence they hold.
26. As a plugin author, I want to receive 70% of every sale, with the split shown on each sale, so that I can check what I am paid.
27. As a plugin author, I want to see my sales, earnings and payouts in the app, so that I do not need a separate account to follow them.
28. As a plugin author, I want to withdraw my plugin from sale without breaking the courses that use it, so that I can stop selling responsibly.
29. As a plugin author, I want the app to refuse to open my paid plugin in a published course that holds no licence, so that being paid does not depend on goodwill alone.

### Learner

30. As a learner, I want never to be asked to pay for anything in a course, so that learning stays free.
31. As a learner, I want the plugins a course needs installed for me when I enrol, so that the course works.
32. As a learner, I want my app to install only plugins that were reviewed or are built in, so that enrolling in a course cannot put unreviewed code on my device.
33. As a learner, I want to be told before I enrol when part of a course is unavailable because its author has not licensed a plugin, so that I can decide with the facts.
34. As a learner, I want an unlicensed element to say why it is unavailable and that the course's author has to fix it, so that I do not think my app is broken.
35. As a learner, I want the rest of a course to work when one element is unavailable, so that one author's lapse costs me as little as possible.
36. As a learner, I want licence checks to work with no network and never to fail because a service is down, so that I can study offline.
37. As a learner, I want what I have already completed and earned to stay mine whatever later happens to a plugin's licence, so that my record does not depend on someone else's payment.

### Store reviewer and operator

38. As a reviewer, I want a queue of submitted versions showing the bundle, its declared capabilities and its changes since the last approved version, so that I can review an update quickly.
39. As a reviewer, I want to approve or refuse with a reason the author sees, so that the decision is accountable.
40. As an operator, I want to withdraw an approval and have apps stop installing that version, so that a harmful plugin can be stopped.
41. As an operator, I want a refund or a chargeback to revoke the licence it paid for, so that a licence never outlives its payment.
42. As an operator, I want every sale, split, refund and payout in an append-only ledger, so that what authors are owed can be reconciled at any time.
43. As an operator, I want a webhook delivered twice to produce one licence and one ledger entry, so that a retry never double-pays an author.

### Maintainer

44. As a maintainer, I want the listing, approval, licence and revocation formats defined once in the permissively licensed verification crate, so that the app and the hosted service agree byte for byte.
45. As a maintainer, I want the rule that decides whether a course's plugins are covered to be one pure function, so that the publish check and the learner's check cannot disagree.
46. As a maintainer, I want a licence to be independent of how it was paid for, so that adding a payment rail changes nothing in the app's checks.
47. As a maintainer, I want the store's key and address read from the network profile, so that no store coordinates are written into code.
48. As a maintainer, I want the app to contain no payment details and no personal details of buyers or sellers, so that the store adds no new sensitive data to a device.
49. As a maintainer, I want Alexandria's own plugins never to be sold, so that nothing the app ships is withheld from anyone.
50. As a maintainer, I want each phase shippable alone, so that a reviewed free store does not wait on payments.

## Implementation Decisions

### Vocabulary

New terms, to be added to the glossary when the work starts.

- **Plugin store**: the reviewed catalogue of plugins that course authors browse, install from and buy licences in.
- **Store listing**: the store's signed entry for one plugin: what it is, who wrote it, which versions are approved and what each tier costs.
- **Version approval**: the store's signed statement that one exact plugin version passed review.
- **Plugin licence**: the store's signed statement that a named course author may use a named plugin within a scope.
- **Licence scope**: `use`, `course` or `account`.
- **Paid plugin**: a plugin version whose manifest says it needs a licence.
- **Store key**: the key the store signs listings, approvals, licences and revocations with.
- **Store share**: the 30% of a sale that Alexandria keeps.

### What the store is

- The store has two halves. The app browses, installs, buys and checks. A new Store service in the hosted service receives submissions, holds listings and bundles, takes payments, pays authors and signs.
- The Store service's address and the store key are part of the network profile. The app reads both from there.
- Everything the app trusts from the store is signed with the store key and checked on the device. The Store service is a carrier for signed statements; the app never takes its unsigned word for an approval or a licence.
- Built-in plugins appear in the store as included with the app. They are free and need no licence.
- **Alexandria's own plugins are never paid.** Only third-party authors sell in the store.
- A plugin outside the store can still be installed from a file by its user. A course never causes it to be installed on a learner's device.

### Submission and review

- A plugin author submits a bundle from the app, with the listing text, screenshots and prices. The submission is signed with the author's key, which is the key the manifest already names.
- Review is per version, because it is the bytes that are reviewed. The reviewer sees the bundle, the capabilities it declares and the changes since the last approved version.
- Review checks that the bundle is complete and matches its manifest, that the listing describes what the plugin does, that the declared capabilities are ones it needs, and that it does not imitate another plugin or author. The written checklist is settled in the first ticket of phase 1 and published for authors.
- Approval produces a version approval and an updated store listing, both signed with the store key. Refusal produces a reason the author sees.
- The store keeps serving every approved bundle for as long as it runs, including versions and plugins no longer on sale.
- **Withdrawal by the author** stops new installs from the store and new sales. Existing licences stay valid and courses that use the plugin keep working.
- **Withdrawal by the store**, for abuse, revokes the version approval. Apps stop installing that version and disable an installed copy, saying why. Licences bought for a plugin removed this way are refunded.

### Pricing

- A plugin is free unless its author prices at least one tier. The author may price any subset of the three tiers; a tier with no price is not offered.
- Where more than one tier is priced, per use may not exceed per course, and per course may not exceed per account.
- Prices are whole amounts in one list currency, exclusive of tax. The list currency and the minimum price are fixed when the payment processor is chosen; the minimum must leave the store share above the processor's fixed fee.
- A price change is a new signed store listing and applies to sales made after it. It never changes a licence already issued.
- **Whether a plugin needs a licence is a property of the version, stated in its manifest**, as a new optional field covered by the plugin's hash and the author's signature. A learner's app therefore knows from the bundle alone, with no network, that a licence is required.
- A version without that field is free for good. A course that pinned it stays free whatever later versions do.
- A paid plugin may depend on built-in plugins, free store plugins and other plugins by the same author. The licence is for the plugin an element names; its dependencies need none of their own.

### Licences and scopes

A licence names the plugin by its id, so it covers every version of that plugin, including later ones. It names the course author's DID, its scope, when it was issued and an opaque order reference. It carries no price and no payment details.

| Scope | What it covers | Bound to |
|---|---|---|
| `use` | One element using the plugin in one course | The course |
| `course` | Every element using the plugin in one course | The course |
| `account` | Every course the licensee owns | The licensee |

- **A use licence is counted, not pinned to an element.** A course holding three use licences for a plugin may use it in up to three elements at a time, whichever they are.
- **An account licence follows the person.** It covers each course the licensee owns. Until course staff exist, the owner is the author who signs the course. If a course passes to an owner who holds no licence, its next version needs one.
- A use or course licence covers the course whoever publishes it, so a co-instructor can publish a course its owner licensed.
- **The store does not learn which course.** A use or course licence binds to a commitment to the course id, made by the buyer's app from the course id and a random value that travels with the licence. Anyone holding the course can check it; the store sees only the commitment.
- Licences are bought once and do not expire. There are no subscriptions and no renewals.
- **Upgrading credits what was paid.** The price of a course licence is reduced by what was paid for use licences of the same plugin in that course. The price of an account licence is reduced by what the buyer paid for use and course licences of that plugin.
- A buyer's licences are stored on their device, carried to their paired devices by the existing device sync, and can be fetched again from the Store service by a request signed with the buyer's key.

### The coverage rule

- One pure function decides whether a course version's paid plugins are covered. Its inputs are the plugin elements of the course document, the manifests of the plugins they name, the course's owner, the licences presented, the store keys of the network profile and the newest revocation list the device holds. Its output is, for each paid plugin, covered or the shortfall.
- A paid plugin used in `n` elements is covered by a valid account licence held by the owner, or a valid course licence for the course, or at least `n` valid use licences for the course.
- The function lives in the verification crate with no I/O. The publish check and the learner's check both call it.

### Publishing

- A draft needs no licence. Any installed store plugin can be placed, configured and previewed in a draft.
- Publishing runs the coverage rule. If anything is short, publishing stops and the app lists each plugin, its number of uses, the tiers on offer with their totals after credit, and the cheapest way to cover the course.
- **Licences are not part of the signed course document.** They are signed by the store and need no second signature. They travel with the course: with the catalog announcement for a public course, over the private channel for a private one. Buying or upgrading a licence therefore never changes a course version's hash, and credentials bound to that hash are unaffected.
- Version approvals for the plugins a course uses travel with it in the same way.

### On the learner's device

- **Install on enrolment.** The enrolment pre-flight installs built-in plugins, as today, and store plugins whose version approval checks out. Bundles are fetched by hash from the Store service and from peers that hold the course. A plugin that is neither built in nor approved is not installed, and the pre-flight says so.
- **Check on open.** Before opening a paid plugin's element, the app runs the coverage rule against the licences that came with the course. With no licence, the element is shown as unavailable, with the reason and a statement that the course's author has to license the plugin.
- A learner is never shown a price, a checkout or a link to one.
- The pre-flight warns before enrolment when elements of a course are unavailable for this reason.
- An unavailable element cannot be completed. Everything else in the course works. Progress, completions and credentials already recorded are never withdrawn because of a licence.
- **The check never needs the network.** Signatures are checked against the store keys in the network profile. Revocations are a signed list the app fetches when it can and keeps; when the Store service cannot be reached, the app uses the list it has and blocks nothing on that account.

### Refunds and revocation

- A refund or a chargeback revokes the licence it paid for. Revoked licences and withdrawn version approvals are published as one signed, numbered revocation list.
- A buyer may ask for a refund within 14 days of purchase. Refunds are decided by the store operator.
- A refund reverses both shares of the sale in the ledger.

### Paying by card

- The processor must split a payment between the store and a third-party seller, onboard and pay out sellers in many countries, and handle sales tax. Choosing it is the first ticket of phase 3.
- **Checkout happens in the browser, on the processor's page.** The app sends the Store service an order signed with the buyer's key, opens the checkout page and waits. Card details never enter the app or the Store service.
- When the processor confirms payment, the Store service issues the licence. The confirmation is idempotent on the order: delivered twice, it yields one licence and one ledger entry.
- **A plugin author who charges must complete payout set-up with the processor** before a paid listing can be approved. The Store service keeps the link between the author's DID and the processor's account reference. Identity documents stay with the processor.
- **The split.** The store share is 30% of the tax-exclusive price, rounded down to the currency's smallest unit. The author receives the remainder, so the two always sum to the price and the author never receives less than 70%. Processing fees are paid out of the store share.
- Every order, payment, split, refund and payout is an append-only ledger entry. Corrections are new entries. The metered-events table in the hosted service is the prior art.
- A plugin author sees sales, earnings and payouts in the app, fetched by a request signed with the author's key.

### Paying in ADA

- The licence format and every check in the app are unchanged. Only the way an order is paid differs.
- The price is quoted in ADA from the list price for a short, stated time. The buyer's wallet sends one transaction with two outputs, 70% to the address the plugin author registered with the store and 30% to the treasury address, carrying the order reference.
- The Store service watches for the transaction, and issues the licence once it is confirmed.
- This rail is built after card payments and after the network it runs on carries real value.

### Mobile

- Buying happens on desktop. The iOS and Android apps show the licences a person holds and what a course is missing, and offer no purchase and no link to one. This keeps the app within the stores' rules on selling digital goods. It is revisited when those rules or the app's distribution change.

### Credentials

- Being in the store does not make a grader eligible to issue credentials. That still requires the exact bundled manifest and grader bytes, until the policy-backed community grader path exists.
- A listing for a graded plugin states plainly that its results do not count towards a credential.
- A version approval is a statement about review for the store. It is not an input to credential authority.

### Enterprise boundary

Rows to add to the worked examples when the work lands:

| Feature | Verdict |
|---|---|
| Browsing the store, installing from it, buying a licence | Core |
| Checking a version approval and a plugin licence | Core, in the permissively licensed verification crate |
| Submitting a plugin and seeing its sales | Core |
| The Store service: submissions, review, listings, payments, payouts, the ledger | Hosted service |

- The test in [enterprise-boundary.md](enterprise-boundary.md) puts the client in core: an individual with no employer wants to find, buy and sell plugins.
- A licence check in core is new and needs stating. It gates a third party's work that a third party chose to charge for. It never gates anything the app ships, which is why Alexandria's own plugins are never paid.
- The app is open source and a modified build can skip the check. The check binds everyone using an unmodified app, and the rule that learners' devices install only approved versions is what gives it force. Nothing stronger is claimed to plugin authors.

### Changes to standing documents

- [vision.md](vision.md) tells educators there are no platform fees and no revenue splits, and names two sources of revenue. Both stay true of teaching: creating and publishing a course is free, and a learner never pays. The store adds a third source of revenue, taken from sales that a plugin author chose to make. The vision is reworded to say exactly that when phase 3 ships.
- [plugins.md](plugins.md) gains the manifest field, the store flows and the learner-side install rule as each phase lands.

### Sequence

Each phase ships on its own.

1. **A reviewed free store.** The listing, approval and revocation formats; the store key and address in the network profile; submission, review and bundle serving in the Store service; the store screen and install in the app; composer choice of store plugins; install on enrolment for approved plugins.
2. **Licences without money.** The manifest field; the licence format and the coverage rule; the publish check; the learner's check and the unavailable element; licences travelling with a course and across paired devices; revocation. Licences are issued by an operator command.
3. **Card payments.** Processor choice; payout set-up for authors; orders, checkout and confirmation; the ledger and the split; upgrade credit; refunds; the author's sales view; the vision wording.
4. **ADA payments.**

Before phase 3 can start, three things are needed that only the project's owner can supply: the legal entity that operates the store and its tax position; the terms that plugin authors and buyers accept, including what licence a plugin's own code must carry given that the app is AGPL; and an account with the chosen processor.

## Testing Decisions

A good test here drives what a person or another device can observe: a course and its licences go in and a publish or a refusal comes out; an element opens or is shown as unavailable; a payment confirmation goes in and a licence and a ledger entry come out. Tests do not assert on message layouts, table shapes or how keys are stored.

Four seams:

1. **Verification crate vectors (no I/O).** Fixed vectors for the store listing, version approval, plugin licence and revocation list, and a table of cases for the coverage rule: each scope; too few use licences; a licence for another course; a licence for another plugin; an account licence held by someone who is not the owner; a forged signature; a key the profile does not list; a revoked licence; a free version of a plugin whose later version is paid; a paid plugin with dependencies. Prior art: the completion endorsement vectors.
2. **Two-party seam (Rust, in-memory databases, real cryptography).** An author profile and a learner profile exchange the production messages as bytes. Covers: publish refused while short and allowed once covered; licences and approvals reaching the learner with a public course and with a private one; install on enrolment of an approved plugin and refusal of an unapproved one; the unavailable element; a co-instructor publishing a licensed course; an upgrade that leaves the course version's hash unchanged; a revocation list arriving and taking effect; the Store service unreachable and nothing blocked; licences reaching a paired device. Prior art: the guardian link and settings sync end-to-end tests.
3. **Store service (integration tests against Postgres, with a fake processor).** Submission, approval and refusal; signed output checked against the crate's vectors; an order paid once and confirmed twice; the split summing to the price at awkward amounts; upgrade credit; a price change leaving earlier sales alone; refund and chargeback reversing both shares and revoking; an author's sales view refused to anyone else's key. Prior art: the idempotency and invoice tests.
4. **Desktop interface seam (browser harness, Chromium and WebKit, fake backend).** The store screen and a listing with each mix of tiers; the publish dialog with its totals and cheapest option; the learner's pre-flight warning and unavailable element, with no price or purchase control anywhere on a learner's screens; the mobile layouts with no purchase control. Prior art: the desktop interface checks with fixtures and a fake backend.

Checks that stay manual and are recorded with each phase: a purchase end to end in the processor's test mode, including a refund; a payout to a test seller; and, for phase 4, a purchase on the test network.

## Out of Scope

- Learners paying for anything, and paid courses.
- Subscriptions, rentals, time-limited licences and pricing by number of learners.
- Coupons, bundles, sales and regional pricing.
- Licences held by an organisation for its authors, and volume pricing.
- Ratings, reviews and ranking of listings.
- Stores run by anyone other than Alexandria, and more than one store in a network profile.
- Protection against a modified app, and any encryption of plugin bundles.
- Making community graders eligible to issue credentials.
- Buying from the iOS and Android apps.
- Selling plugins that are not learning elements.

## Further Notes

- **Decisions taken by the owner.** Both payment rails with card first; the author prices any subset of tiers and the buyer picks; the licence is checked at publish and when a learner opens the plugin; every listing is reviewed, free or paid.
- **Defaults chosen in drafting, to confirm at triage.** "Per use" means per element that uses the plugin, counted and not pinned. A licence covers all later versions of the plugin. Processing fees come out of the store share. The store is given a commitment and not the course id. Upgrades are credited. Refunds are available for 14 days. Buying is desktop only. A paid plugin's dependencies need no licence. Alexandria's own plugins are never paid. The Store service is a module of the hosted service and not a new repository.
- **Why the manifest says a version is paid.** A learner's app must decide offline whether to look for a licence. If that fact lived only in the store listing, an author could publish a course without it and an offline learner's app could not tell. In the manifest it is covered by the plugin's hash, and removing it produces a different, unapproved plugin that no learner's device will install.
- **What a plugin author is and is not promised.** An unmodified app will not publish or open their paid plugin without a licence, and will not install a re-signed copy on a learner's device. Someone who modifies the app can do both. The store's review, the reviewed-only install rule and the convenience of the honest path are the protection, as they are for most software sold without a server.
- **What the store learns.** That a DID bought a licence for a plugin, and, for a use or course licence, a commitment it cannot open. It learns the plugin author's DID and the processor's reference for them. It does not learn which course, who enrolled or what any learner did.
- **A learner can meet an unavailable element through no fault of their own.** This is the cost of checking on the learner's device, accepted when that option was chosen. The pre-flight warning and the wording of the element are there to make the cause and the remedy plain.

## Notes

- 2026-11-20: Spec drafted from the owner's requirements and four decisions. Drafting defaults await confirmation. No code yet.
