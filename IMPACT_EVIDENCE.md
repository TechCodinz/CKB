# CKB Impact Evidence Register

This file is the canonical register for verifiable CKB adoption, technical contribution, commercial impact and independent recognition. It is designed to preserve evidence as it happens instead of reconstructing it later.

## Evidence rules

1. Record only observed facts and measured values.
2. Every metric or achievement must point to a source: marketplace analytics, GitHub analytics, release artifact, customer record, public article, conference listing, independent review, invoice, export or other reproducible record.
3. Never convert estimates, memory or marketing language into evidence.
4. Keep sensitive customer and commercial documents outside the public repository; record only redacted references here.
5. Use dated snapshots so growth can be demonstrated over time.

## Recognition

Track evidence that other people or organisations recognise CKB or its creator's work, including:

- VS Code Marketplace installs/downloads and ratings
- GitHub stars, forks, contributors and outside pull requests
- Independent developer reviews or technical write-ups
- Public references to CKB in developer communities
- Invitations to speak, demonstrate or teach the technology
- Awards, accelerators, grants or recognised competitions
- Third-party integrations or organisations adopting CKB

## Innovation and technical contribution

Track major releases and the specific engineering contribution behind them, for example:

- architecture graph and dependency analysis
- blast-radius and change-impact analysis
- runtime/telemetry fusion
- semantic clone detection
- API contract validation
- test-gap analysis
- MCP/agent integrations
- language/runtime expansion
- performance, reliability and security improvements

For every milestone, link the commit, release or design document and record any measurable outcome.

## Commercial impact

Track paying organisations, pilots, subscriptions, enterprise deployments, renewals and documented customer outcomes. Do not publish confidential contract values or personal customer information.

## Evidence log

| Date | Category | Metric / achievement | Value | Evidence reference | Independent? | Notes |
|---|---|---|---|---|---|---|
| 2026-08-14 | recognition | GitHub stars | 0 | GitHub repository API snapshot, TechCodinz/CKB | Yes | Baseline; do not treat 0 as Marketplace adoption. |
| 2026-08-14 | recognition | GitHub forks | 0 | GitHub repository API snapshot, TechCodinz/CKB | Yes | Baseline. |
| 2026-08-14 | community | GitHub open issues | 7 | GitHub repository API snapshot, TechCodinz/CKB | Yes | Baseline; open issues are activity, not automatically independent recognition. |
| 2026-08-14 | release | GitHub Releases | 0 published releases | GitHub Releases API, TechCodinz/CKB | Yes | Marketplace publication/versioning is tracked separately and must not be inferred from GitHub Releases. |
| 2026-08-14 | recognition | VS Code Marketplace installs/downloads/ratings | unavailable in this capture | Microsoft Visual Studio Marketplace | Yes | Source query did not return the CKB listing metrics in this run; unavailable is intentionally not recorded as zero. |
| 2026-08-15 | distribution | Open VSX public extension publication | TechCodinz.ckb-vscode v1.10.1 | GitHub Actions run 31893247342, job 95032559025; Open VSX API/listing verification | Yes | Verification job resolved Open VSX latest as 1.10.1, downloaded and inspected the published VSIX, matched publisher/name/version and main entrypoint, and confirmed the public listing was reachable. This is external distribution evidence, not a claim of adoption or independent recognition. |

## Monthly snapshot

At least once per month, capture:

- Marketplace installs/downloads
- GitHub stars/forks/watchers
- External contributors and merged outside PRs
- Active organisations or verified deployments where measurable
- Releases shipped
- Public mentions/reviews
- Revenue/customers where applicable

Store the original export or screenshot outside Git when it contains sensitive information and reference its date and location here.

## Case-study record

For each strong external deployment, capture the problem, environment, CKB capability used, measurable result, customer/third-party confirmation, date range and source reference. A case study should demonstrate impact rather than merely describe features.
