# Blacksmith CI runners: fit, cost and migration path

Status: research snapshot; prices, limits and vendor claims reflect the research date and change often.

**Researched:** 2026-11-03
**Scope:** Whether the five Alexandria repositories should move GitHub Actions
jobs to Blacksmith runners, and what that would take. Primary sources only:
Blacksmith docs, pricing page and GitHub App metadata; GitHub docs. Usage
figures come from the GitHub Actions API for the 30 days 2026-10-04 to
2026-11-03. Nothing was installed, transferred or changed.

## Recommendation

**No-go now, on cost. Revisit as a partial move only if CI wall-clock time
becomes the bottleneck.** Three of the five repos are public, so GitHub-hosted
standard runners already cost nothing and give them 4 vCPU. The two private repos
use about $7.81/month of GitHub's $12 included compute. Blacksmith would turn a
$0 bill into roughly $30–$200/month, depending on scope, for vendor-claimed speed.
It would also first require moving every repo from the `ifftu-dev` personal
account into a GitHub organization. If speed matters later, the case is narrow:
move only the long, secret-free Linux PR jobs (`Lint & Test`, `Android grader`),
and keep signing, release and macOS jobs on GitHub-hosted runners.

## 1. Does Blacksmith require a GitHub organization?

**Yes.** The Blacksmith quickstart says, verbatim: *"Blacksmith is limited to
GitHub organizations and not available for personal repositories."*
([quickstart](https://docs.blacksmith.sh/introduction/quickstart)).

The mechanism matches. The `blacksmith-sh` GitHub App requests the
**organization** permission `organization_self_hosted_runners: write`. Its full
permission set, from `gh api apps/blacksmith-sh`, is: actions, checks, contents,
pull_requests and workflows at write; metadata, issues, members and emails at
read. Blacksmith's FAQ adds: *"Blacksmith relies on GitHub's org level runner
registration API"* ([instance types FAQ](https://docs.blacksmith.sh/blacksmith-runners/overview),
[GitHub App](https://docs.blacksmith.sh/blacksmith-administration/github-app)).

Today all repos sit under `ifftu-dev`, which `gh api users/ifftu-dev` reports as
`"type": "User"`. Blacksmith cannot run any job for these repos until they move
into an organization.

Not confirmed: no Blacksmith doc says whether a **GitHub Free** organization is
enough. Nothing found suggests a paid GitHub plan is needed.

## 2. Getting the repos into an organization

### Current state (verified with `gh`, 2026-11-03)

| Repo | Visibility | Actions secrets | Environments | Releases | Rulesets / protection |
|---|---|---|---|---|---|
| `alexandria` | public | 15 | none | 28 | none |
| `alexandria-cloud` | private | 0 | none | 1 | unavailable on Free private |
| `alexandria-relay` | public | 1 | `production` | 0 | none |
| `alexandria-monitoring` | private | 1 | none | 0 | unavailable on Free private |
| `alexandria-website` | public | 0 | none | 0 | none |

None of these repos uses GitHub Pages, webhooks or deploy keys (the API returned
404 or an empty list). The account also owns two other public repos, `ifftu`
and `iroh-live`; decide whether they move too.

### Three ways to get there

1. **Keep the `ifftu-dev` name (recommended if you go ahead).** GitHub documents
   this: rename the personal account, create an organization under the original
   name, then move the repos into it with "Move work to an organization"
   ([moving your work](https://docs.github.com/en/account-and-profile/how-tos/account-management/moving-your-work-to-an-organization),
   [creating an org](https://docs.github.com/en/organizations/collaborating-with-groups-in-organizations/creating-a-new-organization-from-scratch)).
   The final URLs are the same as today's (`github.com/ifftu-dev/<repo>`), so the
   updater endpoint, crate metadata and website links need no changes. Risks: the
   old username is claimable by anyone once it is freed ([username changes](https://docs.github.com/en/account-and-profile/concepts/username-changes)),
   so create the org straight after the rename. Expect a short window in which
   `ifftu-dev/<repo>` URLs fail, between creating the org and finishing the transfers.
2. **New organization with a new name, then transfer.** This is simpler, but every
   URL changes and you depend on GitHub's redirects (below). It also needs the code
   edits listed under "Hard-coded owner references".
3. **Convert the user account in place.** Avoid this. On docs.github.com the
   conversion how-to now 301-redirects to the GitHub Enterprise Server 3.20
   version, so it is unclear whether github.com still offers it. The documented
   side effects are severe: you can no longer sign in to the account, GitHub Apps
   installed on it are uninstalled, commits and comments are unlinked, and
   *"GitHub Actions is not automatically enabled on the account after converting"*
   ([personal account reference](https://docs.github.com/en/account-and-profile/reference/personal-account-reference#side-effects-of-converting-an-account-to-an-organization)).

### What a transfer keeps and redirects (GitHub docs)

From [Transferring a repository](https://docs.github.com/en/repositories/creating-and-managing-repositories/transferring-a-repository):

- **Kept:** *"issues, pull requests, wiki, stars, and watchers are also
  transferred. If the transferred repository contains webhooks, services,
  secrets, or deploy keys, they will remain associated."* Commit history and
  contributions are preserved. Forks stay attached.
- **Redirected:** *"All links to the previous repository location are
  automatically redirected"*, and `git clone`/`fetch`/`push` follow the redirect.
  GitHub still recommends `git remote set-url origin NEW_URL`. Warning: *"If you
  create a new repository or fork at the previous repository location, the
  redirects ... will be permanently deleted."*
- **Not redirected:** GitHub Pages sites. Not relevant here, since no repo uses Pages.
- **Issue assignees:** when moving from a personal account to an org, *"issues
  assigned to members in the organization remain intact, and all other issue
  assignees are cleared"*.
- **Retired names:** a repo with *"more than 100 clones or more than 100 uses of
  GitHub Actions in the week prior to the transfer"* has its old
  `OWNER/REPOSITORY-NAME` retired. `alexandria` likely qualifies, given about 1,240
  CI jobs a month. Retirement protects the redirect because nobody can recreate the
  old name.
- **Private repos on a Free org** lose protected branches and Pages. That changes
  nothing here, because neither is available to the private repos today.
- **Org defaults apply:** *"the organization's default repository permission
  settings and default membership privileges will apply"*.
- **REST API:** clients must follow 301s ([REST best practices](https://docs.github.com/en/rest/using-the-rest-api/best-practices-for-using-the-rest-api#follow-redirects)).

**Not confirmed by GitHub docs, so check after any transfer:**

- Environments, including `alexandria-relay`'s `production` environment and any
  environment secrets.
- Actions variables. There are none today.
- Per-repo Actions settings, such as fork-PR approval policy and workflow permissions.
- Third-party GitHub App installations. Netlify deploys `website` (it has a
  `netlify.toml`). An app installed on a personal account does not cover org repos,
  so expect to reinstall and relink it on the org.
- Whether `releases/latest/download/...` asset URLs redirect. The transfer doc only
  makes the general "all links" claim. Tauri's updater fetches over HTTP and should
  follow a redirect, but this was not tested.

### Hard-coded owner references (only matter for option 2)

`git grep ifftu-dev` finds these load-bearing references:

- `src-tauri/tauri.conf.json` (updater `endpoints`) and
  `src-tauri/src/commands/updater.rs`. Both point at
  `https://github.com/ifftu-dev/alexandria/releases/latest/download/latest.json`.
  Already-installed apps keep the old URL forever, so they depend on the redirect.
- `src-tauri/Cargo.toml` and `crates/alexandria-verify/Cargo.toml` `repository`.
  crates.io shows `alexandria-verify` 0.4.0 with
  `repository = https://github.com/ifftu-dev/alexandria`. That only updates on the
  next publish.
- `cardano/governance/aiken.toml` (`user`).
- In the website repo: `composables/useDownload.ts` calls
  `api.github.com/repos/ifftu-dev/alexandria/releases` and links the repo.
- READMEs and docs in every repo. These are cosmetic and redirects cover them.

### Repos that stay private

The private repos stay private after a move. GitHub Free for organizations has the
same included compute as GitHub Free, $12/month
([Actions billing](https://docs.github.com/en/billing/concepts/product-billing/github-actions#included-compute-usage-and-storage)).

## 3. Blacksmith setup facts

**Install.** Sign in at `app.blacksmith.sh` and install the `blacksmith-sh` GitHub
App on the org, either on all repos or on selected ones
([quickstart](https://docs.blacksmith.sh/introduction/quickstart)). Install it on
**every** repo that uses a `blacksmith-*` label. Otherwise, per Blacksmith, runners
registered at org level can be "adopted" by jobs from repos the app cannot see, and
jobs queue ([FAQ](https://docs.blacksmith.sh/blacksmith-runners/overview)). A
Migration Wizard opens a PR that rewrites `runs-on`.

**Runner labels** ([instance types](https://docs.blacksmith.sh/blacksmith-runners/overview)):

| Current label | Blacksmith label | Note |
|---|---|---|
| `ubuntu-latest`, `ubuntu-24.04` | `blacksmith-{2,4,8,16,32}vcpu-ubuntu-2404` | `2604` also offered |
| `ubuntu-22.04` | `blacksmith-*-ubuntu-2204` | **Deprecated** by Blacksmith |
| `ubuntu-22.04-arm` | `blacksmith-*-ubuntu-2404-arm` | `2204-arm` deprecated |
| `windows-latest` | `blacksmith-{2..32}vcpu-windows-2025` | **Public beta**; ships VS Build Tools 2022, not the full IDE; no Linux containers |
| `macos-latest`, `macos-15` | `blacksmith-{6,12}vcpu-macos-{latest,15,26,27}` | Apple M4; smallest size is 6 vCPU |

Implications for this project:

- `Lint & Test`, `Android grader` and the Linux desktop builds pin `ubuntu-22.04`.
  The Blacksmith 22.04 images are deprecated, so moving would push these jobs to
  24.04. For the Linux release artifacts, that likely raises the glibc floor. This
  is an inference: the workflows do not say why they pin 22.04.
- `Android grader` needs `/dev/kvm`. Blacksmith states nested virtualization works
  on x64 Linux runners but not on ARM.
- Isolation: Linux and Windows jobs run in *"ephemeral Firecracker microVMs"*. No
  Blacksmith doc read for this note describes how macOS jobs are isolated.

**Caching.**

- **Actions cache:** the official GitHub cache actions and *"popular third-party
  cache actions"* transparently use Blacksmith's colocated cache. It is free, with
  25 GB per repo per week and 7-day LRU eviction
  ([actions cache](https://docs.blacksmith.sh/blacksmith-caching/dependencies-actions)).
  Not confirmed: `Swatinem/rust-cache`, which these workflows use, is not named.
  Blacksmith does say *"the Rust `sccache` ... [is] still redirected to GitHub's
  backend."*
- **Archived actions:** `useblacksmith/cache` and `useblacksmith/rust-cache` are
  archived; Blacksmith recommends upstream actions instead.
- **Sticky disks:** `useblacksmith/stickydisk` persists a directory, such as a
  Cargo `target/`, across jobs. It costs $0.50/GB/month, allows up to 5 per job,
  and evicts after 7 days unused. By default, `pull_request` jobs can commit to a
  sticky disk; a dashboard toggle restricts commits to default-branch
  push/schedule/dispatch jobs ([sticky disks](https://docs.blacksmith.sh/blacksmith-caching/dependencies-sticky-disks)).
- **Other caches:** Docker layer caching (`useblacksmith/setup-docker-builder`,
  `useblacksmith/build-push-action`) and git-mirror checkout
  (`useblacksmith/checkout`) are both backed by sticky disks, at $0.50/GB/month
  ([docker builds](https://docs.blacksmith.sh/blacksmith-caching/docker-builds),
  [git checkout](https://docs.blacksmith.sh/blacksmith-caching/git-checkout-caching)).
  The cloud, relay and monitoring CI build Docker images, so these would apply there.

**Concurrency.** Blacksmith states: *"Blacksmith doesn't limit how many jobs or
vCPUs you run at once"*, and that the macOS sizes are *"available on demand"*.
`mobile-shared.yml` records one iOS release that sat queued for 24 hours on
GitHub's ARM macOS pool. That is the only operational pain in this project that
Blacksmith claims to address directly.

## 4. Prices (per minute, USD)

| Runner | GitHub-hosted, private repo | GitHub-hosted, public repo | Blacksmith |
|---|---|---|---|
| Linux x64, 2 vCPU | $0.006 | **free** (and 4 vCPU) | $0.004 |
| Linux x64, 4 vCPU | $0.012 (larger runner) | **free** (standard size) | $0.008 |
| Linux arm64, 2 vCPU | $0.005 | **free** (and 4 vCPU) | $0.0025 |
| Windows x64, 2 vCPU | $0.010 | **free** (and 4 vCPU) | $0.008 |
| macOS arm64 | $0.062 (3-core M1) | **free** | $0.08 (6 vCPU M4) |

Sources:

- GitHub rates: [Actions runner pricing](https://docs.github.com/en/billing/reference/actions-runner-pricing).
- Public repos: *"Use of the standard GitHub-hosted runners is free and unlimited
  on public repositories"*. Public-repo `ubuntu-*`, `windows-*` and `*-arm`
  runners are 4 vCPU / 16 GB, and `macos-15`/`macos-latest` is a 3-core M1
  ([choosing the runner](https://docs.github.com/en/actions/how-tos/write-workflows/choose-where-workflows-run/choose-the-runner-for-a-job#standard-github-hosted-runners-for-public-repositories)).
- Blacksmith rates and free tier: [pricing](https://www.blacksmith.sh/pricing) and
  the [instance types FAQ](https://docs.blacksmith.sh/blacksmith-runners/overview).

How Blacksmith bills:

- Rates scale linearly with vCPU.
- The free tier is *"3000 x64 2vCPU minutes for free per month per
  organization"*, which is worth $12. Other runners consume it at a conversion
  rate: ARM ×0.625, Windows ×2, and one 6-vCPU macOS minute counts as 20 x64
  minutes.

Other notes:

- **GitHub included compute.** Free plans include $12/month of compute, roughly
  2,000 Linux minutes. Self-hosted runner usage, which is how GitHub sees
  Blacksmith runners, is listed as free
  ([Actions billing](https://docs.github.com/en/billing/concepts/product-billing/github-actions)).
- **Self-hosted platform charge.** GitHub announced a $0.002/min charge for
  self-hosted runners from 2026-03-01, then *"postponing the announced billing
  change for self-hosted GitHub Actions"*, with no new date. Public repos stay
  exempt either way: *"Standard GitHub-hosted or self-hosted runner usage on public
  repositories will remain free."*
  ([GitHub 2026 pricing changes](https://github.com/resources/insights/2026-pricing-changes-for-github-actions)).
- **Blacksmith's comparison is out of date.** The instance-types page says Blacksmith
  is *"exactly half the cost of GitHub's per minute"*. That was true against GitHub's
  pre-2026 Linux rate; the pricing page now says *"33% cheaper"*.
- **OSS program.** It requires a *"Permissive OSS license"*. The app is
  AGPL-3.0-or-later, so it probably does not qualify. Not confirmed with Blacksmith.

## 5. Cost for this project

**Measured usage, 2026-10-04 to 2026-11-03.** Each job's duration is rounded up to
the minute, as GitHub bills. The data is the jobs API for every run in the window.

| Repo (visibility) | Linux x64 | Linux arm64 | Windows | macOS | Total |
|---|---|---|---|---|---|
| `alexandria` (public) | 9,075 | 625 | 697 | 1,439 | 11,836 |
| `alexandria-relay` (public) | 117 | – | – | – | 117 |
| `alexandria-website` (public) | 159 | – | – | – | 159 |
| `alexandria-cloud` (private) | 1,214 | – | – | – | 1,214 |
| `alexandria-monitoring` (private) | 88 | – | – | – | 88 |

The top `alexandria` jobs by minutes are:

| Job | Minutes | Runs | Average |
|---|---|---|---|
| `Lint & Test` | 4,469 | 139 | 32 min |
| `Android grader` | 1,709 | 140 | 12 min |
| `Mobile Compile Check`, iOS and Android legs | 1,528 | – | – |
| Linux desktop builds (validate and release) | 1,307 | – | – |
| Windows builds | 697 | – | – |
| macOS desktop and iOS release builds | 691 | – | – |

**Today on GitHub: about $0/month.** The public repos cost nothing. The private
repos use 1,302 Linux minutes × $0.006 = $7.81, inside the $12 included. This
could not be checked against the real bill: the billing endpoints
(`/users/ifftu-dev/settings/billing/usage`) need the `user` token scope, which
the `gh` token lacks. Run `gh auth refresh -s user` to confirm.

**On Blacksmith,** with the same usage, after the $12 free tier:

| Scenario | Monthly |
|---|---|
| Everything, 2 vCPU Linux/Windows (half today's public-repo cores), same duration | ≈ $153 |
| Everything, 4 vCPU parity for public repos, same duration | ≈ $197 |
| Same as above, if Blacksmith's "2x faster" claim holds | ≈ $93 |
| Linux x64 + arm64 only, 4 vCPU parity, same duration | ≈ $71 |
| Same as above, if the 2x claim holds | ≈ $30 |
| Private repos only (1,302 min at 2 vCPU = $5.21) | $0, inside the free tier, but also $0 on GitHub today |

Moving macOS costs more per minute than GitHub's paid M1 rate and is free on
GitHub for public repos. On its own it accounts for about $115/month of the
full-move figure.

**Speed claims (all vendor claims, not verified here):**

- 2x faster runners and 4x faster cache downloads (about 400 MB/s versus 90 MB/s).
- Under-3-second VM boot and no queueing
  ([instance types](https://docs.blacksmith.sh/blacksmith-runners/overview)).

Blacksmith's savings figures assume *"an estimated 2x faster runtime"*. Its
comparisons are against GitHub's paid 2-vCPU private-repo runners; the public repos
here already get 4 vCPU free. No benchmark was run.

## 6. Security and trust

**GitHub's position on self-hosted runners.** *"Self-hosted runners should almost
never be used for public repositories on GitHub, because any user can open pull
requests against the repository and compromise the environment"*
([secure use reference](https://docs.github.com/en/actions/reference/security/secure-use)).
GitHub's stated reason is persistence on non-ephemeral machines. It recommends
just-in-time (JIT) runners, which take at most one job.

**Blacksmith's mitigation.** Blacksmith states that it uses only JIT tokens, one
job per microVM, and that *"After a job completes, the microVM is destroyed"*. It
also says jobs never share kernels and holds SOC 2 Type 2
([security](https://www.blacksmith.sh/security)). These are vendor statements; the
SOC 2 report is available on request via its trust center.

**Secrets.**

- The Blacksmith App does not request secrets, variables, environments or
  administration ([GitHub App](https://docs.blacksmith.sh/blacksmith-administration/github-app)).
- However, any job that runs on a Blacksmith runner receives the secrets it
  references, in memory on Blacksmith hardware. The `alexandria` release workflows
  reference Apple signing certificates and API keys, the Tauri updater signing key,
  the Android keystore, the Windows certificate and the Google Play service account.
  Keep those jobs on GitHub-hosted runners unless that trust shift is accepted
  explicitly.

**Fork pull requests.**

- *"With the exception of `GITHUB_TOKEN`, secrets are not passed to the runner when
  a workflow is triggered from a forked repository"*, and the token is read-only
  ([events that trigger workflows](https://docs.github.com/en/actions/reference/workflows-and-actions/events-that-trigger-workflows#workflows-in-forked-repositories)).
- What changes on a public repo is cost and cache exposure. Fork PRs would spend
  paid Blacksmith minutes, where GitHub runs them free.
- Set **"Require approval for all external contributors"**. GitHub warns that the
  first-time-contributor settings can be bypassed after one merged typo fix
  ([Actions settings](https://docs.github.com/en/repositories/managing-your-repositorys-settings-and-features/enabling-features-for-your-repository/managing-github-actions-settings-for-a-repository)).

**Cache poisoning.** Blacksmith scopes Actions-cache entries per branch, as GitHub
does, and that scoping can be switched off. Sticky disks are shared across a repo
and writable by `pull_request` jobs until "Branch Protection" for sticky disks is
enabled; enable it before using them.

**Org-level runner adoption.** Runners are registered at org level. Any repo in the
org that uses a `blacksmith-*` label can receive a Blacksmith runner, so install the
App on exactly the repos that use those labels.

## 7. Migration checklist (if the decision changes)

1. Confirm the real bill with `gh auth refresh -s user`, then
   `gh api /users/ifftu-dev/settings/billing/usage`. Decide what pain is being
   solved: cost (there is none today) or wall-clock time.
2. Choose the org route. Option 1 (rename the user, create the `ifftu-dev` org, move
   work) keeps every URL. Make sure a second owner account exists first.
3. Before moving, export a list of secrets, the `production` environment and its
   protection rules, the Actions settings and the Netlify linkage. GitHub does not
   document whether environments and settings carry over.
4. Move the repos. Then verify: secrets present, the environment intact, Actions
   enabled, the fork-approval policy set to "all external contributors", and Netlify
   relinked.
5. If the org name changed (option 2), update the updater endpoints, the Cargo
   `repository` fields, `useDownload.ts`, `aiken.toml` and the READMEs. Then
   `git remote set-url` every local clone and any worktrees.
6. Fetch the updater `latest.json` through the old URL with `curl -IL` to confirm
   the redirect. Never create a repo at an old location.
7. Sign up at `app.blacksmith.sh` and install the GitHub App on the selected repos
   only.
8. Pilot one job, for example `Lint & Test`, on `blacksmith-4vcpu-ubuntu-2404` in a
   PR. Compare wall-clock time and the Rust cache hit rate with the 32-minute
   GitHub average.
9. Expand only to secret-free Linux PR jobs that show a measured win. Keep these on
   GitHub-hosted runners: release and signing jobs, macOS and iOS jobs, Linux
   release builds that rely on the 22.04 glibc floor, and Windows (Blacksmith's is
   in beta).
10. Enable sticky-disk Branch Protection before adopting sticky disks. Set a spend
    alert in the Blacksmith dashboard.

## Unconfirmed or open

- Actual GitHub Actions spend: the token lacks the `user` scope.
- Whether a GitHub Free organization is enough for Blacksmith; nothing found says
  otherwise.
- Whether "convert user to organization" still exists on github.com: the docs page
  redirects to GitHub Enterprise Server.
- Whether environments, Actions settings and third-party app installations survive a
  transfer, and whether release-asset URLs redirect. GitHub docs make only the
  general redirect claim.
- Whether Blacksmith's transparent cache covers `Swatinem/rust-cache`.
- How Blacksmith isolates macOS jobs.
- Whether the AGPL app could join Blacksmith's OSS program.
- All Blacksmith speed figures. No benchmark was run against this project's jobs.
- Blacksmith has no public changelog: `/changelog` returns 404 on both
  `blacksmith.sh` and `docs.blacksmith.sh`. Dates of the macOS and Windows launches
  and of the 22.04 deprecation were not found.
