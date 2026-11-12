# CI runner alternatives to Blacksmith for GitHub Actions

Status: research snapshot; prices, plan terms and vendor status reflect the research date.

**Researched:** 2026-11-03
**Scope:** Drop-in replacements for GitHub-hosted runners (faster and/or cheaper)
other than Blacksmith, judged against the five Alexandria repositories. Blacksmith
itself appears only as a one-row baseline; a separate note covers it in depth
(`docs/blacksmith-ci-research.md`). Primary sources only: vendor docs, pricing
pages and source repositories, plus docs.github.com. Vendor performance numbers
are labelled as vendor claims and were not measured. Research only: nothing was
installed, configured or committed.

## Bottom line

1. **The account type rules out the best-known vendors.** `ifftu-dev` is a GitHub
   personal (User) account. Blacksmith ("Blacksmith is limited to GitHub
   organizations and not available for personal repositories",
   [quickstart](https://docs.blacksmith.sh/introduction/quickstart)), Depot runners,
   WarpBuild and Actuated all require an organization. Namespace, Tenki and RunsOn
   say in their docs that personal accounts work. Ubicloud's source code registers
   runners per repository, so it should work too, but its docs don't say so.
2. **Cost barely matters here; speed does.** Three of the five repos are public,
   and GitHub's standard runners are free and unlimited there, including macOS and
   Windows. The two private repos look to be inside the included $12/month of
   compute (estimate below). The real cost is the app's `Lint & Test` job, which
   took 33–52 minutes in the last five successful CI runs.
3. **Recommended:** a hybrid. Move only the heavy Linux Rust jobs in `alexandria`
   to **Namespace**. It supports personal accounts, gives the Android emulator job
   KVM, and has a Rust cache mode. Keep macOS, Windows and Linux arm64 on
   GitHub-hosted runners, where they cost nothing for a public repo. **Ubicloud**
   is the cheaper Linux-only fallback. **RunsOn** is the cheapest per minute if
   running an AWS account is acceptable and the licence question is settled.

## Baseline: what Alexandria runs and what GitHub charges

Repository visibility (checked with `gh repo view`): `alexandria`,
`alexandria-relay` and `alexandria-website` are **public**; `alexandria-cloud`
and `alexandria-monitoring` are **private**. The owner type is `User`.

Runner labels in use: the app uses `ubuntu-latest`, `ubuntu-22.04`,
`ubuntu-24.04`, `ubuntu-22.04-arm`, `macos-15`, `macos-latest` and
`windows-latest`, plus a matrix in `ci.yml`. The other repos use only
`ubuntu-latest`. Heavy jobs:

- `Lint & Test` (`ubuntu-22.04`) is the Rust/Tauri build and test job, at 33–52 min per run.
- `Android grader (on-device)` (`ubuntu-22.04`) takes about 13 min and runs
  `reactivecircus/android-emulator-runner` after enabling `/dev/kvm` with a udev
  rule. **Any replacement runner for this job must expose KVM.**
- `Mobile Compile Check (iOS)` (`macos-15`) takes 6–13 min.
- The macOS, Windows and Linux arm64 desktop/mobile builds are in the reusable `desktop-shared.yml` / `mobile-shared.yml` workflows.
- `alexandria-cloud` CI uses Postgres 16 `services:` containers.

Caching today is `Swatinem/rust-cache` (17 uses) and `actions/setup-node` npm caching.

GitHub facts (docs.github.com):

- **Public repos are free on standard runners:** "Use of the standard
  GitHub-hosted runners is free and unlimited on public repositories." Public
  Linux and Windows runners get 4 CPU / 16 GB; private ones get 2 CPU / 8 GB. macOS
  arm64 (M1) is 3 CPU / 7 GB on both. Linux arm64 (`ubuntu-22.04-arm`,
  `ubuntu-24.04-arm`) and Windows arm64 are available to public and private
  repos. Source:
  [GitHub-hosted runners reference](https://docs.github.com/en/actions/reference/runners/github-hosted-runners).
- **Private-repo allowance:** "The included compute usage is expressed as a
  monetary amount": $12/month on GitHub Free and $18 on Pro, with 10 GB of cache
  per repository. Self-hosted runner usage "remains free independently and does not
  reduce the included compute usage." Source:
  [Actions billing](https://docs.github.com/en/billing/concepts/product-billing/github-actions).
- **Per-minute prices** ([runner pricing](https://docs.github.com/en/billing/reference/actions-runner-pricing)):

  | Runner | Price |
  |---|---|
  | Linux 2-core x64 | $0.006 |
  | Linux 2-core arm64 | $0.005 |
  | Windows 2-core | $0.010 |
  | macOS 3/4-core | $0.062 |
  | Linux 8-core x64 (larger runner) | $0.022 |
  | Linux 8-core arm64 (larger runner) | $0.014 |
  | macOS 12-core Intel large | $0.077 |
  | macOS M2 Pro xlarge | $0.102 |

- **Larger runners are org/enterprise-only (inferred):** "Larger runners are
  billed for public repositories" ([billing](https://docs.github.com/en/billing/concepts/product-billing/github-actions)).
  The management docs describe only organization and enterprise setup: "Organization
  owners can add a larger runner to an organization and control which repositories
  can use it"
  ([manage larger runners](https://docs.github.com/en/actions/how-tos/manage-runners/larger-runners/manage-larger-runners)).
  No personal-account path is documented, so a personal account presumably can't
  use them.
- **Self-hosted runner fee postponed:** GitHub announced a $0.002/min "cloud
  platform charge" for self-hosted runners, then postponed it: "We're postponing
  the announced billing change for self-hosted GitHub Actions to take time to
  re-evaluate our approach"
  ([changelog](https://github.blog/changelog/2025-12-16-coming-soon-simpler-pricing-and-a-better-experience-for-github-actions/)).
  The current billing page lists self-hosted usage as free. This charge would apply
  to every option below that registers self-hosted runners (all of them) if it
  returns.

### Why personal accounts are a problem

Every vendor here, Namespace and Ubicloud included, works by registering
self-hosted runners with GitHub. On a personal account, self-hosted runners can
only be added per repository: "To add a self-hosted runner to a user repository,
you must be the repository owner"
([add runners](https://docs.github.com/en/actions/how-tos/manage-runners/self-hosted-runners/add-runners)).
Runner groups exist only for organizations and enterprises.

Vendors built on organization runner groups are therefore org-only. Depot words it
as "Due to a GitHub limitation, Depot GitHub Actions runners only support
repositories owned by GitHub organizations"
([Depot runners](https://depot.dev/docs/github-actions/overview)). Vendors that
register runners per repository work on personal accounts.

## Comparison

Prices are list prices per minute. "≈GH" compares with the GitHub price for the
same OS from the baseline above. A public repo pays $0 on standard GitHub runners
regardless.

| Option | Model | Personal account works? | Linux x64 | Linux arm64 | macOS | Windows | KVM (Android emulator) | Status 2026-11-03 |
|---|---|---|---|---|---|---|---|---|
| Blacksmith (baseline) | GitHub App, vendor cloud | **No**: "limited to GitHub organizations" | $0.004 (2 vCPU), 3,000 free min/org | $0.0025 | $0.08 | $0.008 | see other note | Operating |
| **Namespace** | GitHub App, vendor cloud, repo-level runners | **Yes**: "You can install the app on either a personal account or an organization" | 2 vCPU/4 GB $0.002 prepaid / $0.003 overage; 8x16 $0.008 / $0.012 | Yes (AmpereOne); per-minute rate not published separately | 6x14 $0.06 / $0.09 | 2 vCPU $0.004 / $0.006 | **Yes**, linux/amd64 | Operating |
| **Ubicloud** | GitHub App, vendor cloud, open source (AGPL-3.0) | **Probably**: docs silent; source registers per-repo JIT runners | 2 vCPU $0.00125; 8 vCPU $0.005 (premium $0.002 / $0.008) | 2 vCPU $0.00125 | No | No | Not documented | Operating |
| **RunsOn** | Self-hosted in your AWS (CloudFormation/Terraform) + private GitHub App | **Yes**: prerequisite is "A GitHub organization (or personal account)" | AWS spot, e.g. 2 vCPU ≈$0.0009, 8 vCPU ≈$0.0022, + licence | 2 vCPU ≈$0.0009 | **No** | Yes | **Yes** (`nested-virt` label, x64 c8i/m8i/r8i) | Operating |
| Tenki | GitHub App, vendor cloud | **Yes** per its FAQ; setup docs describe orgs | $0.002 per vCPU-min ($0.004 for 2 vCPU) | Not documented | $0.020 per vCPU-min (M4 Pro) | Not documented | Not documented | Operating, newer vendor |
| Depot runners | GitHub App, vendor AWS | **No**: org-only | 2 vCPU $0.006; 8 vCPU $0.024 | same as x64 | $0.08 (M2/M4, 8 vCPU) | 2 vCPU $0.008 | **No** (/dev/kvm not provided) | Operating |
| Depot CI | Separate CI that runs Actions YAML from `.depot/workflows/` | **Yes** | $0.00005/vCPU-s + plan | Yes | **No** | **No** | Yes | Operating; not a drop-in runner |
| WarpBuild | GitHub App, vendor cloud or BYOC | **No**: "Repositories under a personal account cannot receive the runners" | 2 vCPU $0.004 | 2 vCPU $0.003 | $0.08 (M4 Pro 6 vCPU) | 4 vCPU $0.016 | Yes (x64 label) | Operating |
| Actuated | Your own servers, vendor control plane | **No**: "a GitHub organisation is required" | Flat $150/server/month | Yes (plan-dependent) | No | No | n/a | Operating (repo activity Oct 2026) |
| Cirrus Runners | Vendor Mac/Linux fleet | n/a | — | — | — | — | — | **Closed to new customers** since 2026-04-07 |
| BuildJet | — | n/a | — | — | — | — | — | **Shut down** 2026-03-31 |
| Buildkite hosted agents | Own pipeline format | n/a | $0.004/vCPU-min | same | $0.02/vCPU-min | — | — | Operating; **not drop-in** |
| Self-hosted runner / ARC | Your hardware | **Yes**, repo-level only | Your cost | Your cost | Yes (spare Mac) | Yes | Your hardware | GitHub-maintained |
| GitHub larger runners | GitHub | **No path documented** for personal accounts | 8-core $0.022 | 8-core $0.014 | $0.077–$0.102 | 8-core $0.042 | Yes | Operating |

## Options in detail

### Namespace

- **Account and install:** "You can install the app on either a personal account
  or an organization." Runners are repository-level by default, which needs the
  app's `Administration: Read and write` permission. Org-level runners are
  available through support
  ([GitHub Actions docs](https://namespace.so/docs/solutions/github-actions)).
- **Labels:** one label in `runs-on`, following `nscloud-{os}-{arch}-{shape}`,
  e.g. `nscloud-ubuntu-22.04-amd64-8x16` or `nscloud-ubuntu-24.04-arm64-4x8`.
  Ubuntu 20.04–26.04 on amd64/arm64, `windows-2022` amd64, and macOS
  sonoma/sequoia/tahoe on arm64 (`6x14`, `12x28`, `12x56`)
  ([runner labels](https://namespace.so/docs/reference/github-actions/runner-configuration)).
  Migration is a one-line `runs-on` change per job. Runner profiles are the
  alternative to inline labels.
- **Pricing:** one unit is 1 vCPU + 2 GB for one minute, with multipliers of Linux 1,
  Windows 2, macOS 10 and Linux on Apple Silicon 7. There is a 1-minute minimum.
  The Developer plan is "Pay as you go" with "No monthly commitment". There is a
  30-day free trial with "No credit card required". Team is $100/month with 100,000
  unit-minutes. "Building open source? Apply to our OSS program", with no terms
  published ([pricing](https://namespace.so/pricing)). The pricing page lists a
  prepaid and an overage price for each shape. Which one applies to the Developer
  plan is not spelled out.
- **Caching:** Cache Volumes are NVMe-backed and persist across runs. A `rust`
  mode in `nscloud-cache-action` caches the Cargo registry, git deps and build
  artifacts; the docs say to disable other Rust toolchain caches. Volumes are per
  profile and per repository, and writes can be limited to chosen branches to avoid
  cache poisoning ([caching](https://namespace.so/docs/solutions/github-actions/caching)).
  An sccache integration also exists ([sccache](https://namespace.so/docs/integrations/sccache)).
  `Swatinem/rust-cache` would be replaced or turned off, not kept alongside.
- **Android emulator:** "KVM-backed Android emulators are currently supported on
  Linux AMD64 runners", with no extra KVM setup
  ([Android emulators](https://namespace.so/docs/integrations/android-emulators),
  [nested virtualization](https://namespace.so/docs/architecture/compute/nestedvirt)).
- **Security:** "We execute every workflow job in a clean isolated virtual
  machine, so untrusted code from public forks cannot compromise the environment",
  and public repos work "without extra configuration"
  ([announcement, 2023](https://namespace.so/blog/completing-the-circle-with-github-runners)).
  Egress policies are available ([egress](https://namespace.so/docs/security/egress-policy)).
- **Speed claim (vendor):** no numbers in the docs. Namespace says users "should
  see immediate performance benefits".
- **Lock-in:** low for the runner itself (one label). Medium if the workflows adopt
  `nscloud-cache-action` / `nscloud-checkout-action`.

### Ubicloud

- **Account and install:** you install the "Ubicloud Managed Runners" GitHub App.
  Migration is a one-line change from `ubuntu-latest` to `ubicloud-standard-2`
  ([quickstart](https://www.ubicloud.com/docs/github-actions-integration/quickstart)).
  The docs don't mention personal accounts. The open-source control plane
  registers each runner with `generate-jitconfig` on the repository path
  `/repos/{repo}/actions/runners`
  ([`prog/github/github_runner_nexus.rb`](https://github.com/ubicloud/ubicloud/blob/8bc62da152963000f2cff34dcaadd96f3fa62565/prog/github/github_runner_nexus.rb)).
  That is the registration GitHub allows for personal-account repos. This is an
  inference from source, not a vendor statement.
- **Labels:** `ubicloud-standard-{2,4,8,16,30}` for x64 and the same sizes with an
  `-arm` suffix. An `-ubuntu-2204` / `-ubuntu-2404` / `-ubuntu-2604` suffix selects
  the OS. Linux only
  ([runner types](https://www.ubicloud.com/docs/github-actions-integration/runner-types)).
- **Pricing:** x64 2 vCPU $0.00125 up to 30 vCPU $0.01875. Premium (Ryzen 9
  7950X3D) 2 vCPU $0.002 up to 30 vCPU $0.030. Arm64 costs the same as standard
  x64. A "$2.5/month credit… equivalent to 1,250 minutes"
  ([pricing](https://www.ubicloud.com/docs/about/pricing)). Premium is on by
  default and falls back to standard when busy. The marketing page quotes
  different headline numbers ($0.0010 / $0.0016 per minute)
  ([use case](https://www.ubicloud.com/use-cases/github-actions)). The docs table
  is used here.
- **Caching:** a transparent cache is on by default and serves `actions/cache`,
  `actions/setup-*` and `Swatinem/rust-cache` with no workflow changes. Docker
  layer caching with `type=gha` also works. 30 GB free per repo (100 GB on
  premium). Unused entries are evicted after 7 days. By default a run reads only
  its own branch's caches or the default branch's
  ([Ubicloud Cache](https://www.ubicloud.com/docs/github-actions-integration/ubicloud-cache)).
- **Security:** "Fully isolated VMs using Linux KVM", a "Clean and ephemeral VM for
  each job", and "GitHub Just-In-Time runner config for ephemeral secrets"
  ([use case](https://www.ubicloud.com/use-cases/github-actions)).
- **Speed claim (vendor):** "Change 1 line. Go twice as fast. Save 4-7x."
- **Gaps:** no macOS, no Windows, and nested KVM isn't documented, so the Android
  grader job stays on GitHub.
- **Lock-in:** minimal. The labels are the only change and the control plane is
  AGPL source.

### RunsOn (self-hosted in your AWS)

- **Model:** "One CloudFormation stack, about ten minutes: its own VPC, the cache
  bucket, IAM roles and the control plane." You create a private GitHub App.
  "Every job gets a fresh EC2 instance that is destroyed when the job ends." Linux
  x64/arm64, Windows and GPU are supported. "macOS jobs stay where they are"
  ([home](https://runs-on.com/)). Source: [runs-on/runs-on](https://github.com/runs-on/runs-on) (MIT).
- **Account:** the prerequisites list "A GitHub organization (or personal
  account) where you can create GitHub Apps"
  ([install](https://runs-on.com/guides/install/)). The install steps are written
  for organizations, and the Fleet mode registers into an org's Default runner
  group, so use the Flex mode on a personal account (not verified end to end).
- **Labels:** `runs-on=${{ github.run_id }}/family=m8+r8/cpu=4`-style, one line per job.
- **Pricing:** you pay the licence plus AWS. "AWS bills you for the instances at
  spot prices, with no markup". The control plane is "From about $7 a month". The
  Free (€0) licence is "for non-profit, open-source and personal non-commercial
  projects". Paid tiers start at €300/yr. Example spot costs: 2 vCPU m8a.large
  $0.0009/min, 8 vCPU c8id.2xlarge $0.0022/min, 2 vCPU arm m8g.large $0.0009/min
  ([pricing](https://runs-on.com/pricing/)).
- **Caching:** with `extras=s3-cache`, `actions/cache` reads and writes an S3
  bucket in your account, with no size cap. There is also ECR-backed Docker layer caching.
- **Android emulator:** a `nested-virt` label on x64 c8i/m8i/r8i families. Not
  available on arm64
  ([nested virtualization](https://runs-on.com/docs/runners/capabilities/nested-virtualization/)).
- **Security:** "Code, secrets, caches, logs and the GitHub App credentials never
  leave your account." The only outbound calls are a licence check and per-launch
  telemetry.
- **Speed claim (vendor):** builds are 2.1–2.6× faster and cache restores
  2.3–2.9× faster than GitHub-hosted.
- **Caveats:** you need an AWS account and someone to operate it. Whether
  Alexandria counts as "non-commercial" needs a decision, because
  `alexandria-cloud` sells hosted services to organisations; if it doesn't, the
  €300/yr Starter licence applies.

### Tenki

- Its FAQ says "You can use Tenki for personal projects: there's no need to be part
  of a GitHub Organization. You can get started and onboard using your personal
  GitHub account with a private profile" ([home FAQ](https://tenki.cloud/)). The
  install docs, though, are written for organizations: "A Tenki Workspace can be
  connected to only one GitHub Organization"
  ([connect GitHub](https://tenki.cloud/docs/github/gh-organization)). It's unclear
  what "with a private profile" requires.
- Linux x64 is $0.002 per vCPU-minute and macOS (M4 Pro) $0.020 per vCPU-minute.
  The free Starter plan has a $100 one-time credit, machines up to 4 cores / 8 GB
  and 5 concurrent sessions ([pricing](https://www.tenki.cloud/pricing),
  [docs pricing](https://tenki.cloud/docs/pricing)).
- Labels look like `tenki-standard-medium-4c-8g`
  ([quickstart](https://tenki.cloud/docs/runners/quickstart)). Linux arm64,
  Windows and KVM are not documented. The free tier's 4-core cap is no bigger than
  GitHub's free public 4-vCPU runner.

### Depot (runners and Depot CI)

- **Runners:** org-only (quoted above). Linux x64/arm64 run 2–64 vCPU at
  $0.006–$0.192, Windows $0.008–$0.256, and macOS $0.08 on M2/M4 with 8 vCPU / 24 GB
  ([runner types](https://depot.dev/docs/github-actions/runner-types)). Plans are
  $20/month for 2,000 min or $200/month for 20,000 min, with a 7-day trial
  ([pricing](https://depot.dev/pricing)). Builds run on "ephemeral EC2 instances
  that are never reused". Vendor claims: runners are "up to 3x faster" and the
  cache "10x faster". /dev/kvm is not provided on these runners
  ([troubleshooting](https://depot.dev/docs/github-actions/troubleshooting)).
- **Depot CI:** "Depot CI supports repositories owned by both GitHub
  organizations and personal accounts"
  ([overview](https://depot.dev/docs/github-actions/overview)). It is a separate CI
  system. Workflows live in `.depot/workflows/`. "Depot CI doesn't provide
  sandboxes for macOS or Windows." `runs-on` labels other than Depot's are treated
  as `depot-ubuntu-latest`. `services:` is supported, and nested virtualization is
  on by default
  ([Depot CI](https://depot.dev/docs/ci/overview),
  [compatibility](https://depot.dev/docs/ci/compatibility),
  [nested virt](https://depot.dev/blog/now-available-nested-virtualization-on-depot-ci)).
  It isn't a one-line migration: it means a second workflow tree and moving
  secrets over. This research didn't confirm how its results report back as
  required PR checks.

### WarpBuild

- Org-only: "Repositories under a personal account cannot receive the runners"
  ([migration guide](https://www.warpbuild.com/migrate/github-hosted-to-warpbuild)).
- Otherwise a full lineup: Linux x64 2 vCPU $0.004, arm64 2 vCPU $0.003, Windows 4
  vCPU $0.016, macOS M4 Pro $0.08, and BYOC on AWS/GCP/Azure at $0.002
  ([pricing](https://www.warpbuild.com/pricing)). KVM is available on Linux x64.
  Its cache action is "a drop-in replacement for `actions/cache@v4`" and is not
  supported on Windows.

### Ruled out or dead

- **BuildJet:** "BuildJet for GitHub Actions has been shut down"
  ([site](https://buildjet.com/for-github-actions)). Announced 2026-02-06; jobs
  stopped 2026-03-31
  ([shutdown post](https://buildjet.com/for-github-actions/blog/we-are-shutting-down)).
- **Cirrus Runners:** "We are no longer accepting new customers for Cirrus
  Runners". Existing contracts are honoured, and Cirrus CI shut down on 2026-06-01
  ([announcement, 2026-04-07](https://cirruslabs.org/)). The Tart VM tool lives
  on, now FSL-1.1-ALv2 ([LICENSE](https://github.com/cirruslabs/tart/blob/bcc8084a22b1ab1cc7162626537d1c6cd794cdba/LICENSE)).
  That matters for the spare-Mac option below.
- **Actuated:** still active, but "a GitHub organisation is required for
  installation". Linux only, and you supply the servers at $150/server/month
  ([FAQ](https://docs.actuated.com/faq), [pricing](https://actuated.com/pricing)).
- **Buildkite hosted agents:** Linux $0.004 and macOS $0.02 per vCPU-minute. The
  free plan includes 2,000 Linux vCPU-minutes. It uses its own `pipeline.yaml`
  format, so this would be a CI migration, not a runner swap
  ([pricing](https://buildkite.com/pricing)).
- **GitHub larger runners:** no personal-account path is documented (see Baseline).

### Self-hosted runners, ARC, or a spare Mac

- Free in GitHub billing ("free to use with GitHub Actions, but you are
  responsible for the cost of maintaining your runner machines",
  [concepts](https://docs.github.com/en/actions/concepts/runners/self-hosted-runners)).
  Supported on Linux, Windows and macOS 11+, x64 and ARM64
  ([reference](https://docs.github.com/en/actions/reference/runners/self-hosted-runners)).
  On a personal account you register each runner per repository.
- ARC is "a Kubernetes operator that orchestrates and scales self-hosted runners"
  ([ARC](https://docs.github.com/en/actions/concepts/runners/actions-runner-controller)).
  It's Linux-container based and needs a cluster, which is heavy for this project.
- **Public-repo risk:** GitHub says self-hosted runners "should almost never be
  used for public repositories" because "any user can open pull requests against
  the repository and compromise the environment"
  ([secure use](https://docs.github.com/en/actions/reference/security/secure-use)).
  `alexandria`'s `ci.yml` runs on `pull_request`, so a spare Mac would be
  acceptable only for:
  - private repos;
  - `workflow_dispatch` or `push`-to-default-branch jobs (e.g. release builds), behind
    ephemeral VMs (Tart) or JIT runners.

  macOS minutes on the public app repo are already free on GitHub, so the case for
  a Mac runner is speed only.

## Fit to Alexandria's workloads

| Workload | Best fit | Why |
|---|---|---|
| `Lint & Test` Rust/Tauri (`alexandria`, public) | Namespace 8x16 or 16x32; Ubicloud standard-8/16 | GitHub's free public runner is already 4 vCPU / 16 GB, so only an 8+ vCPU runner with a persistent cache is a real upgrade |
| `Android grader (on-device)` | GitHub-hosted (free) or Namespace amd64 | Needs `/dev/kvm`. Namespace documents it; Ubicloud and Tenki don't; Depot runners don't have it |
| iOS / macOS builds | GitHub-hosted `macos-15` (free on public) | Vendor macOS costs $0.06–$0.08/min against $0 today |
| Windows desktop build | GitHub-hosted (free on public) | Same reasoning |
| `ubuntu-22.04-arm` build | GitHub-hosted (free on public) | Same; Namespace/Ubicloud arm64 only if speed matters |
| `alexandria-cloud` (private, Postgres `services:`) | Stay on GitHub, or Namespace/Ubicloud x64 | Likely within the $12 included compute. Service-container support on Namespace/Ubicloud is not confirmed in their docs; test it |
| `alexandria-relay`, `website`, `monitoring` | Stay on GitHub | Short jobs; public ones are free |

Fork PRs: with the `pull_request` trigger, "secrets are not passed to the runner"
for forks (except `GITHUB_TOKEN`), and "The `GITHUB_TOKEN` has read-only
permissions in pull requests from forked repositories"
([events](https://docs.github.com/en/actions/reference/workflows-and-actions/events-that-trigger-workflows)).
This holds on vendor runners too. The residual risk is fork code running on vendor
VMs that are billed to you and that can write to shared caches. Namespace's
branch-limited cache writes and Ubicloud's branch-scoped cache reads address the
cache-poisoning part.

## Cost sketch (estimate, not a quote)

These figures are derived from `gh run list` counts for October 2026 and the list
prices above.

- **`alexandria`:** 145 CI runs. Upper bound: every run executes a 45-minute
  `Lint & Test`, about 6,500 runner-minutes, currently $0 on GitHub's free public
  runner. On an 8-vCPU runner at the same wall-clock time:

  | Vendor (8 vCPU) | Monthly upper bound |
  |---|---|
  | Namespace 8x16 | ≈ $52–$78 |
  | Ubicloud standard / premium | ≈ $33 / $52 |
  | RunsOn spot | ≈ $14 + ~$7 control plane + licence |
  | Tenki | ≈ $104 |

  Any real speedup or skipped runs reduce these proportionally. This is spending
  money to save time, not to save money.
- **`alexandria-cloud`:** 59 CI runs at about 15 job-minutes each is about 900
  Linux minutes, or about $5.40 at $0.006. That is inside the $12 included compute,
  so moving it saves nothing unless usage grows. `alexandria-monitoring` is smaller.

## Ranked recommendation

1. **Namespace for heavy Linux jobs in `alexandria`; GitHub-hosted for
   everything else (hybrid).** It is the only managed vendor that states
   personal-account support in its docs and also covers KVM, Linux x64/arm64,
   macOS and Windows if needed later. The change per job is one line
   (`runs-on: nscloud-ubuntu-22.04-amd64-8x16`). Start with `Lint & Test`, compare
   wall-clock time across a week, then decide whether the Android grader moves too.
2. **Ubicloud for heavy Linux jobs (cheaper, Linux-only).** It costs the least
   per vCPU of the managed options, has a transparent cache that keeps
   `Swatinem/rust-cache` working unchanged, and its control plane is open source.
   Two risks: personal-account support is inferred from source code, not
   documented, and the Android emulator job can't move. Verify with a trial install
   on one public repo.
3. **RunsOn in a dedicated AWS account.** It has the lowest compute price, keeps
   secrets and caches in your account, and documents personal accounts and
   nested virtualization. It also has the most operational work: an AWS account,
   stack upgrades, and a licence decision on whether Alexandria is
   "non-commercial".

Not recommended now: Tenki (newer vendor; no arm64/Windows/KVM documented; free
tier capped at 4 cores), Depot CI (a separate CI system with no macOS/Windows), a
self-hosted spare Mac on the public app repo (GitHub's security guidance), and
Buildkite (not drop-in).

**If Blacksmith requires an org (it does, per its quickstart), the best option
that doesn't transfer the repos is #1, Namespace**, with Ubicloud as the
lower-cost Linux-only alternative. Moving the repos into a GitHub organization
would also open up Blacksmith, Depot runners and WarpBuild. Weigh that move
separately; it is out of scope here.

## Unconfirmed or open

- **Ubicloud personal accounts:** inferred from source code (repo-level JIT
  registration), not stated in the docs.
- **Namespace Developer-plan rate:** whether the prepaid or the overage price
  applies to pay-as-you-go is not spelled out.
- **Namespace Linux arm64 price:** the AmpereOne per-minute rate isn't listed
  separately. The multiplier table implies the Linux rate (×1).
- **Namespace OSS programme:** advertised, with no terms published.
- **Tenki:** what "with a private profile" means for a personal account, and
  whether Linux arm64, Windows and KVM are available.
- **RunsOn on a personal account:** the Flex mode on a personal account isn't
  verified end to end. The install docs assume an organization.
- **Postgres service containers:** `services:` support isn't confirmed for
  Namespace, Ubicloud or Tenki (Depot CI documents it).
- **Depot CI PR checks:** how its results surface as required checks on GitHub
  PRs isn't confirmed.
- **Self-hosted fee:** GitHub postponed the $0.002/min self-hosted platform
  charge with no new date. All options here register self-hosted runners and
  would pay it if it returns.
- **Speedups:** every speedup figure is a vendor claim. None was measured on
  Alexandria's workflows.
