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
| 2026-08-15 | technical contribution | JetBrains modern compatibility line | CKB JetBrains v1.9.0 supports IntelliJ Platform 2024.3.6 and verification through IntelliJ IDEA 2026.2.0.1 | TechCodinz/CKB PR #12; merge commit 7f4b0706d09fec8dc7c77e0af4702864b6c89cd5 | No | Modernized to IntelliJ Platform Gradle Plugin 2.18.1 and Java 21 with plugin-verifier gates. No live JetBrains Marketplace listing or adoption is claimed from this milestone. |
| 2026-08-17 | technical contribution | Universal Model Gateway + remote MCP consolidated on main | 13 scoped tools; 137 workspace tests passing; 20 new MCP-server tests | TechCodinz/CKB PR #15; merge commit 3fc7c574f52522f83b374af022f22af2163632e9 | No | Adds the canonical scoped Reality tool registry, Streamable HTTP `/mcp`, provider-neutral `/llm/*` gateway, RFC 9728 resource metadata, Cloud token introspection and per-user project isolation. This is implementation evidence, not external recognition. |
| 2026-08-17 | release readiness | VS Code extension v1.10.2 packaged successfully | 336.3 KB VSIX; npm audit 0 vulnerabilities; package artifact uploaded | GitHub Actions run 32006506303, job 95316972488; merge commit fd468e9e544b62069154169e1f43ac785efb783d | No | Compile and VSIX packaging succeeded. Marketplace publication failed during authentication, so v1.10.2 must not be claimed as publicly released yet. |
| 2026-08-17 | distribution blocker | VS Code Marketplace v1.10.2 publish blocked | OIDC token exchange returned 404 at `/_apis/gallery/token`; no `VSCE_TOKEN` configured | GitHub Actions run 32006506303, job 95316972488 | Yes | Requires Marketplace trusted-publishing support/configuration or a valid Marketplace PAT secret. Do not count v1.10.2 as Marketplace-published until a later run verifies success. |
| 2026-08-17 | distribution status | Open VSX remains on v1.10.1 | expected 1.10.2, public API returned 1.10.1 | GitHub Actions run 32006506287, job 95316972414 | Yes | Public-package smoke correctly failed on the version mismatch. v1.10.1 remains the latest verified public Open VSX package in this capture. |
| 2026-08-17 | community | GitHub open issues | 9 | GitHub repository API snapshot, TechCodinz/CKB | Yes | Activity increased from the 2026-08-14 baseline of 7; this is activity, not automatically recognition. |
| 2026-08-18 | community | GitHub open issues | 3 | GitHub repository API snapshot, TechCodinz/CKB, captured 2026-08-18 | Yes | Open-issue count decreased from 9 on 2026-08-17 to 3. This records repository activity, not adoption or recognition. |
| 2026-08-18 | release engineering | Exact-artifact Marketplace publish path hardened on main | guarded exact-artifact publishing, locked-toolchain/Linux release preflight and Marketplace reconciliation logic consolidated | TechCodinz/CKB commit a5685b318532c6d571879b9fe9c9cedb62b47e98 | No | Release-engineering milestone only. No successful VS Code Marketplace publication for v1.10.2 was verified in this capture, so the 2026-08-17 publication blocker remains the latest source-backed distribution status. |
| 2026-08-19 | community | GitHub open issues | 4 | GitHub repository API snapshot, TechCodinz/CKB, captured 2026-08-19 | Yes | Open-issue count increased from 3 on 2026-08-18 to 4. Stars and forks remain 0 in the same snapshot. This records repository activity, not adoption or recognition. |
| 2026-08-20 | technical contribution | Evidence-first Live Execution Twin / Software MRI core merged | PR #20 merged; five current-head repository suites reported green; first-party Node, Python, Go, Rust, browser, Java and .NET runtime collection paths included | TechCodinz/CKB PR #20; merge commit 21ea69fa3343bdebc7d2ee1059d20b29393f4f6b | No | Adds observed execution intelligence, runtime diagnostics and evolution memory while preserving STATIC/OBSERVED/PREDICTED evidence boundaries and privacy gates. This is verified implementation/CI evidence, not production deployment or external adoption. |
| 2026-08-20 | community | GitHub stars / forks / open issues | 0 / 0 / 4 | GitHub repository API snapshot, TechCodinz/CKB, captured 2026-08-20 | Yes | Unchanged from the 2026-08-19 star/fork/issue snapshot; recorded for continuity, not as a growth milestone. |
| 2026-08-23 | community | GitHub stars / forks / open issues | 0 / 0 / 4 | GitHub repository API snapshot, TechCodinz/CKB, captured 2026-08-23 | Yes | Unchanged from the 2026-08-20 snapshot; recorded for continuity, not as a growth milestone. |
| 2026-08-23 | distribution status | VS Code Marketplace metrics/publication verification | unavailable in this capture | Microsoft Visual Studio Marketplace public search for `TechCodinz.ckb-vscode`, captured 2026-08-23 | Yes | Exact extension queries did not surface a public listing or metrics. Install/download count, rating average/count, version and update timestamps therefore remain unavailable; v1.10.2 must not be claimed as Marketplace-published from this capture. |
| 2026-08-25 | community | GitHub stars / forks / open issues | 0 / 0 / 4 | GitHub repository API snapshot, TechCodinz/CKB, captured 2026-08-25 | Yes | Unchanged from the 2026-08-23 snapshot; recorded for continuity, not as a growth milestone. |
| 2026-08-25 | distribution status | VS Code Marketplace metrics/publication verification | unavailable in this capture | Microsoft Visual Studio Marketplace exact-extension public query for `TechCodinz.ckb-vscode`, captured 2026-08-25 | Yes | The exact extension query still did not surface a public Marketplace listing or metrics. Install/download count, rating average/count, version and publication/update timestamps remain unavailable; v1.10.2 is still not source-verified as Marketplace-published. |
| 2026-08-27 | community | GitHub stars / forks / open issues | 0 / 0 / 5 | GitHub repository API snapshot, TechCodinz/CKB, captured 2026-08-27 | Yes | Open issues-or-PRs increased from 4 on 2026-08-25 to 5; stars and forks remain unchanged. This is repository activity, not adoption or recognition. |
| 2026-08-27 | technical contribution | Verified capability model selector added on main | commit 56a45154bc4a77dde6f74e56796a121aa310c699 | TechCodinz/CKB commit 56a45154bc4a77dde6f74e56796a121aa310c699 | No | Adds a capability-based model selector and selection API constrained to allowed providers. This is source-backed implementation evidence, not production deployment or independent recognition. |
| 2026-08-27 | deployment blocker | Render deployment pipeline blocked before build | `ckb-mcp-server` and `ckb-model-registry` builds canceled because workspace build-pipeline minutes are exhausted | Render service/deploy logs for srv-d9jpqugu01pc73c2vvjg and srv-da3f0uht0dsc73fhm880, captured 2026-08-27 | Yes | The latest CKB repository changes are not source-verified as deployed to these Render services. This is an infrastructure/billing-capacity blocker rather than a compiler failure. |
| 2026-08-27 | distribution status | VS Code Marketplace metrics/publication verification | unavailable in this capture | Microsoft Visual Studio Marketplace exact-extension public search for `TechCodinz.ckb-vscode`, captured 2026-08-27 | Yes | Exact public queries again returned no listing. Install/download count, web/download count, rating average/count, version and publication/update timestamps remain unavailable; v1.10.2 is still not source-verified as Marketplace-published. |
| 2026-09-02 | community | GitHub stars / forks / open issues-or-PRs | 0 / 0 / 6 | GitHub repository API snapshot, TechCodinz/CKB, captured 2026-09-02 | Yes | Open issues-or-PRs increased from 5 on 2026-08-27 to 6. The new open item is owner-authored PR #27, so this is repository activity and not an outside contribution or independent recognition. |
| 2026-09-02 | distribution status | VS Code Marketplace metrics/publication verification | unavailable in this capture | Microsoft Visual Studio Marketplace exact public search for `TechCodinz.ckb-vscode`, captured 2026-09-02 | Yes | Exact searches did not surface the CKB extension listing. Install/download count, web/download count, rating average/count, version and publication/update timestamps remain unavailable; unavailable values are not converted to zero and v1.10.2 remains unverified as Marketplace-published. |

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